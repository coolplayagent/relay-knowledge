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

    /// Returns whether outbound admission rejected the request before I/O.
    pub fn is_qos_rejected(&self) -> bool {
        matches!(self, Self::QosRejected(_))
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
    inner: Option<reqwest::Response>,
    qos: QosRuntime,
    _permit: Option<QosPermit>,
    cancellation: CancellationGuard,
}

impl QosHttpResponse {
    fn with_permit(
        inner: reqwest::Response,
        qos: QosRuntime,
        permit: QosPermit,
        cancellation: CancellationGuard,
    ) -> Self {
        Self {
            inner: Some(inner),
            qos,
            _permit: Some(permit),
            cancellation,
        }
    }

    fn without_permit(
        inner: reqwest::Response,
        qos: QosRuntime,
        cancellation: CancellationGuard,
    ) -> Self {
        Self {
            inner: Some(inner),
            qos,
            _permit: None,
            cancellation,
        }
    }

    pub fn status(&self) -> reqwest::StatusCode {
        self.inner.as_ref().expect("response is available").status()
    }

    pub fn content_length(&self) -> Option<u64> {
        self.inner
            .as_ref()
            .expect("response is available")
            .content_length()
    }

    pub async fn json<T>(mut self) -> Result<T, reqwest::Error>
    where
        T: DeserializeOwned,
    {
        let result = self
            .inner
            .take()
            .expect("response is available")
            .json::<T>()
            .await;
        self.cancellation.complete();
        record_body_timeout(&self.qos, result)
    }

    pub async fn text(mut self) -> Result<String, reqwest::Error> {
        let result = self
            .inner
            .take()
            .expect("response is available")
            .text()
            .await;
        self.cancellation.complete();
        record_body_timeout(&self.qos, result)
    }

    pub async fn bytes(mut self) -> Result<Vec<u8>, reqwest::Error> {
        let result = self
            .inner
            .take()
            .expect("response is available")
            .bytes()
            .await
            .map(|bytes| bytes.to_vec());
        self.cancellation.complete();
        record_body_timeout(&self.qos, result)
    }

    pub async fn chunk(&mut self) -> Result<Option<Vec<u8>>, reqwest::Error> {
        let result = self
            .inner
            .as_mut()
            .expect("response is available")
            .chunk()
            .await
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()));
        if !matches!(&result, Ok(Some(_))) {
            self.cancellation.complete();
        }
        record_body_timeout(&self.qos, result)
    }
}

struct CancellationGuard {
    qos: QosRuntime,
    completed: bool,
}

impl CancellationGuard {
    fn new(qos: QosRuntime) -> Self {
        Self {
            qos,
            completed: false,
        }
    }

    fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.qos.record_cancelled();
        }
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
    let mut cancellation = CancellationGuard::new(qos.clone());
    match request.send().await {
        Ok(response) => Ok(QosHttpResponse::with_permit(
            response,
            qos.clone(),
            permit,
            cancellation,
        )),
        Err(error) => {
            cancellation.complete();
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
    let mut cancellation = CancellationGuard::new(qos.clone());
    match request.send().await {
        Ok(response) => Ok(QosHttpResponse::without_permit(
            response,
            qos.clone(),
            cancellation,
        )),
        Err(error) => {
            cancellation.complete();
            if error.is_timeout() {
                qos.record_timed_out();
            }
            Err(QosHttpClientError::Transport(error))
        }
    }
}

fn record_body_timeout<T>(
    qos: &QosRuntime,
    result: Result<T, reqwest::Error>,
) -> Result<T, reqwest::Error> {
    if matches!(&result, Err(error) if error.is_timeout()) {
        qos.record_timed_out();
    }
    result
}

#[cfg(test)]
#[path = "qos_client_tests.rs"]
mod qos_client_tests;
