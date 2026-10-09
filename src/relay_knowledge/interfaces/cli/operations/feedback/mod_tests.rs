use super::*;

#[test]
fn feedback_parser_rejects_implicit_or_trailing_authority() {
    for args in [
        vec!["report"],
        vec!["submit", "id", "--force"],
        vec!["configure", "--repository", "other/repo"],
    ] {
        assert!(parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err());
    }
    assert!(matches!(
        parse(&["status".into()]).unwrap(),
        CliAction::Feedback(FeedbackCommand::Status { id: None })
    ));
}

#[tokio::test]
async fn cli_persists_and_previews_a_report_without_opening_graph_storage() {
    let _guard = crate::storage::feedback::TEST_LOCK.lock().await;
    use crate::{
        api::InterfaceKind,
        env::{EnvironmentConfig, PlatformKind},
        interfaces::cli::CliCommand,
    };
    let root = std::env::temp_dir().join(format!("feedback-cli-{}", std::process::id()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let input = root.join("input.json");
    tokio::fs::write(&input, serde_json::to_vec(&serde_json::json!({
        "schema_version":1,"kind":"workflow-friction",
        "intent":"Maintain a research topic", "expected":"Apply ten sources in one logical update",
        "actual":"Individual source updates require repeated calls", "impact":"Manual coordination adds steps",
        "evidence":[{"label":"private source","content":"private knowledge must stay local"}]
    })).unwrap()).await.unwrap();
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap())],
    )
    .unwrap();
    let service = RelayKnowledgeService::from_environment(&environment)
        .await
        .unwrap();
    let context =
        RequestContext::with_ids(InterfaceKind::Cli, "original-request", "original-trace");
    let command = CliCommand::parse([
        "feedback",
        "report",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "json",
    ])
    .unwrap();
    let CliAction::Feedback(action) = command.action else {
        panic!("expected feedback")
    };
    let output = run(&service, action, context.clone(), OutputFormat::Json)
        .await
        .unwrap();
    let response: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(response["feedback"]["publication"]["state"], "draft");
    assert_eq!(response["feedback"]["trace_id"], "original-trace");
    assert!(!output.contains("private knowledge must stay local"));
    assert!(!service.storage_is_ready());
    let id = response["feedback"]["id"].as_str().unwrap().to_owned();
    let preview = run(
        &service,
        FeedbackCommand::Preview { id: id.clone() },
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .unwrap();
    assert!(preview.contains("Apply ten sources"));
    let duplicate = run(
        &service,
        FeedbackCommand::Report {
            input: input.to_str().unwrap().into(),
        },
        context.clone(),
        OutputFormat::Json,
    )
    .await
    .unwrap();
    let duplicate: serde_json::Value = serde_json::from_str(&duplicate).unwrap();
    assert_eq!(duplicate["feedback"]["id"], id);
    assert_eq!(duplicate["feedback"]["occurrences"], 2);
    let submitted = run(
        &service,
        FeedbackCommand::Submit { id },
        context,
        OutputFormat::Json,
    )
    .await
    .unwrap();
    assert!(submitted.contains("local-only policy"));
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn input_size_and_schema_fail_before_report_persistence() {
    let root = std::env::temp_dir().join(format!("feedback-input-{}", std::process::id()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let input = root.join("oversized.json");
    tokio::fs::write(&input, vec![b' '; 65_537]).await.unwrap();
    assert!(
        read_input::<FeedbackReport>(input.to_str().unwrap(), OutputFormat::Json)
            .await
            .is_err()
    );
    tokio::fs::write(&input, br#"{"schema_version":2,"execute":"untrusted"}"#)
        .await
        .unwrap();
    assert!(
        read_input::<FeedbackReport>(input.to_str().unwrap(), OutputFormat::Json)
            .await
            .is_err()
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn special_inputs_are_rejected_without_opening_a_blocking_stream() {
    let root = std::env::temp_dir().join(format!("feedback-fifo-{}", std::process::id()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let fifo = root.join("input.fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_input::<FeedbackReport>(fifo.to_str().unwrap(), OutputFormat::Json),
    )
    .await;
    assert!(result.unwrap().is_err());
    assert!(
        read_input::<FeedbackReport>(root.to_str().unwrap(), OutputFormat::Json)
            .await
            .is_err()
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}
