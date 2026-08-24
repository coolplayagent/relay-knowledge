//! Bounded Knowledge Map history paging and archive-chain validation.

use crate::{
    api::RequestContext,
    domain::{KnowledgeMap, KnowledgeMapHistoryEntry},
    project::{KNOWLEDGE_MAP_HISTORY_DIR_NAME, KNOWLEDGE_MAP_RELATIVE_PATH},
};

use super::{
    KnowledgeMapHistoryResponse, KnowledgeMapService, KnowledgeMapServiceError,
    artifact::{
        ARTIFACT_SCHEMA_VERSION, KnowledgeMapArchiveRef, KnowledgeMapHistoryArchive,
        KnowledgeMapHistoryManifest, KnowledgeMapManifest, KnowledgeMapSchemaProbe,
        RECENT_HISTORY_LIMIT, parse_manifest, read_verified_ref, validate_recent_history,
    },
    contracts::metadata,
};

pub(super) const MAX_HISTORY_PAGE_SIZE: usize = 256;

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
            path: KNOWLEDGE_MAP_RELATIVE_PATH.to_owned(),
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
                let map = serde_norway::from_str::<KnowledgeMap>(&content)
                    .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
                map.validate()?;
                let entries = map
                    .history
                    .into_iter()
                    .filter(|entry| entry.version >= from_version)
                    .take(limit)
                    .collect();
                Ok((map.map_version, entries))
            }
            ARTIFACT_SCHEMA_VERSION => {
                let manifest = parse_manifest(&content)?;
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
            let mut archive_ref = manifest.history.archive.clone().ok_or_else(|| {
                KnowledgeMapServiceError::Integrity(
                    "history archive is missing for a non-zero checkpoint".to_owned(),
                )
            })?;
            let mut expected_through = manifest.history.archived_through;
            let mut visited = std::collections::HashSet::new();
            loop {
                if !visited.insert(archive_ref.r#ref.clone()) {
                    return Err(KnowledgeMapServiceError::Integrity(
                        "history archive chain has a cycle".to_owned(),
                    ));
                }
                let archive = self
                    .load_history_archive(&archive_ref, expected_through)
                    .await?;
                entries.extend(
                    archive
                        .entries
                        .iter()
                        .filter(|entry| (from_version..=through_version).contains(&entry.version))
                        .cloned(),
                );
                if archive.from_version <= from_version {
                    break;
                }
                expected_through = archive.from_version - 1;
                archive_ref = archive.previous.ok_or_else(|| {
                    KnowledgeMapServiceError::Integrity(
                        "history archive chain ends before the requested version".to_owned(),
                    )
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

    pub(super) async fn load_archived_history(
        &self,
        history: &KnowledgeMapHistoryManifest,
    ) -> Result<Vec<KnowledgeMapHistoryEntry>, KnowledgeMapServiceError> {
        let Some(mut archive_ref) = history.archive.clone() else {
            if history.archived_through != 0 {
                return Err(KnowledgeMapServiceError::Integrity(
                    "history archive is missing for a non-zero checkpoint".to_owned(),
                ));
            }
            return Ok(Vec::new());
        };
        let mut expected_through = history.archived_through;
        let mut chunks = Vec::new();
        let mut visited = std::collections::HashSet::new();
        loop {
            if !archive_ref
                .r#ref
                .starts_with(&format!("{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/"))
                || !visited.insert(archive_ref.r#ref.clone())
            {
                return Err(KnowledgeMapServiceError::Integrity(
                    "history archive chain has an unsafe ref or cycle".to_owned(),
                ));
            }
            let archive = self
                .load_history_archive(&archive_ref, expected_through)
                .await?;
            expected_through = archive.from_version.saturating_sub(1);
            chunks.push(archive.entries);
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
        chunks.reverse();
        Ok(chunks.into_iter().flatten().collect())
    }

    async fn load_history_archive(
        &self,
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
        let content = read_verified_ref(
            &self.repository_root,
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
        if archive.schema_version != ARTIFACT_SCHEMA_VERSION
            || archive_ref.r#ref != expected_ref
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
