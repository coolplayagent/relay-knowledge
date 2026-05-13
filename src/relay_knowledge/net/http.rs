//! HTTP runtime policy owned by the network boundary.
//!
//! This module intentionally models configuration and validation only. Future
//! HTTP server/client adapters must use a mature async runtime underneath this
//! boundary and keep QoS admission in `net::qos`.

use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, IntoFuture, Ready, ready},
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
    serve::{IncomingStream, Listener},
};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tower::Service;

use crate::{
    env::NetworkEnvOverrides,
    net::qos::{QosPermit, QosPolicy, QosRuntime, RejectReason},
};

pub const DEFAULT_HTTP_BIND: &str = "127.0.0.1:8791";
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_MAX_BODY_BYTES: u64 = 1_048_576;
pub const DEFAULT_SSL_VERIFY: bool = true;

/// Event-driven HTTP configuration for future inbound and outbound adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConfig {
    pub bind_address: HttpBindAddress,
    pub request_timeout: Duration,
    pub graceful_shutdown_timeout: Duration,
    pub max_request_body_bytes: u64,
    pub proxy: HttpProxyConfig,
}

/// Validated HTTP bind address in `host:port` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpBindAddress {
    value: String,
    port: u16,
}

impl HttpBindAddress {
    /// Parses a host or IP literal with an explicit non-zero port.
    pub fn parse(value: &str) -> Result<Self, HttpConfigError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(HttpConfigError::InvalidBindAddress {
                value: value.to_owned(),
            });
        }

        if let Ok(socket_addr) = trimmed.parse::<std::net::SocketAddr>() {
            return Self::from_parts(trimmed.to_owned(), socket_addr.port());
        }

        let Some((host, port)) = trimmed.rsplit_once(':') else {
            return Err(HttpConfigError::InvalidBindAddress {
                value: value.to_owned(),
            });
        };

        if host.is_empty() || host.contains('/') || host.contains(char::is_whitespace) {
            return Err(HttpConfigError::InvalidBindAddress {
                value: value.to_owned(),
            });
        }

        let port = port
            .parse::<u16>()
            .map_err(|_| HttpConfigError::InvalidBindAddress {
                value: value.to_owned(),
            })?;

        Self::from_parts(trimmed.to_owned(), port)
    }

    /// Returns the explicit TCP port.
    pub const fn port(&self) -> u16 {
        self.port
    }

    fn from_parts(value: String, port: u16) -> Result<Self, HttpConfigError> {
        if port == 0 {
            return Err(HttpConfigError::EphemeralPort);
        }

        Ok(Self { value, port })
    }
}

impl fmt::Display for HttpBindAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

/// Outbound HTTP proxy and TLS verification policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpProxyConfig {
    pub proxy: Option<String>,
    pub no_proxy_rules: Vec<String>,
    pub ssl_verify: bool,
}

impl HttpProxyConfig {
    /// Validates proxy URL shape and no-proxy entries without exposing credentials.
    pub fn new(
        proxy: Option<String>,
        no_proxy_rules: Vec<String>,
        ssl_verify: bool,
    ) -> Result<Self, HttpConfigError> {
        if let Some(proxy_url) = proxy.as_deref() {
            validate_proxy_url(proxy_url)?;
        }

        for rule in &no_proxy_rules {
            if rule.trim().is_empty() {
                return Err(HttpConfigError::EmptyNoProxyRule);
            }
        }

        Ok(Self {
            proxy,
            no_proxy_rules,
            ssl_verify,
        })
    }

    /// Applies proxy, no-proxy, and TLS verification environment overrides.
    pub fn from_overrides(overrides: &NetworkEnvOverrides) -> Result<Self, HttpConfigError> {
        Self::new(
            overrides.proxy.clone(),
            parse_no_proxy_rules(overrides.no_proxy.as_deref())?,
            overrides.ssl_verify.unwrap_or(DEFAULT_SSL_VERIFY),
        )
    }

    /// Returns whether outbound HTTP should use a proxy.
    pub fn is_proxy_configured(&self) -> bool {
        self.proxy.is_some()
    }
}

impl HttpConfig {
    /// Builds HTTP config while enforcing bounded request and shutdown behavior.
    pub fn new(
        bind_address: HttpBindAddress,
        request_timeout: Duration,
        graceful_shutdown_timeout: Duration,
        max_request_body_bytes: u64,
        proxy: HttpProxyConfig,
    ) -> Result<Self, HttpConfigError> {
        if request_timeout.is_zero() {
            return Err(HttpConfigError::ZeroDuration {
                field: "request_timeout",
            });
        }

        if graceful_shutdown_timeout.is_zero() {
            return Err(HttpConfigError::ZeroDuration {
                field: "graceful_shutdown_timeout",
            });
        }

        if max_request_body_bytes == 0 {
            return Err(HttpConfigError::ZeroMaxBodyBytes);
        }

        Ok(Self {
            bind_address,
            request_timeout,
            graceful_shutdown_timeout,
            max_request_body_bytes,
            proxy,
        })
    }

