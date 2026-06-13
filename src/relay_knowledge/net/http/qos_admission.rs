use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use axum::{
    extract::Request,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use tower::{Layer, Service};

use crate::net::qos::{QosPolicy, QosRuntime, RejectReason};

#[derive(Clone)]
pub(crate) struct QosRequestLayer {
    qos: QosRuntime,
    policy: QosPolicy,
}

impl QosRequestLayer {
    pub(crate) fn new(qos: QosRuntime, policy: QosPolicy) -> Self {
        Self { qos, policy }
    }
}

impl<S> Layer<S> for QosRequestLayer {
    type Service = QosRequestService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        QosRequestService {
            inner,
            qos: self.qos.clone(),
            policy: self.policy.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct QosRequestService<S> {
    inner: S,
    qos: QosRuntime,
    policy: QosPolicy,
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
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let permit = match qos.admit_request(&policy) {
                Ok(permit) => permit,
                Err(reason) => {
                    return Ok(qos_rejection_response(request.uri().path(), reason));
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

fn qos_rejection_response(path: &str, reason: RejectReason) -> Response {
    if path == "/api" || path.starts_with("/api/") {
        let body = format!(
            r#"{{"error_kind":"qos_rejected","message":"QoS request admission rejected: {}"}}"#,
            reason.as_str()
        );
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response();
    }

    (StatusCode::TOO_MANY_REQUESTS, reason.as_str()).into_response()
}
