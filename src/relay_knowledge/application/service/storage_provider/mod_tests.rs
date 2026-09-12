use crate::storage::{KnowledgeStoreFactoryFuture, StorageTopologySnapshot};

use super::*;

struct FailingFactory;

struct CatalogFreeFactory;

// The pre-existing public interface must still compile without a lifecycle hook.
impl KnowledgeStoreFactory for CatalogFreeFactory {
    fn open(&self) -> KnowledgeStoreFactoryFuture<'_, Arc<dyn KnowledgeStore>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "not opened by preflight".to_owned(),
            ))
        })
    }

    fn topology_snapshot(&self) -> KnowledgeStoreFactoryFuture<'_, StorageTopologySnapshot> {
        Box::pin(async { Ok(StorageTopologySnapshot::default()) })
    }
}

#[tokio::test]
async fn legacy_factory_without_lifecycle_hook_keeps_cold_preflight_compatible() {
    let provider = StorageProvider::configured(Arc::new(CatalogFreeFactory));
    provider.validate_lifecycle_storage().await.unwrap();
    assert!(provider.ready_store().is_none());
}

impl KnowledgeStoreFactory for FailingFactory {
    fn validate_lifecycle_storage(&self) -> KnowledgeStoreFactoryFuture<'_, ()> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "factory-lifecycle-failed".to_owned(),
            ))
        })
    }

    fn open(&self) -> KnowledgeStoreFactoryFuture<'_, Arc<dyn KnowledgeStore>> {
        Box::pin(async { Err(StorageError::InvalidInput("factory-open-failed".to_owned())) })
    }

    fn topology_snapshot(&self) -> KnowledgeStoreFactoryFuture<'_, StorageTopologySnapshot> {
        Box::pin(async { Ok(StorageTopologySnapshot::default()) })
    }
}

#[tokio::test]
async fn lazy_provider_preserves_factory_errors_without_partial_initialization() {
    let provider = StorageProvider::configured(Arc::new(FailingFactory));

    let error = match provider.get().await {
        Ok(_) => panic!("factory should fail"),
        Err(error) => error,
    };

    assert_eq!(
        error.to_string(),
        "invalid storage input: factory-open-failed"
    );
    assert!(provider.ready_store().is_none());
    assert!(
        provider
            .validate_lifecycle_storage()
            .await
            .unwrap_err()
            .to_string()
            .contains("factory-lifecycle-failed")
    );
}
