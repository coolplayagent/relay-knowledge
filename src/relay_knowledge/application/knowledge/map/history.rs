//! Bounded Knowledge Map history paging and archive-chain validation.

use crate::{
    api::RequestContext, domain::KnowledgeMapHistoryEntry, project::KNOWLEDGE_MAP_HISTORY_DIR_NAME,
};

use super::{
    KnowledgeMapHistoryResponse, KnowledgeMapService, KnowledgeMapServiceError,
    artifact::{
        ARTIFACT_SCHEMA_VERSION, HISTORY_INDEX_FANOUT, HISTORY_INDEX_MAX_HEIGHT,
        KnowledgeMapArchiveRef, KnowledgeMapHistoryArchive, KnowledgeMapHistoryIndexEntry,
        KnowledgeMapHistoryIndexNode, KnowledgeMapHistoryIndexRef, KnowledgeMapHistoryIndexTarget,
        KnowledgeMapHistoryManifest, KnowledgeMapManifest, KnowledgeMapSchemaProbe,
        LEGACY_ARTIFACT_SCHEMA_VERSION, RECENT_HISTORY_LIMIT, content_digest, parse_manifest,
        read_verified_ref_in, serialize_yaml, validate_history_index_ref_shape,
        validate_recent_history,
    },
    contracts::metadata,
    publish_immutable_in,
};

pub(crate) const MAX_HISTORY_PAGE_SIZE: usize = 256;
pub(super) const MAX_HISTORY_LOOKUP_READS: usize = HISTORY_INDEX_MAX_HEIGHT as usize + 2;
pub(super) const MISSING_HISTORY_INDEX_MESSAGE: &str =
    "history archive index is missing; run `relay-knowledge map init` to migrate this v2 map";

impl KnowledgeMapService {
    pub async fn history(
        &self,
        context: &RequestContext,
        from_version: u64,
        limit: usize,
    ) -> Result<KnowledgeMapHistoryResponse, KnowledgeMapServiceError> {
        if from_version == 0 || limit == 0 || limit > MAX_HISTORY_PAGE_SIZE {
            return Err(KnowledgeMapServiceError::InvalidRequest(format!(
                "history from_version must be positive and limit must be within 1..={MAX_HISTORY_PAGE_SIZE}"
            )));
        }
        let (map_version, entries) = self.load_history_page(from_version, limit).await?;
        let through_version = entries
            .last()
            .map_or(from_version.saturating_sub(1), |entry| entry.version);
        let next_from_version = (through_version < map_version)
            .then(|| through_version.checked_add(1))
            .flatten();
        Ok(KnowledgeMapHistoryResponse {
            metadata: metadata(context),
            path: self.relative_path().to_owned(),
            map_type: self.map_type,
            map_version,
            from_version,
            through_version,
            next_from_version,
            entries,
        })
    }

    async fn load_history_page(
        &self,
        from_version: u64,
        limit: usize,
    ) -> Result<(u64, Vec<KnowledgeMapHistoryEntry>), KnowledgeMapServiceError> {
        let content = self.read_root_content().await?;
        let probe = serde_norway::from_str::<KnowledgeMapSchemaProbe>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        match probe.schema_version {
            1 => {
                let map = super::parse_v1_map(&content)?;
                let entries = map
                    .history
                    .into_iter()
                    .filter(|entry| entry.version >= from_version)
                    .take(limit)
                    .collect();
                Ok((map.map_version, entries))
            }
            LEGACY_ARTIFACT_SCHEMA_VERSION | ARTIFACT_SCHEMA_VERSION => {
                let manifest = parse_manifest(&content)?;
                self.validate_manifest_identity(&manifest)?;
                let entries = self
                    .load_v2_history_page(&manifest, from_version, limit)
                    .await?;
                Ok((manifest.map_version, entries))
            }
            version => Err(KnowledgeMapServiceError::Yaml(format!(
                "unsupported schema_version {version}"
            ))),
        }
    }

