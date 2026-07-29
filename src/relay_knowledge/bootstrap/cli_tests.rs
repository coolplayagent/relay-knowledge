use super::*;

#[tokio::test]
async fn process_entry_delegates_to_existing_cli_behavior() {
    let bootstrap = run_process(["--version"], false)
        .await
        .expect("bootstrap CLI process should render version");
    let interface = crate::interfaces::cli::run_process(["--version"], false)
        .await
        .expect("interface CLI process should render version");

    assert_eq!(bootstrap, interface);
}
