use std::sync::{Arc, OnceLock};

use crate::storage::{
    KnowledgeStore, KnowledgeStoreFactory, StorageError, StorageTopologySnapshot,
};

#[derive(Clone)]
pub(in crate::application) struct StorageProvider {
    factory: Option<Arc<dyn KnowledgeStoreFactory>>,
    ready: Arc<OnceLock<Arc<dyn KnowledgeStore>>>,
    init_lock: Arc<tokio::sync::Mutex<()>>,
}

impl StorageProvider {
    pub(super) fn configured(factory: Arc<dyn KnowledgeStoreFactory>) -> Self {
        Self {
            factory: Some(factory),
            ready: Arc::new(OnceLock::new()),
            init_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(super) fn ready(store: Arc<dyn KnowledgeStore>) -> Self {
        let ready = OnceLock::new();
        let _ = ready.set(store);

        Self {
            factory: None,
            ready: Arc::new(ready),
            init_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(in crate::application) async fn get(
        &self,
    ) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
        if let Some(store) = self.ready.get() {
            return Ok(Arc::clone(store));
        }
        let _guard = self.init_lock.lock().await;
        if let Some(store) = self.ready.get() {
            return Ok(Arc::clone(store));
        }

        let Some(factory) = &self.factory else {
            return Err(StorageError::InvalidInput(
                "storage provider was not initialized".to_owned(),
            ));
        };
        let store = factory.open().await?;
        let _ = self.ready.set(Arc::clone(&store));
        Ok(store)
    }

    pub(in crate::application) fn ready_store(&self) -> Option<Arc<dyn KnowledgeStore>> {
        self.ready.get().map(Arc::clone)
    }

    /// Preserve the configured catalog guard without initializing cold graph
    /// storage. Injected ready stores have no external catalog to inspect.
    pub(in crate::application) async fn validate_lifecycle_storage(
        &self,
    ) -> Result<(), StorageError> {
        match &self.factory {
            Some(factory) => factory.validate_lifecycle_storage().await,
            None => Ok(()),
        }
    }

    pub(in crate::application) async fn topology_snapshot(
        &self,
    ) -> Result<StorageTopologySnapshot, StorageError> {
        let Some(factory) = &self.factory else {
            return Ok(StorageTopologySnapshot::default());
        };
        factory.topology_snapshot().await
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
