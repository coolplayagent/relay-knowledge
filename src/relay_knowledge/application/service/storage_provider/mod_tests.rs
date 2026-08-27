use crate::storage::{KnowledgeStoreFactoryFuture, StorageTopologySnapshot};

use super::*;

struct FailingFactory;

impl KnowledgeStoreFactory for FailingFactory {
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
}
