use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{Method, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use tower::{Layer, Service};

use crate::net::qos::{QosPolicy, QosRuntime};

type BypassValidator = Arc<dyn Fn(&Parts, &[u8]) -> bool + Send + Sync>;

/// Bounded request-body predicate for priority control traffic.
#[derive(Clone)]
pub struct QosRequestBypass {
    method: Method,
    path: String,
    json_field: &'static str,
    json_value: &'static str,
    max_body_bytes: usize,
    validator: Option<BypassValidator>,
}

impl QosRequestBypass {
    /// Matches a small JSON request whose top-level string field has a value.
    pub fn json_field(
        method: Method,
        path: impl Into<String>,
        json_field: &'static str,
        json_value: &'static str,
        max_body_bytes: usize,
    ) -> Self {
        Self::json_field_with_validator(
            method,
            path,
            json_field,
            json_value,
            max_body_bytes,
            |_parts, _body| true,
        )
    }

    /// Matches a small JSON request and then applies an owner-specific validator.
    pub fn json_field_with_validator(
        method: Method,
        path: impl Into<String>,
        json_field: &'static str,
        json_value: &'static str,
        max_body_bytes: usize,
        validator: impl Fn(&Parts, &[u8]) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            method,
            path: path.into(),
            json_field,
            json_value,
            max_body_bytes,
            validator: Some(Arc::new(validator)),
        }
    }

    fn matches_head(&self, request: &Request) -> bool {
        request.method() == self.method && request.uri().path() == self.path
    }

    fn permits_body_inspection(&self, request: &Request) -> bool {
        request
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length <= self.max_body_bytes)
    }

    fn matches_body(&self, body: &[u8]) -> bool {
        serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|value| {
                value
                    .get(self.json_field)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .is_some_and(|value| value == self.json_value)
    }

    fn validates(&self, parts: &Parts, body: &[u8]) -> bool {
        self.validator
            .as_ref()
            .is_none_or(|validator| validator(parts, body))
    }
}

#[derive(Clone)]
pub(crate) struct QosRequestLayer {
    qos: QosRuntime,
    policy: QosPolicy,
    bypasses: Arc<Vec<QosRequestBypass>>,
}

impl QosRequestLayer {
    pub(crate) fn new(qos: QosRuntime, policy: QosPolicy, bypasses: Vec<QosRequestBypass>) -> Self {
        Self {
            qos,
            policy,
            bypasses: Arc::new(bypasses),
        }
    }
}

impl<S> Layer<S> for QosRequestLayer {
    type Service = QosRequestService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        QosRequestService {
            inner,
            qos: self.qos.clone(),
            policy: self.policy.clone(),
            bypasses: self.bypasses.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct QosRequestService<S> {
    inner: S,
    qos: QosRuntime,
    policy: QosPolicy,
    bypasses: Arc<Vec<QosRequestBypass>>,
}

impl<S> Service<Request> for QosRequestService<S>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let qos = self.qos.clone();
        let policy = self.policy.clone();
        let bypasses = self.bypasses.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let (request, bypassed) = inspect_bypass_request(request, &bypasses).await;
            if bypassed {
                return super::QOS_REQUEST_CONTEXT
                    .scope((), inner.call(request))
                    .await;
            }

            let permit = match qos.admit_request(&policy) {
                Ok(permit) => permit,
                Err(reason) => {
                    return Ok((StatusCode::TOO_MANY_REQUESTS, reason.as_str()).into_response());
                }
            };
            let result = super::QOS_REQUEST_CONTEXT
                .scope((), inner.call(request))
                .await;
            drop(permit);
            result
        })
    }
}

async fn inspect_bypass_request(
    request: Request,
    bypasses: &[QosRequestBypass],
) -> (Request, bool) {
    let Some(max_body_bytes) = bypasses
        .iter()
        .filter(|bypass| bypass.matches_head(&request) && bypass.permits_body_inspection(&request))
        .map(|bypass| bypass.max_body_bytes)
        .max()
    else {
        return (request, false);
    };

    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, max_body_bytes).await else {
        return (Request::from_parts(parts, Body::empty()), false);
    };
    let bypassed = bypasses.iter().any(|bypass| {
        parts.method == bypass.method
            && parts.uri.path() == bypass.path
            && bypass.matches_body(bytes.as_ref())
            && bypass.validates(&parts, bytes.as_ref())
    });
    (Request::from_parts(parts, Body::from(bytes)), bypassed)
}
