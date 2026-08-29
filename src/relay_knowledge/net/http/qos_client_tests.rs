use std::time::Duration;

use tokio::io::AsyncWriteExt;

use super::*;
use crate::net::qos::{QosPolicy, QosRuntime};

#[tokio::test]
async fn send_request_with_qos_holds_permit_until_body_is_consumed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("local addr should load");
    let (release_body, wait_for_release) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("client should connect");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
            .await
            .expect("headers should write");
        let _ = wait_for_release.await;
        stream.write_all(b"ok").await.expect("body should write");
    });
    let qos = QosRuntime::default();
    let policy = QosPolicy::new(8, 1, 8).expect("policy should build");
    let client = reqwest::Client::new();

    let response = send_request_with_qos(&qos, &policy, client.get(format!("http://{addr}/slow")))
        .await
        .expect("response headers should arrive");

    assert_eq!(qos.diagnostics_snapshot().usage.in_flight_requests, 1);
    release_body.send(()).expect("body release should send");
    assert_eq!(
        response.text().await.expect("body should read"),
        "ok".to_owned()
    );
    assert_eq!(qos.diagnostics_snapshot().usage.in_flight_requests, 0);
    server.await.expect("server task should finish");
}

#[tokio::test]
async fn qos_response_records_timeout_while_reading_body() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("local addr should load");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("client should connect");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
            .await
            .expect("headers should write");
        tokio::time::sleep(Duration::from_millis(200)).await;
    });
    let qos = QosRuntime::default();
    let policy = QosPolicy::new(8, 1, 8).expect("policy should build");
    let client = reqwest::Client::new();

    let response = send_request_with_qos(
        &qos,
        &policy,
        client
            .get(format!("http://{addr}/slow"))
            .timeout(Duration::from_millis(50)),
    )
    .await
    .expect("response headers should arrive");
    let error = response
        .text()
        .await
        .expect_err("body read should time out");

    assert!(error.is_timeout());
    assert_eq!(qos.diagnostics_snapshot().timed_out_total, 1);
    assert_eq!(qos.diagnostics_snapshot().usage.in_flight_requests, 0);
    server.await.expect("server task should finish");
}

#[tokio::test]
async fn cancelled_send_records_cancellation_and_releases_permit() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("local addr should load");
    let (accepted_sender, accepted_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("client should connect");
        let _ = accepted_sender.send(());
        std::future::pending::<()>().await;
    });
    let qos = QosRuntime::default();
    let policy = QosPolicy::new(8, 1, 8).expect("policy should build");
    let request_qos = qos.clone();
    let request = tokio::spawn(async move {
        send_request_with_qos(
            &request_qos,
            &policy,
            reqwest::Client::new().get(format!("http://{addr}/cancel")),
        )
        .await
    });

    accepted_receiver
        .await
        .expect("server should observe admitted request");
    request.abort();
    let _ = request.await;
    tokio::task::yield_now().await;

    let diagnostics = qos.diagnostics_snapshot();
    assert_eq!(diagnostics.cancelled_total, 1);
    assert_eq!(diagnostics.usage.in_flight_requests, 0);
    server.abort();
}
