use std::{path::Path, time::Duration};

use crate::api::RuntimeStatus;

use super::RuntimeConfiguration;

pub(super) fn runtime_status(runtime: &RuntimeConfiguration) -> RuntimeStatus {
    let network = runtime.network.current();

    RuntimeStatus {
        config_dir: path_string(&runtime.paths.config_dir),
        data_dir: path_string(&runtime.paths.data_dir),
        state_dir: path_string(&runtime.paths.state_dir),
        cache_dir: path_string(&runtime.paths.cache_dir),
        log_dir: path_string(&runtime.paths.log_dir),
        temp_dir: path_string(&runtime.paths.temp_dir),
        runtime_dir: path_string(&runtime.paths.runtime_dir),
        service_dir: path_string(&runtime.paths.service_dir),
        http_bind: network.http.bind_address.to_string(),
        http_request_timeout_ms: duration_millis(network.http.request_timeout),
        http_graceful_shutdown_timeout_ms: duration_millis(network.http.graceful_shutdown_timeout),
        http_max_request_body_bytes: network.http.max_request_body_bytes,
        http_proxy_configured: network.http.proxy.is_proxy_configured(),
        http_no_proxy_rules: network.http.proxy.no_proxy_rules.len(),
        http_ssl_verify: network.http.proxy.ssl_verify,
        qos_max_connections: network.qos.max_connections,
        qos_max_in_flight_requests: network.qos.max_in_flight_requests,
        qos_max_queue_depth: network.qos.max_queue_depth,
    }
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
