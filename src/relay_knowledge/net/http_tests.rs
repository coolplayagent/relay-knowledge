use super::*;
use crate::net::qos::{QosPolicy, QosRuntime};
use axum::{Router, routing::get};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn parses_overridden_http_bind_address() {
    let overrides = NetworkEnvOverrides {
        http_bind: Some("localhost:9000".to_owned()),
        http_request_timeout_ms: Some(1500),
        http_shutdown_timeout_ms: Some(2500),
        http_max_body_bytes: Some(4096),
        proxy: Some("https://proxy.internal:8443".to_owned()),
        no_proxy: Some("localhost,.internal".to_owned()),
        ssl_verify: Some(false),
        ..NetworkEnvOverrides::default()
    };

    let config = HttpConfig::from_overrides(&overrides).expect("config should parse");

    assert_eq!(config.bind_address.to_string(), "localhost:9000");
    assert_eq!(config.bind_address.port(), 9000);
    assert_eq!(config.request_timeout, Duration::from_millis(1500));
    assert_eq!(
        config.graceful_shutdown_timeout,
        Duration::from_millis(2500)
    );
    assert_eq!(config.max_request_body_bytes, 4096);
    assert_eq!(
        config.proxy.proxy,
        Some("https://proxy.internal:8443".to_owned())
    );
    assert_eq!(config.proxy.no_proxy_rules, ["localhost", ".internal"]);
    assert!(!config.proxy.ssl_verify);
}

#[test]
fn rejects_invalid_bind_addresses() {
    let overrides = NetworkEnvOverrides {
        http_bind: Some("localhost".to_owned()),
        ..NetworkEnvOverrides::default()
    };

    let error = HttpConfig::from_overrides(&overrides)
        .expect_err("bind address must include host and port");

    assert_eq!(
        error,
        HttpConfigError::InvalidBindAddress {
            value: "localhost".to_owned()
        }
    );
}

#[test]
fn rejects_ephemeral_ports() {
    let error = HttpBindAddress::parse("127.0.0.1:0").expect_err("port zero should fail");

    assert_eq!(error, HttpConfigError::EphemeralPort);
}

#[test]
fn rejects_proxy_urls_without_supported_scheme_or_host() {
    for proxy in [
        "socks5://proxy.internal:1080",
        "http://:8080",
        "https://user@:443",
    ] {
        let overrides = NetworkEnvOverrides {
            proxy: Some(proxy.to_owned()),
            ..NetworkEnvOverrides::default()
        };

        let error = HttpConfig::from_overrides(&overrides).expect_err("invalid proxy should fail");

        assert_eq!(error, HttpConfigError::InvalidProxyUrl);
    }
}

#[test]
fn rejects_empty_no_proxy_entries() {
    let overrides = NetworkEnvOverrides {
        no_proxy: Some("localhost,,example.com".to_owned()),
        ..NetworkEnvOverrides::default()
    };

    let error =
        HttpConfig::from_overrides(&overrides).expect_err("empty no-proxy entry should fail");

    assert_eq!(error, HttpConfigError::EmptyNoProxyRule);
}

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
async fn serve_router_enforces_graceful_shutdown_timeout() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let bind = listener
        .local_addr()
        .expect("listener should expose local address")
        .to_string();
    let config = HttpConfig::new(
        HttpBindAddress::parse(&bind).expect("bind should parse"),
        Duration::from_secs(5),
        Duration::from_millis(10),
        1024,
        HttpProxyConfig::new(None, Vec::new(), true).expect("proxy should build"),
    )
    .expect("config should build");
    let (handler_started, handler_started_waiter) = tokio::sync::oneshot::channel();
    let route_handler_started = Arc::new(std::sync::Mutex::new(Some(handler_started)));
    let router = Router::new().route(
        "/hold",
        get(move || {
            let handler_started = route_handler_started.clone();
            let sender = handler_started
                .lock()
                .expect("handler signal mutex should not be poisoned")
                .take();
            if let Some(sender) = sender {
                let _ = sender.send(());
            }
            async move { std::future::pending::<&'static str>().await }
        }),
    );
    let (shutdown, shutdown_waiter) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_listener(listener, router, config, async {
        let _ = shutdown_waiter.await;
    }));

    let mut stream = connect_with_retry(&bind).await;
    let request = b"GET /hold HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream
        .write_all(request)
        .await
        .expect("request should write completely");
    tokio::time::timeout(Duration::from_secs(5), handler_started_waiter)
        .await
        .expect("handler should start before shutdown")
        .expect("handler should signal startup");
    let _ = shutdown.send(());

    let error = server
        .await
        .expect("server task should join")
        .expect_err("active request should exceed shutdown timeout");

    assert!(matches!(error, HttpServeError::ShutdownTimeout));
}

#[tokio::test]
async fn serve_router_with_qos_rejects_excess_connections() {
    let bind = format!("127.0.0.1:{}", unused_port());
    let config = HttpConfig::new(
        HttpBindAddress::parse(&bind).expect("bind should parse"),
        Duration::from_secs(5),
        Duration::from_millis(100),
        1024,
        HttpProxyConfig::new(None, Vec::new(), true).expect("proxy should build"),
    )
    .expect("config should build");
    let router = Router::new().route("/ok", get(|| async { "ok" }));
    let qos = QosRuntime::default();
    let policy = QosPolicy::new(1, 4, 4).expect("policy should build");
    let (shutdown, shutdown_waiter) = tokio::sync::oneshot::channel();
    let server_qos = qos.clone();
    let server = tokio::spawn(serve_router_with_qos(
        router,
        config,
        server_qos,
        policy,
        async {
            let _ = shutdown_waiter.await;
        },
    ));

    let first = connect_with_retry(&bind).await;
    wait_for_connection_count(&qos, 1).await;
    let second = connect_with_retry(&bind).await;

    wait_for_peer_close(&second).await;

    drop(first);
    let _ = shutdown.send(());
    server
        .await
        .expect("server task should join")
        .expect("server should stop");
}

fn unused_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("listener should bind")
        .local_addr()
        .expect("listener should expose address")
        .port()
}

async fn connect_with_retry(bind: &str) -> tokio::net::TcpStream {
    for _ in 0..50 {
        if let Ok(stream) = tokio::net::TcpStream::connect(bind).await {
            return stream;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("server did not accept connections on {bind}");
}

async fn wait_for_connection_count(qos: &QosRuntime, expected: usize) {
    for _ in 0..50 {
        if qos.snapshot().connections == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("connection count did not reach {expected}");
}

async fn wait_for_peer_close(stream: &tokio::net::TcpStream) {
    let mut buffer = [0; 1024];
    for _ in 0..50 {
        stream.readable().await.expect("stream should be readable");
        match stream.try_read(&mut buffer) {
            Ok(0) => return,
            Ok(_) => panic!("over-budget connection should close before serving data"),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => return,
            Err(error) => panic!("response read failed: {error}"),
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("server did not close over-budget connection");
}
