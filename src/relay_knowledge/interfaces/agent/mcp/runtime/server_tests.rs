use super::super::super::tests::server_with_env;

#[tokio::test]
async fn cloned_server_state_shares_cancellation_registry() {
    let server = server_with_env([]).await;
    let clone = server.clone();
    let (cancellation, _registration) = server.cancellations.register("request".to_owned());

    assert!(clone.cancellations.cancel("request"));
    assert!(*cancellation.borrow());
}
