//! Recoverable v1/v2 to v3 Knowledge Map migration and rollback.

use std::path::{Path, PathBuf};

use tokio::fs;

use crate::{
    api::RequestContext,
    domain::RepositoryMapType,
    project::{
        AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_HISTORY_DIR_NAME, KNOWLEDGE_MAP_RELATIVE_PATH,
        KNOWLEDGE_MAP_TOPICS_DIR_NAME, LEGACY_AGENT_CONTRACT_DIR_NAME,
        LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH, LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH,
    },
};

use super::{
    KnowledgeMapMutationResponse, KnowledgeMapService, KnowledgeMapServiceError, WRITE_LOCK_TIMEOUT,
};

impl KnowledgeMapService {
    pub async fn migrate_to_v3(
        &self,
        context: &RequestContext,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        self.require_knowledge_map("map migrate --to-v3")?;
        self.init(context).await
    }

    pub async fn rollback_v3(
        &self,
        context: &RequestContext,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        self.require_knowledge_map("map migrate --rollback")?;
        let _legacy_lock = self.acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT).await?;
        let _lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        let legacy_backup = self.legacy_backup_path();
        if !fs::try_exists(&legacy_backup).await? {
            return Err(KnowledgeMapServiceError::InvalidRequest(
                "v2 rollback backup is missing".to_owned(),
            ));
        }
        let current = self.map_path();
        if fs::try_exists(&current).await? {
            let retained = current.with_extension("yaml.v3.previous");
            if fs::try_exists(&retained).await? {
                fs::remove_file(&retained).await?;
            }
            fs::rename(&current, retained).await?;
        }
        fs::copy(&legacy_backup, self.legacy_map_path()).await?;
        let map = self.load_for_mutation().await?;
        Ok(self.mutation_response(
            context,
            map.map.map_version,
            "restored Knowledge Map v2 root; retained v3 data for forward recovery".to_owned(),
        ))
    }

    pub(super) async fn prepare_legacy_migration(&self) -> Result<(), KnowledgeMapServiceError> {
        if self.map_type != RepositoryMapType::Knowledge || fs::try_exists(self.map_path()).await? {
            return Ok(());
        }
        let legacy = self.legacy_map_path();
        if !fs::try_exists(&legacy).await? {
            return Ok(());
        }
        let backup = self.legacy_backup_path();
        if !fs::try_exists(&backup).await? {
            fs::copy(&legacy, &backup).await?;
        }
        copy_contract_tree(
            &self
                .repository_root
                .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME),
            &self
                .repository_root
                .join(AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME),
        )
        .await?;
        copy_contract_tree(
            &self
                .repository_root
                .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
            &self
                .repository_root
                .join(AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
        )
        .await?;
        let legacy_glossary = self
            .repository_root
            .join(LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH);
        let glossary = self.business_glossary_path();
        if fs::try_exists(&legacy_glossary).await? && !fs::try_exists(&glossary).await? {
            if let Some(parent) = glossary.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(legacy_glossary, glossary).await?;
        }
        fs::copy(legacy, self.map_path()).await?;
        Ok(())
    }

    pub(super) async fn publish_legacy_redirect(&self) -> Result<(), KnowledgeMapServiceError> {
        if self.map_type != RepositoryMapType::Knowledge {
            return Ok(());
        }
        let yaml = format!(
            "schema_version: 3\nartifact_kind: redirect\nmap_type: knowledge\ntarget: {KNOWLEDGE_MAP_RELATIVE_PATH}\n"
        );
        let legacy = self.legacy_map_path();
        let prepared = legacy.with_extension("yaml.redirect.prepared");
        let previous = legacy.with_extension("yaml.redirect.previous");
        fs::write(&prepared, yaml).await?;
        if fs::try_exists(&previous).await? {
            fs::remove_file(&previous).await?;
        }
        fs::rename(&legacy, &previous).await?;
        if let Err(error) = fs::rename(&prepared, &legacy).await {
            let _ = fs::rename(&previous, &legacy).await;
            let _ = fs::remove_file(prepared).await;
            return Err(error.into());
        }
        fs::remove_file(previous).await?;
        Ok(())
    }

    pub(super) fn legacy_map_path(&self) -> PathBuf {
        self.repository_root
            .join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH)
    }

    pub(super) fn legacy_backup_path(&self) -> PathBuf {
        self.repository_root
            .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
            .join("knowledge-map.v2.yaml")
    }
}

async fn copy_contract_tree(source: &Path, target: &Path) -> Result<(), KnowledgeMapServiceError> {
    if !fs::try_exists(source).await? {
        return Ok(());
    }
    fs::create_dir_all(target).await?;
    let mut entries = fs::read_dir(source).await?;
    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        if metadata.is_file() {
            let destination = target.join(entry.file_name());
            if !fs::try_exists(&destination).await? {
                fs::copy(entry.path(), destination).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
