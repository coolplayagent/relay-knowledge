use std::time::Duration;

use reqwest::{StatusCode, header};

use crate::{
    net::{
        NetworkRuntime, http,
        qos::{QosPolicy, QosRuntime},
    },
    ports::release_metadata::{
        ReleaseMetadataError, ReleaseMetadataErrorKind, ReleaseMetadataFuture, ReleaseMetadataPort,
        ReleaseMetadataRequest, ReleaseMetadataSession,
    },
    project::PROJECT_NAME,
};

const VERSION_CHECK_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

struct HttpReleaseMetadataSession {
    client: reqwest::Client,
    qos: QosRuntime,
    policy: QosPolicy,
    max_response_bytes: u64,
}

impl ReleaseMetadataPort for NetworkRuntime {
    fn open(&self) -> Result<Box<dyn ReleaseMetadataSession>, ReleaseMetadataError> {
        let config = self.current();
        let client =
            http::outbound_json_client(&config.http).map_err(|error| ReleaseMetadataError {
                kind: ReleaseMetadataErrorKind::ClientBuild,
                message: error.to_string(),
                retryable: false,
            })?;

        Ok(Box::new(HttpReleaseMetadataSession {
            client,
            qos: self.qos_runtime(),
            policy: config.qos,
            max_response_bytes: config.http.max_request_body_bytes,
        }))
    }
}

impl ReleaseMetadataSession for HttpReleaseMetadataSession {
    fn fetch(&self, request: ReleaseMetadataRequest) -> ReleaseMetadataFuture<'_> {
        Box::pin(async move {
            let response = send_request(self, &request.url)
                .await
                .map_err(qos_transport_error)?;
            validate_status(response.status())?;
            read_bounded_body(response, self.max_response_bytes).await
        })
    }
}

async fn send_request(
    session: &HttpReleaseMetadataSession,
    url: &str,
) -> Result<http::QosHttpResponse, http::QosHttpClientError> {
    http::send_request_with_qos(
        &session.qos,
        &session.policy,
        session
            .client
            .get(url)
            .header(
                header::USER_AGENT,
                format!("{PROJECT_NAME}/{}", env!("CARGO_PKG_VERSION")),
            )
            .timeout(VERSION_CHECK_REQUEST_TIMEOUT),
    )
    .await
}

fn validate_status(status: StatusCode) -> Result<(), ReleaseMetadataError> {
    if status.is_success() {
        return Ok(());
    }

    Err(ReleaseMetadataError {
        kind: ReleaseMetadataErrorKind::HttpStatus,
        message: format!("release metadata request returned HTTP {}", status.as_u16()),
        retryable: status.is_server_error()
            || status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::TOO_MANY_REQUESTS,
    })
}

async fn read_bounded_body(
    mut response: http::QosHttpResponse,
    max_response_bytes: u64,
) -> Result<Vec<u8>, ReleaseMetadataError> {
    if response
        .content_length()
        .is_some_and(|length| length > max_response_bytes)
    {
        return Err(response_too_large_error(max_response_bytes));
    }

    let limit = max_response_bytes.try_into().unwrap_or(usize::MAX);
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| ReleaseMetadataError {
            kind: ReleaseMetadataErrorKind::Transport,
            message: error.to_string(),
            retryable: true,
        })?
    {
        append_bounded_body(&mut body, &chunk, limit)?;
    }
    Ok(body)
}

fn append_bounded_body(
    body: &mut Vec<u8>,
    chunk: &[u8],
    max_response_bytes: usize,
) -> Result<(), ReleaseMetadataError> {
    let Some(next_len) = body
        .len()
        .checked_add(chunk.len())
        .filter(|next_len| *next_len <= max_response_bytes)
    else {
        return Err(response_too_large_error(
            max_response_bytes.try_into().unwrap_or(u64::MAX),
        ));
    };
    body.reserve(next_len.saturating_sub(body.len()));
    body.extend_from_slice(chunk);
    Ok(())
}

fn qos_transport_error(error: http::QosHttpClientError) -> ReleaseMetadataError {
    ReleaseMetadataError {
        kind: if error.is_timeout() {
            ReleaseMetadataErrorKind::NetworkTimeout
        } else {
            ReleaseMetadataErrorKind::Network
        },
        message: error.to_string(),
        retryable: true,
    }
}

fn response_too_large_error(max_response_bytes: u64) -> ReleaseMetadataError {
    ReleaseMetadataError {
        kind: ReleaseMetadataErrorKind::ResponseTooLarge,
        message: format!("release metadata response exceeded {max_response_bytes} bytes"),
        retryable: false,
    }
}

#[cfg(test)]
#[path = "release_metadata_tests.rs"]
mod tests;