    async fn load_v2_history_page(
        &self,
        manifest: &KnowledgeMapManifest,
        from_version: u64,
        limit: usize,
    ) -> Result<Vec<KnowledgeMapHistoryEntry>, KnowledgeMapServiceError> {
        validate_recent_history(manifest)?;
        if from_version > manifest.map_version {
            return Ok(Vec::new());
        }
        let through_version = from_version
            .saturating_add(limit as u64 - 1)
            .min(manifest.map_version);
        let mut entries = manifest
            .history
            .recent
            .iter()
            .filter(|entry| (from_version..=through_version).contains(&entry.version))
            .cloned()
            .collect::<Vec<_>>();
        if from_version <= manifest.history.archived_through {
            manifest.history.archive.as_ref().ok_or_else(|| {
                KnowledgeMapServiceError::Integrity(
                    "history archive is missing for a non-zero checkpoint".to_owned(),
                )
            })?;
            let index = manifest.history.index.as_ref().ok_or_else(|| {
                KnowledgeMapServiceError::Integrity(MISSING_HISTORY_INDEX_MESSAGE.to_owned())
            })?;
            let mut target = from_version;
            while target <= through_version && target <= manifest.history.archived_through {
                let (archive, _, reads) = self.load_indexed_history_archive(index, target).await?;
                if reads > MAX_HISTORY_LOOKUP_READS {
                    return Err(KnowledgeMapServiceError::Integrity(
                        "history archive index exceeded its lookup read budget".to_owned(),
                    ));
                }
                entries.extend(
                    archive
                        .entries
                        .iter()
                        .filter(|entry| (from_version..=through_version).contains(&entry.version))
                        .cloned(),
                );
                target = archive.through_version.checked_add(1).ok_or_else(|| {
                    KnowledgeMapServiceError::Integrity("history version overflow".to_owned())
                })?;
            }
        }
        entries.sort_by_key(|entry| entry.version);
        let mut expected = from_version;
        for entry in &entries {
            if entry.version != expected {
                return Err(KnowledgeMapServiceError::Integrity(
                    "requested history page is not contiguous".to_owned(),
                ));
            }
            expected = expected.checked_add(1).ok_or_else(|| {
                KnowledgeMapServiceError::Integrity("history version overflow".to_owned())
            })?;
        }
        if entries.len() != (through_version - from_version + 1) as usize {
            return Err(KnowledgeMapServiceError::Integrity(
                "requested history page is incomplete".to_owned(),
            ));
        }
        Ok(entries)
    }

    pub(super) async fn validate_archived_history(
        &self,
        history: &KnowledgeMapHistoryManifest,
    ) -> Result<(), KnowledgeMapServiceError> {
        let contract_dir = self.read_contract_dir_name().await?;
        self.validate_archived_history_in(contract_dir, history)
            .await
    }

    pub(super) async fn validate_archived_history_in(
        &self,
        contract_dir: &str,
        history: &KnowledgeMapHistoryManifest,
    ) -> Result<(), KnowledgeMapServiceError> {
        let Some(mut archive_ref) = history.archive.clone() else {
            return if history.archived_through == 0 {
                Ok(())
            } else {
                Err(KnowledgeMapServiceError::Integrity(
                    "history archive is missing for a non-zero checkpoint".to_owned(),
                ))
            };
        };
        let mut expected_through = history.archived_through;
        loop {
            let archive = self
                .load_history_archive_in(contract_dir, &archive_ref, expected_through)
                .await?;
            if let Some(index) = &history.index {
                let (_, indexed_ref, _) = self
                    .load_indexed_history_archive_in(contract_dir, index, archive.from_version)
                    .await?;
                if indexed_ref != archive_ref {
                    return Err(KnowledgeMapServiceError::Integrity(
                        "history archive index disagrees with the canonical archive chain"
                            .to_owned(),
                    ));
                }
            }
            expected_through = archive.from_version - 1;
            match archive.previous {
                Some(previous) => archive_ref = previous,
                None if expected_through == 0 => return Ok(()),
                None => {
                    return Err(KnowledgeMapServiceError::Integrity(
                        "history archive chain ends before version 1".to_owned(),
                    ));
                }
            }
        }
    }

