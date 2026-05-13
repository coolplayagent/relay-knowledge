use axum::http::{HeaderMap, StatusCode, header};

use crate::{
    api::AgentAccessPolicy,
    net::http::{HttpConfig, remote_clients_allowed},
};

use super::{MCP_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION_HEADER, McpServeError, McpServer};

pub(super) fn validate_http_headers(
    server: &McpServer,
    headers: &HeaderMap,
) -> Result<(), StatusCode> {
    if !content_type_is_json(headers) {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    if !accepts_json(headers) {
        return Err(StatusCode::NOT_ACCEPTABLE);
    }
    validate_protocol_version_header(headers, false)?;
    validate_origin(server, headers)
}

pub(super) fn validate_protocol_version_header(
    headers: &HeaderMap,
    required: bool,
) -> Result<(), StatusCode> {
    let Some(version) = headers.get(MCP_PROTOCOL_VERSION_HEADER) else {
        return if required {
            Err(StatusCode::BAD_REQUEST)
        } else {
            Ok(())
        };
    };
    let Ok(version) = version.to_str() else {
        return Err(StatusCode::BAD_REQUEST);
    };
    if version == MCP_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

pub(super) fn ensure_remote_bind_allowed(
    config: &HttpConfig,
    policy: &AgentAccessPolicy,
) -> Result<(), McpServeError> {
    if remote_clients_allowed(config, policy.allow_remote_clients) {
        Ok(())
    } else {
        Err(McpServeError::RemoteBindDisabled)
    }
}

fn content_type_is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
}

fn accepts_json(headers: &HeaderMap) -> bool {
    let Some(value) = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let ranges = value
        .split(',')
        .filter_map(|item| AcceptRange::parse(item.trim()))
        .collect::<Vec<_>>();

    accepts_media_type(&ranges, "application", "json")
        && accepts_media_type(&ranges, "text", "event-stream")
}

struct AcceptRange<'a> {
    type_name: &'a str,
    subtype: &'a str,
    quality: f32,
}

impl<'a> AcceptRange<'a> {
    fn parse(item: &'a str) -> Option<Self> {
        let mut parts = item.split(';');
        let (type_name, subtype) = parts.next()?.trim().split_once('/')?;
        let mut quality = 1.0;
        for parameter in parts {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("q") {
                quality = value.trim().parse::<f32>().unwrap_or(0.0);
            }
        }

        Some(Self {
            type_name: type_name.trim(),
            subtype: subtype.trim(),
            quality,
        })
    }

    fn specificity_for(&self, type_name: &str, subtype: &str) -> Option<u8> {
        let type_matches = self.type_name == "*" || self.type_name.eq_ignore_ascii_case(type_name);
        let subtype_matches = self.subtype == "*" || self.subtype.eq_ignore_ascii_case(subtype);
        if !type_matches || !subtype_matches {
            return None;
        }

        Some(u8::from(self.type_name != "*") + u8::from(self.subtype != "*"))
    }
}

fn accepts_media_type(ranges: &[AcceptRange<'_>], type_name: &str, subtype: &str) -> bool {
    ranges
        .iter()
        .filter_map(|range| {
            range
                .specificity_for(type_name, subtype)
                .map(|specificity| (specificity, range.quality))
        })
        .max_by_key(|(specificity, _)| *specificity)
        .is_some_and(|(_, quality)| quality > 0.0)
}

fn validate_origin(server: &McpServer, headers: &HeaderMap) -> Result<(), StatusCode> {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return Ok(());
    };
    let Ok(origin) = origin.to_str() else {
        return Err(StatusCode::FORBIDDEN);
    };
    if server
        .agent
        .mcp_allowed_origins
        .iter()
        .any(|allowed| allowed == origin)
    {
        return Ok(());
    }
    if server.agent.mcp_allowed_origins.is_empty() && is_loopback_origin(origin) {
        return Ok(());
    }

    Err(StatusCode::FORBIDDEN)
}

fn is_loopback_origin(origin: &str) -> bool {
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
        .unwrap_or_default();
    let host = origin_host(authority);

    is_loopback_host(host)
}

fn origin_host(authority: &str) -> &str {
    authority_host(authority)
}

fn authority_host(authority: &str) -> &str {
    if let Some(remainder) = authority.strip_prefix('[') {
        return remainder
            .find(']')
            .map_or(authority, |index| &remainder[..index]);
    }

    authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}
