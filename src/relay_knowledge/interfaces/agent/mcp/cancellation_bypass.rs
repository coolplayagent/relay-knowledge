use axum::http::Method;

use crate::net::http::QosRequestBypass;

use super::{
    McpServer,
    http_contract::{validate_http_headers, validate_protocol_version_header},
};

impl McpServer {
    /// Builds the request-admission bypass for valid cancellation notifications.
    pub(crate) fn cancellation_qos_bypass(&self, max_body_bytes: usize) -> QosRequestBypass {
        let server = self.clone();
        QosRequestBypass::json_field_with_validator(
            Method::POST,
            self.agent.mcp_endpoint.clone(),
            "method",
            "notifications/cancelled",
            max_body_bytes,
            move |parts, _body| {
                validate_http_headers(&server, &parts.headers).is_ok()
                    && validate_protocol_version_header(&parts.headers, true).is_ok()
                    && server
                        .sessions
                        .require_session(&parts.headers)
                        .is_ok_and(|session| session.initialized)
            },
        )
    }
}
