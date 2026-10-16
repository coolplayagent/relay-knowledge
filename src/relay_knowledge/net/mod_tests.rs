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

#[test]
fn refresh_keeps_the_shortest_issued_listener_deadline_until_restart() {
    let runtime = NetworkRuntime::from_overrides(&NetworkEnvOverrides::default()).unwrap();
    let clone = runtime.clone();
    for (timeout_ms, expected_ms) in [(200, 200), (5000, 200), (100, 100), (30000, 100)] {
        runtime
            .refresh_from_overrides(&NetworkEnvOverrides {
                http_request_timeout_ms: Some(timeout_ms),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            runtime.current().http.request_timeout,
            Duration::from_millis(timeout_ms)
        );
        assert_eq!(
            clone.request_timeout_floor(),
            Duration::from_millis(expected_ms)
        );
    }
    assert!(
        runtime
            .refresh_from_overrides(&NetworkEnvOverrides {
                http_request_timeout_ms: Some(0),
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(runtime.request_timeout_floor(), Duration::from_millis(100));
}
