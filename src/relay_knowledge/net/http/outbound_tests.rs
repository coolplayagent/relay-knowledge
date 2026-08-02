use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::net::{
    http::{HttpBindAddress, HttpProxyConfig},
    qos::{QosPolicy, QosRuntime, RejectReason},
};

#[test]
fn outbound_json_client_accepts_request_scoped_transport_policy() {
    let config = HttpConfig::new(
        HttpBindAddress::parse("127.0.0.1:8791").expect("bind should parse"),
        Duration::from_secs(5),
        Duration::from_secs(5),
        1024,
        HttpProxyConfig::new(None, Vec::new(), true).expect("proxy should build"),
    )
    .expect("config should build");

    let client =
        outbound_json_client_with_policy(&config, Some(false), Some(Duration::from_millis(25)));

    assert!(client.is_ok());
}

#[tokio::test]
async fn post_json_sends_bounded_worker_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("local addr should load");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("client should connect");
        let mut buffer = vec![0; 1024];
        let count = stream.read(&mut buffer).await.expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..count]);

        assert!(request.starts_with("POST /worker HTTP/1.1"));
        assert!(request.contains("Host: 127.0.0.1"));
        assert!(request.contains("Content-Type: application/json"));
        assert!(request.contains("\"task\":\"ocr\""));

        stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}",
                )
                .await
                .expect("response should write");
    });
    let config = HttpConfig::new(
        HttpBindAddress::parse("127.0.0.1:8791").expect("bind should parse"),
        Duration::from_secs(5),
        Duration::from_secs(5),
        1024,
        HttpProxyConfig::new(None, Vec::new(), true).expect("proxy should build"),
    )
    .expect("config should build");

    let response = post_json(
        &config,
        &format!("http://{addr}/worker"),
        &json!({"task": "ocr"}),
    )
    .await
    .expect("worker response should parse");

    assert_eq!(response["ok"], true);
    server.await.expect("server task should finish");
}

#[tokio::test]
async fn post_json_with_qos_rejects_when_outbound_budget_is_exhausted() {
    let config = HttpConfig::new(
        HttpBindAddress::parse("127.0.0.1:8791").expect("bind should parse"),
        Duration::from_secs(5),
        Duration::from_secs(5),
        1024,
        HttpProxyConfig::new(None, Vec::new(), true).expect("proxy should build"),
    )
    .expect("config should build");
    let qos = QosRuntime::default();
    let policy = QosPolicy::new(1, 1, 1).expect("policy should build");
    let _permit = qos
        .admit_request(&policy)
        .expect("first request should consume budget");

    let error = post_json_with_qos(
        &config,
        &qos,
        &policy,
        "http://127.0.0.1:1/worker",
        &json!({"task": "ocr"}),
    )
    .await
    .expect_err("exhausted request budget should reject before transport");

    assert!(matches!(
        error,
        HttpClientError::QosRejected(RejectReason::RequestBudgetExceeded)
    ));
    assert_eq!(qos.diagnostics_snapshot().rejected_total, 1);
}
