//! HTTP runtime policy owned by the network boundary.
//!
//! This module intentionally models configuration and validation only. Future
//! HTTP server/client adapters must use a mature async runtime underneath this
//! boundary and keep QoS admission in `net::qos`.

use std::{error::Error, fmt, time::Duration};

use crate::env::NetworkEnvOverrides;

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
        let overrides = NetworkEnvOverrides {
            proxy: Some("socks5://proxy.internal:1080".to_owned()),
            ..NetworkEnvOverrides::default()
        };

        let error = HttpConfig::from_overrides(&overrides)
            .expect_err("unsupported proxy scheme should fail");

        assert_eq!(error, HttpConfigError::InvalidProxyUrl);
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
}
