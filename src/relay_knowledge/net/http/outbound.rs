use std::{error::Error, fmt, io, time::Duration};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::net::qos::{QosPolicy, QosRuntime, RejectReason};

use super::{HttpConfig, qos_request_context_active};

/// Builds an async outbound JSON client from validated network policy.
pub fn outbound_json_client(config: &HttpConfig) -> Result<reqwest::Client, OutboundClientError> {
    outbound_json_client_with_policy(config, None, None)
}

/// Builds an async outbound JSON client with request-scoped transport policy.
pub fn outbound_json_client_with_policy(
    config: &HttpConfig,
    ssl_verify: Option<bool>,
    connect_timeout: Option<Duration>,
) -> Result<reqwest::Client, OutboundClientError> {
    let mut builder = reqwest::Client::builder()
        .timeout(config.request_timeout)
        .danger_accept_invalid_certs(!ssl_verify.unwrap_or(config.proxy.ssl_verify));
    if let Some(timeout) = connect_timeout {
        builder = builder.connect_timeout(timeout);
    }
    if let Some(proxy_url) = &config.proxy.proxy {
        let no_proxy = reqwest::NoProxy::from_string(&config.proxy.no_proxy_rules.join(","));
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|error| OutboundClientError {
                message: error.to_string(),
            })?
            .no_proxy(no_proxy);
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|error| OutboundClientError {
        message: error.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundClientError {
    pub message: String,
}
impl fmt::Display for OutboundClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for OutboundClientError {}

/// Error raised by bounded outbound JSON HTTP calls.
#[derive(Debug)]
pub enum HttpClientError {
    InvalidUrl(String),
    QosRejected(RejectReason),
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
            Self::QosRejected(reason) => write!(
                formatter,
                "HTTP worker request rejected by QoS: {}",
                reason.as_str()
            ),
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

/// Posts JSON through the raw worker HTTP helper after outbound QoS admission.
pub async fn post_json_with_qos(
    config: &HttpConfig,
    qos: &QosRuntime,
    policy: &QosPolicy,
    url: &str,
    payload: &Value,
) -> Result<Value, HttpClientError> {
    let permit = if qos_request_context_active() {
        None
    } else {
        Some(
            qos.admit_request(policy)
                .map_err(HttpClientError::QosRejected)?,
        )
    };
    let result = post_json(config, url, payload).await;
    drop(permit);
    if matches!(result, Err(HttpClientError::Timeout)) {
        qos.record_timed_out();
    }

    result
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

#[cfg(test)]
#[path = "outbound_tests.rs"]
mod outbound_tests;