    pub(super) async fn ensure_history_index(
        &self,
        history: &KnowledgeMapHistoryManifest,
    ) -> Result<Option<KnowledgeMapHistoryIndexRef>, KnowledgeMapServiceError> {
        if history.archived_through == 0 {
            return Ok(None);
        }
        if let Some(index) = &history.index {
            return Ok(Some(index.clone()));
        }
        let mut archive_ref = history.archive.clone().ok_or_else(|| {
            KnowledgeMapServiceError::Integrity(
                "history archive is missing for a non-zero checkpoint".to_owned(),
            )
        })?;
        let mut expected_through = history.archived_through;
        let mut index = None;
        loop {
            let archive = self
                .load_history_archive(&archive_ref, expected_through)
                .await?;
            index = Some(
                self.prepend_history_index(index, archive_ref.clone(), &archive)
                    .await?,
            );
            expected_through = archive.from_version - 1;
            match archive.previous {
                Some(previous) => archive_ref = previous,
                None if expected_through == 0 => break,
                None => {
                    return Err(KnowledgeMapServiceError::Integrity(
                        "history archive chain ends before version 1".to_owned(),
                    ));
                }
            }
        }
        Ok(index)
    }

    pub(super) async fn append_history_index(
        &self,
        index: Option<KnowledgeMapHistoryIndexRef>,
        archive_ref: KnowledgeMapArchiveRef,
        archive: &KnowledgeMapHistoryArchive,
    ) -> Result<KnowledgeMapHistoryIndexRef, KnowledgeMapServiceError> {
        self.update_history_index(index, archive_ref, archive, false)
            .await
    }

    async fn prepend_history_index(
        &self,
        index: Option<KnowledgeMapHistoryIndexRef>,
        archive_ref: KnowledgeMapArchiveRef,
        archive: &KnowledgeMapHistoryArchive,
    ) -> Result<KnowledgeMapHistoryIndexRef, KnowledgeMapServiceError> {
        self.update_history_index(index, archive_ref, archive, true)
            .await
    }

    async fn update_history_index(
        &self,
        index: Option<KnowledgeMapHistoryIndexRef>,
        archive_ref: KnowledgeMapArchiveRef,
        archive: &KnowledgeMapHistoryArchive,
        prepend: bool,
    ) -> Result<KnowledgeMapHistoryIndexRef, KnowledgeMapServiceError> {
        let archive_entry = archive_index_entry(archive_ref, archive);
        let Some(root) = index else {
            return self.publish_index_node(0, vec![archive_entry]).await;
        };
        if prepend {
            if archive.through_version.checked_add(1) != Some(root.from_version) {
                return Err(noncontiguous_index());
            }
        } else if root.through_version.checked_add(1) != Some(archive.from_version) {
            return Err(noncontiguous_index());
        }
        let mut path = Vec::new();
        let mut current = root;
        loop {
            let node = self.load_history_index_node(&current).await?;
            if node.height == 0 {
                path.push(node);
                break;
            }
            let child = if prepend {
                node.entries.first()
            } else {
                node.entries.last()
            }
            .and_then(index_node_ref)
            .ok_or_else(invalid_index)?;
            path.push(node);
            current = child;
        }
        let mut replacements = {
            let mut leaf = path.pop().expect("index path contains a leaf").entries;
            if prepend {
                leaf.insert(0, archive_entry);
            } else {
                leaf.push(archive_entry);
            }
            self.publish_index_level(0, leaf).await?
        };
        while let Some(parent) = path.pop() {
            let mut entries = parent.entries;
            if prepend {
                entries.remove(0);
                for replacement in replacements.into_iter().rev() {
                    entries.insert(0, node_index_entry(replacement));
                }
            } else {
                entries.pop();
                entries.extend(replacements.into_iter().map(node_index_entry));
            }
            replacements = self.publish_index_level(parent.height, entries).await?;
        }
        if replacements.len() == 1 {
            return replacements.pop().ok_or_else(invalid_index);
        }
        let height = replacements[0]
            .height
            .checked_add(1)
            .ok_or_else(invalid_index)?;
        if height > HISTORY_INDEX_MAX_HEIGHT {
            return Err(KnowledgeMapServiceError::Integrity(
                "history archive index exceeds its maximum depth".to_owned(),
            ));
        }
        self.publish_index_node(
            height,
            replacements.into_iter().map(node_index_entry).collect(),
        )
        .await
    }

