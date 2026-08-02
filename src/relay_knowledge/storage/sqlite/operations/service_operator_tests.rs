use crate::{
    domain::ServiceOperatorState,
    storage::{IndexStore, ServiceOperatorUpdate, SqliteGraphStore},
};

#[tokio::test]
async fn sqlite_operator_round_trip() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let operator = store
        .update_service_operator(ServiceOperatorUpdate {
            state: ServiceOperatorState::Enabled,
            silent_updates_enabled: true,
            allowed_scopes: vec!["docs".to_owned(), "src".to_owned()],
            last_error: Some("previous failure".to_owned()),
            now_ms: 40,
        })
        .await
        .expect("operator should update");

    assert_eq!(operator.state, ServiceOperatorState::Enabled);
    assert!(operator.silent_updates_enabled);
    assert_eq!(operator.allowed_scopes, ["docs", "src"]);
    assert_eq!(operator.last_error.as_deref(), Some("previous failure"));
}