    /// Applies environment overrides to the default local HTTP policy.
    pub fn from_overrides(overrides: &NetworkEnvOverrides) -> Result<Self, HttpConfigError> {
        let bind_value = overrides.http_bind.as_deref().unwrap_or(DEFAULT_HTTP_BIND);
        let bind_address = HttpBindAddress::parse(bind_value)?;
        let request_timeout = overrides
            .http_request_timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_REQUEST_TIMEOUT);
        let shutdown_timeout = overrides
            .http_shutdown_timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_SHUTDOWN_TIMEOUT);
        let max_body_bytes = overrides
            .http_max_body_bytes
            .unwrap_or(DEFAULT_MAX_BODY_BYTES);
        let proxy = HttpProxyConfig::from_overrides(overrides)?;

        Self::new(
            bind_address,
            request_timeout,
            shutdown_timeout,
            max_body_bytes,
            proxy,
        )
    }
}

/// HTTP configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpConfigError {
    InvalidBindAddress { value: String },
    EphemeralPort,
    ZeroDuration { field: &'static str },
    ZeroMaxBodyBytes,
    InvalidProxyUrl,
    EmptyNoProxyRule,
}

impl fmt::Display for HttpConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBindAddress { value } => {
                write!(formatter, "bind address '{value}' is not host:port")
            }
            Self::EphemeralPort => write!(formatter, "bind address must use an explicit port"),
            Self::ZeroDuration { field } => write!(formatter, "{field} must be greater than zero"),
            Self::ZeroMaxBodyBytes => write!(
                formatter,
                "max request body bytes must be greater than zero"
            ),
            Self::InvalidProxyUrl => write!(
                formatter,
                "proxy must use http:// or https:// and include a host"
            ),
            Self::EmptyNoProxyRule => write!(formatter, "no-proxy entries must not be empty"),
        }
    }
}

impl Error for HttpConfigError {}

/// Error raised while serving an event-driven HTTP adapter.
#[derive(Debug)]
pub enum HttpServeError {
    Bind(std::io::Error),
    Serve(std::io::Error),
    ShutdownTimeout,
}

impl fmt::Display for HttpServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(error) => write!(formatter, "failed to bind HTTP listener: {error}"),
            Self::Serve(error) => write!(formatter, "HTTP server failed: {error}"),
            Self::ShutdownTimeout => write!(formatter, "HTTP graceful shutdown timed out"),
        }
    }
}

impl Error for HttpServeError {}

/// Error raised by bounded outbound JSON HTTP calls.
#[derive(Debug)]
pub enum HttpClientError {
    InvalidUrl(String),
    Io(io::Error),
    Timeout,
    InvalidResponse,
    ResponseStatus(u16),
    ResponseJson(serde_json::Error),
}

impl fmt::Display for HttpClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(value) => write!(formatter, "invalid HTTP worker URL: {value}"),
            Self::Io(error) => write!(formatter, "HTTP worker request failed: {error}"),
            Self::Timeout => write!(formatter, "HTTP worker request timed out"),
            Self::InvalidResponse => write!(formatter, "HTTP worker returned invalid response"),
            Self::ResponseStatus(status) => {
                write!(formatter, "HTTP worker returned status {status}")
            }
            Self::ResponseJson(error) => {
                write!(formatter, "HTTP worker returned invalid JSON: {error}")
            }
        }
    }
}

impl Error for HttpClientError {}

/// Posts a JSON payload through the network boundary using the configured timeout.
pub async fn post_json(
    config: &HttpConfig,
    url: &str,
    payload: &Value,
) -> Result<Value, HttpClientError> {
    let request = JsonHttpRequest::parse(url)?;
    let body = serde_json::to_vec(payload).map_err(HttpClientError::ResponseJson)?;
    let response = tokio::time::timeout(config.request_timeout, send_json_request(request, body))
        .await
        .map_err(|_| HttpClientError::Timeout)??;

    serde_json::from_slice(&response).map_err(HttpClientError::ResponseJson)
}

struct JsonHttpRequest {
    host: String,
    port: u16,
    path: String,
}

