use std::{error::Error, fmt};

use serde::de::DeserializeOwned;

use crate::net::{
    http::qos_request_context_active,
    qos::{QosPermit, QosPolicy, QosRuntime, RejectReason},
};

/// Error raised by QoS-gated outbound reqwest calls.
#[derive(Debug)]
pub enum QosHttpClientError {
    QosRejected(RejectReason),
    Transport(reqwest::Error),
}

impl QosHttpClientError {
    /// Returns whether the transport layer reported a timeout.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Transport(error) if error.is_timeout())
    }
}

impl fmt::Display for QosHttpClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QosRejected(reason) => {
                write!(formatter, "request rejected by QoS: {}", reason.as_str())
            }
            Self::Transport(error) => error.fmt(formatter),
        }
    }
}

impl Error for QosHttpClientError {}

/// Reqwest response that keeps the QoS request permit until the body is consumed.
pub struct QosHttpResponse {
    inner: reqwest::Response,
    qos: Option<QosRuntime>,
    _permit: Option<QosPermit>,
}

impl QosHttpResponse {
    fn with_permit(inner: reqwest::Response, qos: QosRuntime, permit: QosPermit) -> Self {
        Self {
            inner,
            qos: Some(qos),
            _permit: Some(permit),
        }
    }

    fn without_permit(inner: reqwest::Response, qos: QosRuntime) -> Self {
        Self {
            inner,
            qos: Some(qos),
            _permit: None,
        }
    }

    pub fn unmetered(inner: reqwest::Response) -> Self {
        Self {
            inner,
            qos: None,
            _permit: None,
        }
    }

    pub fn status(&self) -> reqwest::StatusCode {
        self.inner.status()
    }

    pub fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    pub async fn json<T>(self) -> Result<T, reqwest::Error>
    where
        T: DeserializeOwned,
    {
        let qos = self.qos.clone();
        record_body_timeout(qos.as_ref(), self.inner.json::<T>().await)
    }

    pub async fn text(self) -> Result<String, reqwest::Error> {
        let qos = self.qos.clone();
        record_body_timeout(qos.as_ref(), self.inner.text().await)
    }

    pub async fn bytes(self) -> Result<Vec<u8>, reqwest::Error> {
        let qos = self.qos.clone();
        record_body_timeout(
            qos.as_ref(),
            self.inner.bytes().await.map(|bytes| bytes.to_vec()),
        )
    }

    pub async fn chunk(&mut self) -> Result<Option<Vec<u8>>, reqwest::Error> {
        record_body_timeout(
            self.qos.as_ref(),
            self.inner
                .chunk()
                .await
                .map(|chunk| chunk.map(|bytes| bytes.to_vec())),
        )
    }
}

/// Sends an outbound reqwest request after acquiring a QoS request permit.
pub async fn send_request_with_qos(
    qos: &QosRuntime,
    policy: &QosPolicy,
    request: reqwest::RequestBuilder,
) -> Result<QosHttpResponse, QosHttpClientError> {
    if qos_request_context_active() {
        return send_request_without_new_permit(qos, request).await;
    }

    let permit = qos
        .admit_request(policy)
        .map_err(QosHttpClientError::QosRejected)?;
    match request.send().await {
        Ok(response) => Ok(QosHttpResponse::with_permit(response, qos.clone(), permit)),
        Err(error) => {
            if error.is_timeout() {
                qos.record_timed_out();
            }
            Err(QosHttpClientError::Transport(error))
        }
    }
}

async fn send_request_without_new_permit(
    qos: &QosRuntime,
    request: reqwest::RequestBuilder,
) -> Result<QosHttpResponse, QosHttpClientError> {
    match request.send().await {
        Ok(response) => Ok(QosHttpResponse::without_permit(response, qos.clone())),
        Err(error) => {
            if error.is_timeout() {
                qos.record_timed_out();
            }
            Err(QosHttpClientError::Transport(error))
        }
    }
}

fn record_body_timeout<T>(
    qos: Option<&QosRuntime>,
    result: Result<T, reqwest::Error>,
) -> Result<T, reqwest::Error> {
    if matches!(&result, Err(error) if error.is_timeout()) {
        if let Some(qos) = qos {
            qos.record_timed_out();
        }
    }
    result
}
