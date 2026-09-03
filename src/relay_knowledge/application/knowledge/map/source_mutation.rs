//! Validated and recoverable Knowledge Map source mutations.

use tokio::fs;

use crate::{
    api::RequestContext,
    domain::{KnowledgeMapChange, KnowledgeMapSource},
};

use super::{
    KnowledgeMapMutationResponse, KnowledgeMapService, KnowledgeMapServiceError,
    KnowledgeMapSourceAddRequest, MutableKnowledgeMap, WRITE_LOCK_TIMEOUT,
    lock::KnowledgeMapWriteLock, now_stamp,
};

pub(super) struct KnowledgeMapMutationLocks {
    _legacy_lock: Option<KnowledgeMapWriteLock>,
    _current_lock: KnowledgeMapWriteLock,
    legacy_recovery_state: bool,
}

impl KnowledgeMapService {
    pub async fn add_source(
        &self,
        context: &RequestContext,
        request: KnowledgeMapSourceAddRequest,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        self.require_knowledge_map("map source add")?;
        let id = request.id.clone();
        let topic = request.topic.clone();
        let source = KnowledgeMapSource::new(
            request.id,
            request.topic,
            request.kind,
            request.uri,
            request.source_scope,
            request.description,
        )?;
        let initial_legacy_recovery_state = self.legacy_recovery_state_exists().await?;
        let contract_exists = fs::try_exists(self.map_path()).await?
            || fs::try_exists(self.backup_path()).await?
            || initial_legacy_recovery_state;
        if !contract_exists {
            let mut preflight = MutableKnowledgeMap::initial(self.map_type, now_stamp());
            preflight
                .map
                .add_source_snapshot(source.clone(), preflight.archived_through)?;
        }

        let mutation_locks = self.acquire_legacy_aware_mutation_locks().await?;
        let legacy_recovery_state = mutation_locks.legacy_recovery_state;
        let _rollback_committed = self.recover_legacy_rollback_transition().await?;
        self.recover_manifest_backup().await?;
        self.recover_legacy_redirect_transition().await?;
        if legacy_recovery_state && !fs::try_exists(self.map_path()).await? {
            let mut preflight = self.load_for_mutation().await?;
            preflight
                .map
                .ensure_reserved_repository_routes_snapshot(preflight.archived_through)?;
            preflight
                .map
                .add_source_snapshot(source.clone(), preflight.archived_through)?;
            preflight.map.validate_reserved_repository_routes()?;
        }
        self.prepare_legacy_migration().await?;
        let mut snapshot = self.load_or_initial().await?;
        snapshot
            .map
            .ensure_reserved_repository_routes_snapshot(snapshot.archived_through)?;
        snapshot
            .map
            .add_source_snapshot(source, snapshot.archived_through)?;
        snapshot.map.validate_reserved_repository_routes()?;
        snapshot.map.record_change(
            "source.add",
            format!("Added source '{id}' to topic '{topic}'."),
            now_stamp(),
        );
        self.ensure_baseline_files().await?;
        self.ensure_default_business_glossary().await?;
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("added source {id}"),
        ))
    }

    pub async fn update_source(
        &self,
        context: &RequestContext,
        change: KnowledgeMapChange,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        self.require_knowledge_map("map source update")?;
        let mutation_locks = self.acquire_legacy_aware_mutation_locks().await?;
        let legacy_recovery_state = mutation_locks.legacy_recovery_state;
        self.recover_legacy_rollback_transition().await?;
        self.recover_manifest_backup().await?;
        self.recover_legacy_redirect_transition().await?;
        if legacy_recovery_state && !fs::try_exists(self.map_path()).await? {
            let mut preflight = self.load_for_mutation().await?;
            preflight
                .map
                .ensure_reserved_repository_routes_snapshot(preflight.archived_through)?;
            preflight
                .map
                .update_source_snapshot(change.clone(), preflight.archived_through)?;
            preflight.map.validate_reserved_repository_routes()?;
        }
        self.prepare_legacy_migration().await?;
        let mut snapshot = self.load_for_mutation().await?;
        snapshot
            .map
            .ensure_reserved_repository_routes_snapshot(snapshot.archived_through)?;
        let id = change.id.clone();
        snapshot
            .map
            .update_source_snapshot(change, snapshot.archived_through)?;
        snapshot.map.validate_reserved_repository_routes()?;
        snapshot.map.record_change(
            "source.update",
            format!("Updated source '{id}'."),
            now_stamp(),
        );
        self.ensure_baseline_files().await?;
        self.ensure_default_business_glossary().await?;
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("updated source {id}"),
        ))
    }

    pub async fn remove_source(
        &self,
        context: &RequestContext,
        id: String,
    ) -> Result<KnowledgeMapMutationResponse, KnowledgeMapServiceError> {
        self.require_knowledge_map("map source remove")?;
        let mutation_locks = self.acquire_legacy_aware_mutation_locks().await?;
        let legacy_recovery_state = mutation_locks.legacy_recovery_state;
        self.recover_legacy_rollback_transition().await?;
        self.recover_manifest_backup().await?;
        self.recover_legacy_redirect_transition().await?;
        if legacy_recovery_state && !fs::try_exists(self.map_path()).await? {
            let mut preflight = self.load_for_mutation().await?;
            preflight
                .map
                .ensure_reserved_repository_routes_snapshot(preflight.archived_through)?;
            preflight
                .map
                .remove_source_snapshot(&id, preflight.archived_through)?;
            preflight.map.validate_reserved_repository_routes()?;
        }
        self.prepare_legacy_migration().await?;
        let mut snapshot = self.load_for_mutation().await?;
        snapshot
            .map
            .ensure_reserved_repository_routes_snapshot(snapshot.archived_through)?;
        snapshot
            .map
            .remove_source_snapshot(&id, snapshot.archived_through)?;
        snapshot.map.validate_reserved_repository_routes()?;
        snapshot.map.record_change(
            "source.remove",
            format!("Removed source '{id}'."),
            now_stamp(),
        );
        self.ensure_baseline_files().await?;
        self.ensure_default_business_glossary().await?;
        self.write_map(&mut snapshot).await?;
        Ok(self.mutation_response(
            context,
            snapshot.map.map_version,
            format!("removed source {id}"),
        ))
    }

    pub(super) async fn acquire_legacy_aware_mutation_locks(
        &self,
    ) -> Result<KnowledgeMapMutationLocks, KnowledgeMapServiceError> {
        if self.legacy_recovery_state_exists().await? {
            let legacy_lock = self.acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT).await?;
            let current_lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
            return Ok(KnowledgeMapMutationLocks {
                _legacy_lock: Some(legacy_lock),
                _current_lock: current_lock,
                legacy_recovery_state: true,
            });
        }

        let current_lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        if !self.legacy_recovery_state_exists().await? {
            return Ok(KnowledgeMapMutationLocks {
                _legacy_lock: None,
                _current_lock: current_lock,
                legacy_recovery_state: false,
            });
        }
        drop(current_lock);

        let legacy_lock = self.acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT).await?;
        let current_lock = self.acquire_write_lock(WRITE_LOCK_TIMEOUT).await?;
        Ok(KnowledgeMapMutationLocks {
            _legacy_lock: Some(legacy_lock),
            _current_lock: current_lock,
            legacy_recovery_state: true,
        })
    }
}