impl JsonHttpRequest {
    fn parse(value: &str) -> Result<Self, HttpClientError> {
        let remainder = value
            .strip_prefix("http://")
            .ok_or_else(|| HttpClientError::InvalidUrl(value.to_owned()))?;
        let (authority, path) = remainder
            .split_once('/')
            .map_or((remainder, "/"), |(authority, path)| {
                (authority, path.trim_start_matches('/'))
            });
        if authority.is_empty() {
            return Err(HttpClientError::InvalidUrl(value.to_owned()));
        }
        let (host, port) = authority
            .rsplit_once(':')
            .map(|(host, port)| {
                let parsed_port = port
                    .parse::<u16>()
                    .map_err(|_| HttpClientError::InvalidUrl(value.to_owned()))?;
                Ok((host.to_owned(), parsed_port))
            })
            .unwrap_or_else(|| Ok((authority.to_owned(), 80)))?;
        if host.is_empty() || port == 0 {
            return Err(HttpClientError::InvalidUrl(value.to_owned()));
        }
        let path = if path.is_empty() {
            "/".to_owned()
        } else {
            format!("/{path}")
        };

        Ok(Self { host, port, path })
    }
}

async fn send_json_request(
    request: JsonHttpRequest,
    body: Vec<u8>,
) -> Result<Vec<u8>, HttpClientError> {
    let mut stream = tokio::net::TcpStream::connect((request.host.as_str(), request.port))
        .await
        .map_err(HttpClientError::Io)?;
    let head = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        request.path,
        request.host,
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(HttpClientError::Io)?;
    stream.write_all(&body).await.map_err(HttpClientError::Io)?;
    stream.shutdown().await.map_err(HttpClientError::Io)?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .map_err(HttpClientError::Io)?;
    parse_http_response(response)
}

fn parse_http_response(response: Vec<u8>) -> Result<Vec<u8>, HttpClientError> {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Err(HttpClientError::InvalidResponse);
    };
    let headers = std::str::from_utf8(&response[..header_end])
        .map_err(|_| HttpClientError::InvalidResponse)?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(HttpClientError::InvalidResponse)?;
    if !(200..300).contains(&status) {
        return Err(HttpClientError::ResponseStatus(status));
    }

    Ok(response[header_end + 4..].to_vec())
}

/// Stable identifier assigned to an accepted HTTP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HttpConnectionId(u64);

impl HttpConnectionId {
    /// Returns the numeric connection identifier for request correlation.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Starts an async HTTP server with graceful shutdown under the network boundary.
pub async fn serve_router(
    router: Router,
    config: HttpConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), HttpServeError> {
    let listener = tokio::net::TcpListener::bind(config.bind_address.to_string())
        .await
        .map_err(HttpServeError::Bind)?;

    serve_listener(listener, router, config, shutdown).await
}

/// Starts an async HTTP server whose accepted connections consume QoS permits.
pub async fn serve_router_with_qos(
    router: Router,
    config: HttpConfig,
    qos: QosRuntime,
    policy: QosPolicy,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), HttpServeError> {
    let listener = tokio::net::TcpListener::bind(config.bind_address.to_string())
        .await
        .map_err(HttpServeError::Bind)?;
    let listener = QosTcpListener::new(listener, qos, policy);

    serve_listener(listener, router, config, shutdown).await
}

async fn serve_listener<L>(
    listener: L,
    router: Router,
    config: HttpConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), HttpServeError>
where
    L: Listener,
    L::Addr: fmt::Debug,
{
    let (shutdown_started, mut shutdown_observed) = tokio::sync::watch::channel(false);
    let graceful_shutdown = async move {
        shutdown.await;
        let _ = shutdown_started.send(true);
    };
    let server = axum::serve(
        listener,
        HttpMakeService::new(router, config.request_timeout),
    )
    .with_graceful_shutdown(graceful_shutdown)
    .into_future();

    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result.map_err(HttpServeError::Serve),
        changed = shutdown_observed.changed() => {
            let _ = changed;
            match tokio::time::timeout(config.graceful_shutdown_timeout, &mut server).await {
                Ok(result) => result.map_err(HttpServeError::Serve),
                Err(_) => Err(HttpServeError::ShutdownTimeout),
            }
        }
    }
}

struct HttpMakeService {
    router: Router,
    request_timeout: Duration,
    next_connection_id: Arc<AtomicU64>,
}