    async fn publish_index_level(
        &self,
        height: u8,
        entries: Vec<KnowledgeMapHistoryIndexEntry>,
    ) -> Result<Vec<KnowledgeMapHistoryIndexRef>, KnowledgeMapServiceError> {
        let Some(split) = balanced_index_split(entries.len()) else {
            return Ok(vec![self.publish_index_node(height, entries).await?]);
        };
        let right = entries[split..].to_vec();
        let mut refs = vec![
            self.publish_index_node(height, entries[..split].to_vec())
                .await?,
        ];
        refs.push(self.publish_index_node(height, right).await?);
        Ok(refs)
    }

    async fn publish_index_node(
        &self,
        height: u8,
        entries: Vec<KnowledgeMapHistoryIndexEntry>,
    ) -> Result<KnowledgeMapHistoryIndexRef, KnowledgeMapServiceError> {
        validate_index_entries(height, &entries)?;
        let node = KnowledgeMapHistoryIndexNode {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            from_version: entries.first().expect("validated entries").from_version,
            through_version: entries.last().expect("validated entries").through_version,
            height,
            entries,
        };
        let yaml = serialize_yaml(&node)?;
        let digest = content_digest(yaml.as_bytes());
        let relative = format!(
            "{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/index-{height:02}-{:020}-{:020}-{digest}.yaml",
            node.from_version, node.through_version
        );
        publish_immutable_in(
            &self.repository_root,
            self.contract_dir_name(),
            &relative,
            yaml.as_bytes(),
        )
        .await?;
        Ok(KnowledgeMapHistoryIndexRef {
            from_version: node.from_version,
            through_version: node.through_version,
            height,
            r#ref: relative,
            digest,
        })
    }

    pub(super) async fn load_indexed_history_archive(
        &self,
        root: &KnowledgeMapHistoryIndexRef,
        version: u64,
    ) -> Result<(KnowledgeMapHistoryArchive, KnowledgeMapArchiveRef, usize), KnowledgeMapServiceError>
    {
        let contract_dir = self.read_contract_dir_name().await?;
        self.load_indexed_history_archive_in(contract_dir, root, version)
            .await
    }

    async fn load_indexed_history_archive_in(
        &self,
        contract_dir: &str,
        root: &KnowledgeMapHistoryIndexRef,
        version: u64,
    ) -> Result<(KnowledgeMapHistoryArchive, KnowledgeMapArchiveRef, usize), KnowledgeMapServiceError>
    {
        if version < root.from_version || version > root.through_version {
            return Err(KnowledgeMapServiceError::Integrity(
                "requested history version is outside the archive index".to_owned(),
            ));
        }
        let mut current = root.clone();
        let mut reads = 0;
        loop {
            reads += 1;
            let node = self
                .load_history_index_node_in(contract_dir, &current)
                .await?;
            let entry = node
                .entries
                .iter()
                .find(|entry| (entry.from_version..=entry.through_version).contains(&version))
                .ok_or_else(invalid_index)?;
            match &entry.target {
                KnowledgeMapHistoryIndexTarget::Node {
                    height,
                    r#ref,
                    digest,
                } => {
                    current = KnowledgeMapHistoryIndexRef {
                        from_version: entry.from_version,
                        through_version: entry.through_version,
                        height: *height,
                        r#ref: r#ref.clone(),
                        digest: digest.clone(),
                    };
                }
                KnowledgeMapHistoryIndexTarget::Archive { r#ref, digest } => {
                    reads += 1;
                    let archive_ref = KnowledgeMapArchiveRef {
                        r#ref: r#ref.clone(),
                        digest: digest.clone(),
                    };
                    let archive = self
                        .load_history_archive_in(contract_dir, &archive_ref, entry.through_version)
                        .await?;
                    if archive.from_version != entry.from_version {
                        return Err(invalid_index());
                    }
                    return Ok((archive, archive_ref, reads));
                }
            }
        }
    }

