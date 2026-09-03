//! Typed repository-directory governance shared by CodeSpec and Knowledge maps.

use tokio::fs;

use crate::{
    api::RequestContext,
    domain::{
        RepositoryMapDirectory, RepositoryMapDirectoryChange, RepositoryMapType,
        validate_directory_collection,
    },
    project::{CODESPEC_MAP_FILE_NAME, KNOWLEDGE_MAP_FILE_NAME},
};

use super::{
    KnowledgeMapMutationResponse, KnowledgeMapService, KnowledgeMapServiceError,
    ensure_owned_directory, now_stamp,
};

impl KnowledgeMapService {
    pub async fn add_directory(
        &self,
        context: &RequestContext,
        directory: RepositoryMapDirectory,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let _mutation_locks = self.acquire_legacy_aware_mutation_locks().await?;
        self.recover_legacy_rollback_transition().await?;
        self.recover_manifest_backup().await?;
        self.recover_legacy_redirect_transition().await?;
        self.prepare_legacy_migration().await?;
        let mut snapshot = self.load_for_mutation().await?;
        directory.validate(self.map_type)?;
        if snapshot
            .directories
            .iter()
            .any(|entry| entry.directory.eq_ignore_ascii_case(&directory.directory))
        {
            return Err(KnowledgeMapServiceError::InvalidRequest(format!(
                "directory '{}' already exists",
                directory.directory
            )));
        }
        let name = directory.directory.clone();
        snapshot.directories.push(directory);
        snapshot
            .directories
            .sort_by(|left, right| left.directory.cmp(&right.directory));
        validate_directory_collection(self.map_type, &snapshot.directories, true)?;
        snapshot.map.record_change(
            "directory.add",
            format!("Added governed directory '{name}'."),
            now_stamp(),
        );
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("added directory {name}"),
        ))
    }

    pub async fn update_directory(
        &self,
        context: &RequestContext,
        change: RepositoryMapDirectoryChange,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        let _mutation_locks = self.acquire_legacy_aware_mutation_locks().await?;
        self.recover_legacy_rollback_transition().await?;
        self.recover_manifest_backup().await?;
        self.recover_legacy_redirect_transition().await?;
        self.prepare_legacy_migration().await?;
        let mut snapshot = self.load_for_mutation().await?;
        let name = change.directory.clone();
        let entry = snapshot
            .directories
            .iter_mut()
            .find(|entry| entry.directory == name)
            .ok_or_else(|| {
                KnowledgeMapServiceError::InvalidRequest(format!(
                    "directory '{name}' does not exist"
                ))
            })?;
        if let Some(value) = change.purpose {
            entry.purpose = value;
        }
        if let Some(value) = change.content_scope {
            entry.content_scope = value;
        }
        if let Some(value) = change.key_files {
            entry.key_files = value;
        }
        if let Some(value) = change.load_hint {
            entry.load_hint = value;
        }
        if let Some(value) = change.relations {
            entry.relations = value;
        }
        if let Some(value) = change.update_rule {
            entry.update_rule = value;
        }
        validate_directory_collection(self.map_type, &snapshot.directories, true)?;
        snapshot.map.record_change(
            "directory.update",
            format!("Updated governed directory '{name}'."),
            now_stamp(),
        );
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("updated directory {name}"),
        ))
    }

    pub async fn remove_directory(
        &self,
        context: &RequestContext,
        directory: String,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        if self
            .map_type
            .required_directories()
            .contains(&directory.as_str())
        {
            return Err(KnowledgeMapServiceError::InvalidRequest(format!(
                "required directory '{directory}' cannot be removed"
            )));
        }
        let _mutation_locks = self.acquire_legacy_aware_mutation_locks().await?;
        self.recover_legacy_rollback_transition().await?;
        self.recover_manifest_backup().await?;
        self.recover_legacy_redirect_transition().await?;
        self.prepare_legacy_migration().await?;
        let mut snapshot = self.load_for_mutation().await?;
        let before = snapshot.directories.len();
        snapshot
            .directories
            .retain(|entry| entry.directory != directory);
        if before == snapshot.directories.len() {
            return Err(KnowledgeMapServiceError::InvalidRequest(format!(
                "directory '{directory}' does not exist"
            )));
        }
        validate_directory_collection(self.map_type, &snapshot.directories, true)?;
        snapshot.map.record_change(
            "directory.remove",
            format!("Removed governed directory '{directory}'."),
            now_stamp(),
        );
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("removed directory {directory}"),
        ))
    }

    pub(super) async fn ensure_baseline_files(&self) -> Result<(), KnowledgeMapServiceError> {
        let contract = self.repository_root.join(self.contract_dir_name());
        ensure_owned_directory(&self.repository_root, &contract).await?;
        for directory in self.map_type.required_directories() {
            let path = contract.join(directory);
            ensure_owned_directory(&self.repository_root, &path).await?;
            let readme = path.join("README.md");
            if !fs::try_exists(&readme).await? {
                fs::write(readme, baseline_readme(self.map_type, directory)).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn validate_directory_files(&self) -> Result<(), KnowledgeMapServiceError> {
        if self.uses_legacy_contract().await? {
            return Ok(());
        }
        let map = self.load_show_view().await?;
        validate_directory_collection(self.map_type, &map.directories, true)?;
        let repository = fs::canonicalize(&self.repository_root).await?;
        for entry in map.directories {
            let directory = self
                .repository_root
                .join(self.map_type.as_str())
                .join(&entry.directory);
            let metadata = fs::symlink_metadata(&directory).await?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(KnowledgeMapServiceError::UnsafePath(
                    directory.display().to_string(),
                ));
            }
            let canonical = fs::canonicalize(directory).await?;
            if !canonical.starts_with(&repository) {
                return Err(KnowledgeMapServiceError::UnsafePath(
                    canonical.display().to_string(),
                ));
            }
            for key_file in entry.key_files {
                let path = self.repository_root.join(&key_file);
                let metadata = fs::symlink_metadata(&path).await?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(KnowledgeMapServiceError::UnsafePath(key_file));
                }
                if !fs::canonicalize(path).await?.starts_with(&repository) {
                    return Err(KnowledgeMapServiceError::UnsafePath(key_file));
                }
            }
        }
        Ok(())
    }

    pub(super) async fn validate_cross_map_relations(
        &self,
    ) -> Result<(), KnowledgeMapServiceError> {
        let map = self.load_show_view().await?;
        let peer_type = match self.map_type {
            RepositoryMapType::Knowledge => RepositoryMapType::Codespec,
            RepositoryMapType::Codespec => RepositoryMapType::Knowledge,
        };
        let peer_targets = map
            .directories
            .iter()
            .flat_map(|entry| &entry.relations)
            .filter_map(|relation| {
                relation
                    .target
                    .strip_prefix(&format!("{}:", peer_type.as_str()))
            })
            .collect::<Vec<_>>();
        if peer_targets.is_empty() {
            return Ok(());
        }
        let peer = self.for_type(peer_type).load_show_view().await?;
        for target in peer_targets {
            if !peer
                .directories
                .iter()
                .any(|entry| entry.directory == target)
            {
                return Err(KnowledgeMapServiceError::Integrity(format!(
                    "cross-map directory target '{}:{target}' does not exist",
                    peer_type.as_str()
                )));
            }
        }
        Ok(())
    }
}

fn baseline_readme(map_type: RepositoryMapType, directory: &str) -> String {
    let title = directory
        .split('-')
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + characters.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ");
    let file_name = match map_type {
        RepositoryMapType::Knowledge => KNOWLEDGE_MAP_FILE_NAME,
        RepositoryMapType::Codespec => CODESPEC_MAP_FILE_NAME,
    };
    format!(
        "# {title}\n\nThis directory is governed by `{}/{file_name}`. Update its map entry through `relay-knowledge map directory` and keep reviewed source material within the declared content scope.\n",
        map_type.as_str()
    )
}

#[cfg(test)]
#[path = "governance_tests.rs"]
mod tests;
