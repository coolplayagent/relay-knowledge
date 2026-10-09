//! Deterministic loopback fixtures; endpoint injection exists only in test builds.

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
    task::JoinHandle,
};

use super::*;
use crate::env::NetworkEnvOverrides;

pub(super) enum MockResponse {
    Reply(String),
    Disconnect,
    Hang,
}

pub(super) struct MockServer {
    pub requests: mpsc::Receiver<String>,
    task: JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn mock_provider(
    responses: Vec<MockResponse>,
    timeout_ms: u64,
) -> (GithubFeedbackProvider, MockServer) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let address = listener.local_addr().expect("mock address");
    let (sender, requests) = mpsc::channel(16);
    let task = tokio::spawn(async move {
        for response in responses {
            let (mut stream, _) = listener.accept().await.expect("mock accept");
            let mut received = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).await.expect("mock read");
                if count == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..count]);
                assert!(received.len() < 128 * 1024, "request must be bounded");
                if let Some(header_end) = received.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&received[..header_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().expect("valid length"))
                        })
                        .unwrap_or(0);
                    if received.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            sender
                .send(String::from_utf8(received).expect("ASCII request"))
                .await
                .expect("test observes requests");
            match response {
                MockResponse::Reply(response) => {
                    let _ = stream.write_all(response.as_bytes()).await;
                }
                MockResponse::Disconnect => {}
                MockResponse::Hang => std::future::pending::<()>().await,
            }
        }
    });
    let network = NetworkRuntime::from_overrides(&NetworkEnvOverrides {
        http_request_timeout_ms: Some(timeout_ms),
        no_proxy: Some("*".to_owned()),
        ..Default::default()
    })
    .expect("mock network");
    let mut provider = GithubFeedbackProvider::new(
        network,
        Some(FeedbackGithubToken(Ok("fixture-token".to_owned()))),
    );
    provider.test_endpoint = Some(format!("http://{address}"));
    (provider, MockServer { requests, task })
}

pub(super) fn json_response(status: u16, body: serde_json::Value) -> MockResponse {
    let body = body.to_string();
    MockResponse::Reply(format!(
        "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    ))
}

pub(super) fn issue(number: u64, body: &str) -> serde_json::Value {
    serde_json::json!({"number":number,"html_url":format!("https://github.com/acme/project/issues/{number}"),"state":"open","body":body})
}

pub(super) fn search_response(items: Vec<serde_json::Value>) -> MockResponse {
    json_response(
        200,
        serde_json::json!({"total_count":items.len(),"incomplete_results":false,"items":items}),
    )
}