    async fn load_history_index_node(
        &self,
        index: &KnowledgeMapHistoryIndexRef,
    ) -> Result<KnowledgeMapHistoryIndexNode, KnowledgeMapServiceError> {
        let contract_dir = self.read_contract_dir_name().await?;
        self.load_history_index_node_in(contract_dir, index).await
    }

    async fn load_history_index_node_in(
        &self,
        contract_dir: &str,
        index: &KnowledgeMapHistoryIndexRef,
    ) -> Result<KnowledgeMapHistoryIndexNode, KnowledgeMapServiceError> {
        validate_history_index_ref_shape(index, index.through_version)?;
        let content = read_verified_ref_in(
            &self.repository_root,
            contract_dir,
            &index.r#ref,
            &index.digest,
        )
        .await?;
        let node = serde_norway::from_str::<KnowledgeMapHistoryIndexNode>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if !matches!(
            node.schema_version,
            LEGACY_ARTIFACT_SCHEMA_VERSION | ARTIFACT_SCHEMA_VERSION
        ) || node.height != index.height
            || node.from_version != index.from_version
            || node.through_version != index.through_version
            || node.entries.first().map(|entry| entry.from_version) != Some(node.from_version)
            || node.entries.last().map(|entry| entry.through_version) != Some(node.through_version)
        {
            return Err(invalid_index());
        }
        validate_index_entries(node.height, &node.entries)?;
        Ok(node)
    }

    async fn load_history_archive(
        &self,
        archive_ref: &KnowledgeMapArchiveRef,
        expected_through: u64,
    ) -> Result<KnowledgeMapHistoryArchive, KnowledgeMapServiceError> {
        let contract_dir = self.read_contract_dir_name().await?;
        self.load_history_archive_in(contract_dir, archive_ref, expected_through)
            .await
    }

    async fn load_history_archive_in(
        &self,
        contract_dir: &str,
        archive_ref: &KnowledgeMapArchiveRef,
        expected_through: u64,
    ) -> Result<KnowledgeMapHistoryArchive, KnowledgeMapServiceError> {
        if !archive_ref
            .r#ref
            .starts_with(&format!("{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/"))
        {
            return Err(KnowledgeMapServiceError::Integrity(
                "history archive chain has an unsafe ref".to_owned(),
            ));
        }
        let content = read_verified_ref_in(
            &self.repository_root,
            contract_dir,
            &archive_ref.r#ref,
            &archive_ref.digest,
        )
        .await?;
        let archive = serde_norway::from_str::<KnowledgeMapHistoryArchive>(&content)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        let expected_ref = format!(
            "{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/{:020}-{:020}-{}.yaml",
            archive.from_version, archive.through_version, archive_ref.digest
        );
        if !matches!(
            archive.schema_version,
            LEGACY_ARTIFACT_SCHEMA_VERSION | ARTIFACT_SCHEMA_VERSION
        ) || archive_ref.r#ref != expected_ref
            || archive.through_version != expected_through
            || archive.entries.is_empty()
            || archive.entries.len() > RECENT_HISTORY_LIMIT
            || archive.entries.first().map(|entry| entry.version) != Some(archive.from_version)
            || archive.entries.last().map(|entry| entry.version) != Some(archive.through_version)
        {
            return Err(KnowledgeMapServiceError::Integrity(format!(
                "history archive '{}' does not match its checkpoint",
                archive_ref.r#ref
            )));
        }
        let mut expected = archive.from_version;
        for entry in &archive.entries {
            entry
                .validate()
                .map_err(|error| KnowledgeMapServiceError::Integrity(error.to_string()))?;
            if entry.version != expected {
                return Err(KnowledgeMapServiceError::Integrity(format!(
                    "history archive '{}' is not contiguous",
                    archive_ref.r#ref
                )));
            }
            expected = expected.checked_add(1).ok_or_else(|| {
                KnowledgeMapServiceError::Integrity("history version overflow".to_owned())
            })?;
        }
        Ok(archive)
    }
}

