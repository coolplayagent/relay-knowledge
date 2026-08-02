//! Direct contracts for foreground service network admission.

use std::time::Duration;

use crate::net::http::{HttpBindAddress, HttpConfig, HttpProxyConfig};

use super::*;

#[test]
fn web_bind_policy_allows_loopback_or_explicit_remote_access() {
    let loopback = http_config("127.0.0.1:8791");
    let remote = http_config("0.0.0.0:8791");

    assert_eq!(ensure_web_remote_bind_allowed(&loopback, false), Ok(()));
    assert!(matches!(
        ensure_web_remote_bind_allowed(&remote, false),
        Err(CliError::ServiceRunFailed(message))
            if message == "Web remote bind requires allow_remote_clients=true"
    ));
    assert_eq!(ensure_web_remote_bind_allowed(&remote, true), Ok(()));
}

fn http_config(bind: &str) -> HttpConfig {
    HttpConfig::new(
        HttpBindAddress::parse(bind).expect("bind address should parse"),
        Duration::from_secs(1),
        Duration::from_secs(1),
        1_024,
        HttpProxyConfig::new(None, Vec::new(), true).expect("proxy policy should validate"),
    )
    .expect("HTTP config should validate")
}
