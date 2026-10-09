//! Credential handling, bounded response streaming, and failure classification.

use reqwest::{Method, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;

use super::{
    FeedbackProviderError, FeedbackProviderErrorKind, GITHUB_API, GithubFeedbackProvider, failure,
    uncertain_kind,
};
use crate::net::http::{QosHttpClientError, QosHttpResponse, send_request_with_qos};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

impl GithubFeedbackProvider {
    pub(super) fn request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<RequestBuilder, FeedbackProviderError> {
        let token = self.token.as_ref().ok_or_else(|| failure(FeedbackProviderErrorKind::Rejected,
            "GitHub feedback credential is missing; configure RELAY_KNOWLEDGE_FEEDBACK_GITHUB_TOKEN"))?;
        let token = token.0.as_ref().map_err(|()| {
            failure(
                FeedbackProviderErrorKind::Rejected,
                "GitHub feedback credential is invalid; correct the credential before retrying",
            )
        })?;
        let mut authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| {
                failure(
                    FeedbackProviderErrorKind::Rejected,
                    "GitHub feedback credential is invalid",
                )
            })?;
        authorization.set_sensitive(true);
        #[cfg(test)]
        let endpoint = self.test_endpoint.as_deref().unwrap_or(GITHUB_API);
        #[cfg(not(test))]
        let endpoint = GITHUB_API;
        let client = self.client.as_ref().map_err(Clone::clone)?;
        Ok(client
            .request(method, format!("{endpoint}{path}"))
            .header(reqwest::header::AUTHORIZATION, authorization)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28"))
    }

    pub(super) async fn execute<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        creating: bool,
    ) -> Result<T, FeedbackProviderError> {
        let config = self.network.current();
        let response = send_request_with_qos(&self.network.qos_runtime(), &config.qos, request).await
            .map_err(|error| match error {
                QosHttpClientError::QosRejected(_) => failure(FeedbackProviderErrorKind::Retryable, "GitHub feedback request was rejected by local QoS before transmission"),
                QosHttpClientError::Transport(error) if error.is_builder() => failure(FeedbackProviderErrorKind::Rejected, "GitHub feedback request configuration is invalid"),
                QosHttpClientError::Transport(_) => failure(uncertain_kind(creating), "GitHub feedback transport failed; reconcile an uncertain creation before retrying"),
            })?;
        classify_status(response.status(), creating)?;
        let bytes = bounded_body(response, creating).await?;
        serde_json::from_slice(&bytes).map_err(|_| {
            failure(
                uncertain_kind(creating),
                "GitHub returned an invalid feedback response",
            )
        })
    }
}

fn classify_status(status: StatusCode, creating: bool) -> Result<(), FeedbackProviderError> {
    if status
        == if creating {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        }
    {
        return Ok(());
    }
    let kind = match status.as_u16() {
        400 | 401 | 403 | 404 | 410 | 422 => FeedbackProviderErrorKind::Rejected,
        429 => FeedbackProviderErrorKind::Retryable,
        _ => uncertain_kind(creating),
    };
    Err(failure(
        kind,
        &format!(
            "GitHub feedback request returned HTTP {}; response content was withheld",
            status.as_u16()
        ),
    ))
}

async fn bounded_body(
    mut response: QosHttpResponse,
    creating: bool,
) -> Result<Vec<u8>, FeedbackProviderError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(failure(
            uncertain_kind(creating),
            "GitHub feedback response exceeded its byte budget",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        failure(
            uncertain_kind(creating),
            "GitHub feedback response body was interrupted",
        )
    })? {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
            return Err(failure(
                uncertain_kind(creating),
                "GitHub feedback response exceeded its byte budget",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod transport_tests;