fn archive_index_entry(
    archive_ref: KnowledgeMapArchiveRef,
    archive: &KnowledgeMapHistoryArchive,
) -> KnowledgeMapHistoryIndexEntry {
    KnowledgeMapHistoryIndexEntry {
        from_version: archive.from_version,
        through_version: archive.through_version,
        target: KnowledgeMapHistoryIndexTarget::Archive {
            r#ref: archive_ref.r#ref,
            digest: archive_ref.digest,
        },
    }
}

fn node_index_entry(index: KnowledgeMapHistoryIndexRef) -> KnowledgeMapHistoryIndexEntry {
    KnowledgeMapHistoryIndexEntry {
        from_version: index.from_version,
        through_version: index.through_version,
        target: KnowledgeMapHistoryIndexTarget::Node {
            height: index.height,
            r#ref: index.r#ref,
            digest: index.digest,
        },
    }
}

fn index_node_ref(entry: &KnowledgeMapHistoryIndexEntry) -> Option<KnowledgeMapHistoryIndexRef> {
    let KnowledgeMapHistoryIndexTarget::Node {
        height,
        r#ref,
        digest,
    } = &entry.target
    else {
        return None;
    };
    Some(KnowledgeMapHistoryIndexRef {
        from_version: entry.from_version,
        through_version: entry.through_version,
        height: *height,
        r#ref: r#ref.clone(),
        digest: digest.clone(),
    })
}

fn validate_index_entries(
    height: u8,
    entries: &[KnowledgeMapHistoryIndexEntry],
) -> Result<(), KnowledgeMapServiceError> {
    if height > HISTORY_INDEX_MAX_HEIGHT
        || entries.is_empty()
        || entries.len() > HISTORY_INDEX_FANOUT
    {
        return Err(invalid_index());
    }
    let mut expected = entries[0].from_version;
    for entry in entries {
        if entry.from_version == 0
            || entry.from_version != expected
            || entry.from_version > entry.through_version
        {
            return Err(noncontiguous_index());
        }
        match &entry.target {
            KnowledgeMapHistoryIndexTarget::Archive { r#ref, digest }
                if height == 0
                    && r#ref.starts_with(&format!("{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/"))
                    && digest.len() == 64
                    && digest.bytes().all(lower_hex_byte) => {}
            KnowledgeMapHistoryIndexTarget::Node {
                height: child_height,
                r#ref,
                digest,
            } if height > 0
                && child_height.checked_add(1) == Some(height)
                && r#ref.starts_with(&format!("{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/index-"))
                && digest.len() == 64
                && digest.bytes().all(lower_hex_byte) => {}
            _ => return Err(invalid_index()),
        }
        expected = entry
            .through_version
            .checked_add(1)
            .ok_or_else(invalid_index)?;
    }
    Ok(())
}

fn lower_hex_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn invalid_index() -> KnowledgeMapServiceError {
    KnowledgeMapServiceError::Integrity("history archive index is invalid".to_owned())
}

fn noncontiguous_index() -> KnowledgeMapServiceError {
    KnowledgeMapServiceError::Integrity(
        "history archive index ranges must be contiguous and non-overlapping".to_owned(),
    )
}

pub(super) fn balanced_index_split(entry_count: usize) -> Option<usize> {
    (entry_count > HISTORY_INDEX_FANOUT).then_some(entry_count / 2)
}