impl HttpMakeService {
    fn new(router: Router, request_timeout: Duration) -> Self {
        Self {
            router,
            request_timeout,
            next_connection_id: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl<'a, L> Service<IncomingStream<'a, L>> for HttpMakeService
where
    L: Listener,
{
    type Response = HttpConnectionService<Router>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: IncomingStream<'a, L>) -> Self::Future {
        let connection_id =
            HttpConnectionId(self.next_connection_id.fetch_add(1, Ordering::Relaxed));
        ready(Ok(HttpConnectionService::new(
            self.router.clone(),
            connection_id,
            self.request_timeout,
        )))
    }
}

struct HttpConnectionService<S> {
    inner: S,
    connection_id: HttpConnectionId,
    request_timeout: Duration,
}

impl<S> HttpConnectionService<S> {
    fn new(inner: S, connection_id: HttpConnectionId, request_timeout: Duration) -> Self {
        Self {
            inner,
            connection_id,
            request_timeout,
        }
    }
}

impl<S> Clone for HttpConnectionService<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            connection_id: self.connection_id,
            request_timeout: self.request_timeout,
        }
    }
}

impl<S> Service<Request> for HttpConnectionService<S>
where
    S: Service<Request, Response = Response, Error = Infallible> + Send,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, mut request: Request) -> Self::Future {
        request.extensions_mut().insert(self.connection_id);
        let future = self.inner.call(request);
        let request_timeout = self.request_timeout;
        Box::pin(async move {
            match tokio::time::timeout(request_timeout, future).await {
                Ok(result) => result,
                Err(_) => Ok((StatusCode::REQUEST_TIMEOUT, "request timed out").into_response()),
            }
        })
    }
}

struct QosTcpListener {
    inner: tokio::net::TcpListener,
    qos: QosRuntime,
    policy: QosPolicy,
}

impl QosTcpListener {
    fn new(inner: tokio::net::TcpListener, qos: QosRuntime, policy: QosPolicy) -> Self {
        Self { inner, qos, policy }
    }
}

impl Listener for QosTcpListener {
    type Io = QosTcpStream;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.inner.accept().await {
                Ok((stream, address)) => match self.qos.admit_connection(&self.policy) {
                    Ok(permit) => {
                        return (
                            QosTcpStream {
                                inner: stream,
                                _permit: permit,
                            },
                            address,
                        );
                    }
                    Err(RejectReason::ConnectionBudgetExceeded) => drop(stream),
                    Err(_) => drop(stream),
                },
                Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

struct QosTcpStream {
    inner: tokio::net::TcpStream,
    _permit: QosPermit,
}

impl AsyncRead for QosTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for QosTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

fn validate_proxy_url(value: &str) -> Result<(), HttpConfigError> {
    let Some((scheme, remainder)) = value.split_once("://") else {
        return Err(HttpConfigError::InvalidProxyUrl);
    };

    if !matches!(scheme, "http" | "https") {
        return Err(HttpConfigError::InvalidProxyUrl);
    }

    let authority = remainder.split('/').next().unwrap_or_default();
    if authority.is_empty() || authority.starts_with('@') {
        return Err(HttpConfigError::InvalidProxyUrl);
    }
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if let Some(remainder) = host_port.strip_prefix('[') {
        remainder.split_once(']').map_or("", |(host, _)| host)
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    if host.is_empty() {
        return Err(HttpConfigError::InvalidProxyUrl);
    }

    Ok(())
}

fn parse_no_proxy_rules(value: Option<&str>) -> Result<Vec<String>, HttpConfigError> {
    value
        .map(|rules| {
            rules
                .split(',')
                .map(str::trim)
                .map(|rule| {
                    if rule.is_empty() {
                        Err(HttpConfigError::EmptyNoProxyRule)
                    } else {
                        Ok(rule.to_owned())
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

#[cfg(test)]
mod tests {
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

            let error =
                HttpConfig::from_overrides(&overrides).expect_err("invalid proxy should fail");

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
        let bind = format!("127.0.0.1:{}", unused_port());
        let config = HttpConfig::new(
            HttpBindAddress::parse(&bind).expect("bind should parse"),
            Duration::from_secs(5),
            Duration::from_millis(10),
            1024,
            HttpProxyConfig::new(None, Vec::new(), true).expect("proxy should build"),
        )
        .expect("config should build");
        let router = Router::new().route(
            "/hold",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "done"
            }),
        );
        let (shutdown, shutdown_waiter) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_router(router, config, async {
            let _ = shutdown_waiter.await;
        }));

        let stream = connect_with_retry(&bind).await;
        let request = b"GET /hold HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream
            .writable()
            .await
            .expect("stream should become writable");
        stream.try_write(request).expect("request should write");
        tokio::time::sleep(Duration::from_millis(20)).await;
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
}
