use super::super::super::{CliAction, remote};
use super::super::test_support::{FixtureRepo, context, json_value, service_with_memory_store};
use super::*;
use crate::env::NetworkEnvOverrides;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn scope_preview_and_dry_run_return_bounded_details_locally_and_remotely() {
    let repo = FixtureRepo::create("cli-preview-details");
    for index in 0..51 {
        repo.write(&format!("src/broken_{index:02}.c"), "int broken = ;\n");
        repo.write(&format!("data/excluded_{index:02}.jsonl"), "{}\n");
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "preview details"]);
    let service = service_with_memory_store().await;
    run_repo(
        &service,
        RepoCommand::Register {
            root_path: repo.path.display().to_string(),
            alias: "fixture".to_owned(),
            path_filters: vec![".".to_owned()],
            language_filters: Vec::new(),
        },
        context("register"),
        OutputFormat::Json,
    )
    .await
    .unwrap();
    let commands = [
        RepoCommand::ScopePreview {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
        },
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: true,
            reuse_historical: false,
        },
    ];
    let mut expected = None;
    for command in commands {
        let output = run_repo(
            &service,
            command.clone(),
            context("preview"),
            OutputFormat::Json,
        )
        .await
        .unwrap();
        let value = json_value(&output);
        let preview = &value["preview"];
        assert!(preview.get("expected_degraded_file_count").is_none());
        for key in ["expected_degraded_files", "excluded_paths"] {
            let files = preview[key].as_array().unwrap();
            assert_eq!(files.len(), 50);
            assert_eq!(preview[format!("{key}_truncated")], true);
            assert!(
                files
                    .iter()
                    .all(|file| !file["reason"].as_str().unwrap().is_empty())
            );
        }
        if let Some(expected) = &expected {
            assert_eq!(preview, expected);
        }
        expected = Some(preview.clone());

        // A loopback HTTP fixture checks typed remote deserialization without runtime state.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
                assert!(request.len() <= 16_384);
                if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    break;
                }
            }
            assert!(
                String::from_utf8_lossy(&request)
                    .starts_with("POST /api/v1/code/repositories/fixture/scope/preview HTTP/1.1")
            );
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                output.len()
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(output.as_bytes()).await.unwrap();
        });
        let remote = remote::run_remote(
            &NetworkEnvOverrides::default(),
            &format!("http://{address}"),
            &CliAction::Repo(command),
            context("remote-preview"),
            OutputFormat::Json,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(json_value(&remote)["preview"], *preview);
        server.await.unwrap();
    }
    let repositories = run_repo(
        &service,
        RepoCommand::List,
        context("list"),
        OutputFormat::Json,
    )
    .await
    .unwrap();
    assert!(
        json_value(&repositories)["repositories"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
