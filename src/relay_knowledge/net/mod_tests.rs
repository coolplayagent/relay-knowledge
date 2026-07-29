use super::*;
use crate::env::PlatformKind;

#[test]
fn resolves_default_network_configuration() {
    let config = NetworkConfig::from_overrides(&NetworkEnvOverrides::default())
        .expect("defaults should resolve");

    assert_eq!(config.http.bind_address.to_string(), "127.0.0.1:8791");
    assert!(!config.http.proxy.is_proxy_configured());
    assert!(config.http.proxy.ssl_verify);
    assert_eq!(config.qos.max_connections, 1024);
    assert_eq!(config.qos.max_in_flight_requests, 256);
    assert_eq!(config.qos.max_queue_depth, 512);
}

#[test]
fn refreshes_runtime_network_config_from_environment_snapshot() {
    let runtime = NetworkRuntime::from_overrides(&NetworkEnvOverrides::default())
        .expect("runtime should build");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HTTP_PROXY", "http://relay-proxy:8080"),
            ("NO_PROXY", "localhost"),
            ("SSL_VERIFY", "false"),
            ("RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS", "8"),
        ],
    )
    .expect("environment should parse");

    runtime
        .refresh_from_environment(&environment)
        .expect("network refresh should succeed");
    let config = runtime.current();

    assert_eq!(
        config.http.proxy.proxy,
        Some("http://relay-proxy:8080".to_owned())
    );
    assert_eq!(config.http.proxy.no_proxy_rules, ["localhost"]);
    assert!(!config.http.proxy.ssl_verify);
    assert_eq!(config.qos.max_connections, 8);
}
