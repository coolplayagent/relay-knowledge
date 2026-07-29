use super::*;

#[test]
fn telemetry_config_applies_documented_defaults() {
    let config = TelemetryConfig::from_environment(&TelemetryEnvOverrides::default());

    assert_eq!(config.otel_endpoint, DEFAULT_OTEL_ENDPOINT);
    assert!(!config.traces_enabled);
    assert!(!config.metrics_enabled);
    assert_eq!(config.export_timeout, Duration::from_millis(5_000));
    assert_eq!(config.service_environment, "local");
    assert_eq!(config.trace_endpoint(), "http://127.0.0.1:4318/v1/traces");
    assert_eq!(config.metric_endpoint(), "http://127.0.0.1:4318/v1/metrics");
}

#[test]
fn telemetry_config_uses_validated_environment_overrides() {
    let config = TelemetryConfig::from_environment(&TelemetryEnvOverrides {
        otel_endpoint: Some("http://collector:4318/".to_owned()),
        otel_traces: Some(true),
        otel_metrics: Some(true),
        export_timeout_ms: Some(250),
        service_environment: Some("ci".to_owned()),
    });

    assert_eq!(config.otel_endpoint, "http://collector:4318/");
    assert!(config.traces_enabled);
    assert!(config.metrics_enabled);
    assert_eq!(config.export_timeout, Duration::from_millis(250));
    assert_eq!(config.service_environment, "ci");
    assert_eq!(config.trace_endpoint(), "http://collector:4318/v1/traces");
    assert_eq!(config.metric_endpoint(), "http://collector:4318/v1/metrics");
}

#[test]
fn signal_endpoint_preserves_signal_specific_paths() {
    assert_eq!(
        signal_endpoint("http://collector:4318/v1/traces", OTLP_TRACE_PATH),
        "http://collector:4318/v1/traces"
    );
    assert_eq!(
        signal_endpoint("http://collector:4318/v1/metrics", OTLP_METRIC_PATH),
        "http://collector:4318/v1/metrics"
    );
}

#[test]
fn signal_endpoint_routes_sibling_signals_from_specific_paths() {
    assert_eq!(
        signal_endpoint("http://collector:4318/v1/traces", OTLP_METRIC_PATH),
        "http://collector:4318/v1/metrics"
    );
    assert_eq!(
        signal_endpoint("http://collector:4318/v1/metrics", OTLP_TRACE_PATH),
        "http://collector:4318/v1/traces"
    );
}

#[test]
fn disabled_exporters_still_report_runtime_status() {
    let runtime =
        ObservabilityRuntime::new(TelemetryConfig::from_environment(&TelemetryEnvOverrides {
            otel_endpoint: Some("http://collector:4318".to_owned()),
            service_environment: Some("test".to_owned()),
            ..TelemetryEnvOverrides::default()
        }));

    runtime.initialize();
    let status = runtime.status();

    assert!(status.otlp_endpoint_configured);
    assert!(!status.traces_enabled);
    assert!(!status.metrics_enabled);
    assert!(!status.trace_exporter_initialized);
    assert!(!status.metrics_exporter_initialized);
    assert_eq!(status.export_timeout_ms, 5_000);
    assert_eq!(status.service_environment, "test");
    assert_eq!(status.last_error, None);
}

#[test]
fn agent_protocol_metrics_snapshot_records_all_event_types() {
    let metrics = AgentProtocolMetrics::default();

    metrics.record_request("mcp", "tools/call", "ok", 12, false);
    metrics.record_request("mcp", "resources/read", "ok", 34, true);
    metrics.record_rejection("mcp", "qos");
    metrics.record_cancelled("acp");
    metrics.record_cold_start("mcp", 56);

    assert_eq!(
        metrics.snapshot(),
        AgentProtocolMetricsSnapshot {
            requests_total: 2,
            request_duration_ms_total: 46,
            rejections_total: 1,
            cancelled_total: 1,
            context_truncated_total: 1,
            cold_start_total: 1,
            cold_start_duration_ms_total: 56,
        }
    );
}

#[test]
fn agent_protocol_metrics_saturate_instead_of_wrapping() {
    let metrics = AgentProtocolMetrics::default();
    {
        let mut snapshot = metrics.inner.lock().expect("metrics mutex");
        snapshot.requests_total = u64::MAX;
        snapshot.request_duration_ms_total = u64::MAX - 1;
        snapshot.rejections_total = u64::MAX;
        snapshot.cancelled_total = u64::MAX;
        snapshot.context_truncated_total = u64::MAX;
    }

    metrics.record_request("mcp", "tools/call", "ok", 50, true);
    metrics.record_rejection("mcp", "budget");
    metrics.record_cancelled("mcp");

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.requests_total, u64::MAX);
    assert_eq!(snapshot.request_duration_ms_total, u64::MAX);
    assert_eq!(snapshot.rejections_total, u64::MAX);
    assert_eq!(snapshot.cancelled_total, u64::MAX);
    assert_eq!(snapshot.context_truncated_total, u64::MAX);
}

#[test]
fn duration_millis_saturates_large_durations() {
    let large = Duration::from_millis(u64::MAX) + Duration::from_millis(1);

    assert_eq!(duration_millis(large), u64::MAX);
}

#[test]
fn initialized_telemetry_accumulates_exporter_errors() {
    let mut initialized = InitializedTelemetry::default();

    initialized.push_error("metrics exporter: invalid endpoint".to_owned());
    initialized.push_error("trace exporter: invalid endpoint".to_owned());

    assert_eq!(
        initialized.last_error.as_deref(),
        Some("metrics exporter: invalid endpoint; trace exporter: invalid endpoint")
    );
}

#[test]
fn observability_state_accumulates_shutdown_errors() {
    let mut state = ObservabilityState::default();

    state.push_error("trace shutdown: timed out".to_owned());
    state.push_error("metrics shutdown: already shut down".to_owned());

    assert_eq!(
        state.last_error.as_deref(),
        Some("trace shutdown: timed out; metrics shutdown: already shut down")
    );
}
