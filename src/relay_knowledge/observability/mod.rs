//! Observability runtime for local diagnostics and OTLP export.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use opentelemetry::{KeyValue, global, trace::TracerProvider};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{Resource, metrics::SdkMeterProvider, trace::SdkTracerProvider};
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{env::TelemetryEnvOverrides, project::PROJECT_NAME};

const DEFAULT_OTEL_ENDPOINT: &str = "http://127.0.0.1:4318";
const DEFAULT_EXPORT_TIMEOUT_MS: u64 = 5_000;
const OTLP_TRACE_PATH: &str = "/v1/traces";
const OTLP_METRIC_PATH: &str = "/v1/metrics";

/// Runtime telemetry configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryConfig {
    pub otel_endpoint: String,
    pub traces_enabled: bool,
    pub metrics_enabled: bool,
    pub export_timeout: Duration,
    pub service_environment: String,
}

impl TelemetryConfig {
    /// Builds telemetry config from validated environment values.
    pub fn from_environment(environment: &TelemetryEnvOverrides) -> Self {
        Self {
            otel_endpoint: environment
                .otel_endpoint
                .clone()
                .unwrap_or_else(|| DEFAULT_OTEL_ENDPOINT.to_owned()),
            traces_enabled: environment.otel_traces.unwrap_or(false),
            metrics_enabled: environment.otel_metrics.unwrap_or(false),
            export_timeout: Duration::from_millis(
                environment
                    .export_timeout_ms
                    .unwrap_or(DEFAULT_EXPORT_TIMEOUT_MS),
            ),
            service_environment: environment
                .service_environment
                .clone()
                .unwrap_or_else(|| "local".to_owned()),
        }
    }

    fn trace_endpoint(&self) -> String {
        signal_endpoint(&self.otel_endpoint, OTLP_TRACE_PATH)
    }

    fn metric_endpoint(&self) -> String {
        signal_endpoint(&self.otel_endpoint, OTLP_METRIC_PATH)
    }
}

/// Shared observability handles.
#[derive(Debug, Clone)]
pub struct ObservabilityRuntime {
    config: TelemetryConfig,
    state: Arc<Mutex<ObservabilityState>>,
    metrics: AgentProtocolMetrics,
}

#[derive(Debug, Default)]
struct ObservabilityState {
    trace_initialized: bool,
    metrics_initialized: bool,
    trace_provider: Option<SdkTracerProvider>,
    metrics_provider: Option<SdkMeterProvider>,
    last_error: Option<String>,
}

impl ObservabilityRuntime {
    /// Creates the runtime without installing exporters.
    pub fn new(config: TelemetryConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(ObservabilityState::default())),
            metrics: AgentProtocolMetrics::default(),
        }
    }

    /// Installs tracing and OTLP exporters. Exporter failures are captured for diagnostics.
    pub fn initialize(&self) {
        let initialized = self.try_initialize();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.trace_initialized = initialized.trace_initialized;
        state.metrics_initialized = initialized.metrics_initialized;
        state.trace_provider = initialized.trace_provider;
        state.metrics_provider = initialized.metrics_provider;
        state.last_error = initialized.last_error;
    }

    /// Returns a recorder for low-cardinality agent protocol metrics.
    pub fn agent_metrics(&self) -> AgentProtocolMetrics {
        self.metrics.clone()
    }

    /// Returns secret-free diagnostics for service status.
    pub fn status(&self) -> TelemetryStatus {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        TelemetryStatus {
            otlp_endpoint_configured: self.config.otel_endpoint != DEFAULT_OTEL_ENDPOINT,
            traces_enabled: self.config.traces_enabled,
            metrics_enabled: self.config.metrics_enabled,
            trace_exporter_initialized: state.trace_initialized,
            metrics_exporter_initialized: state.metrics_initialized,
            export_timeout_ms: duration_millis(self.config.export_timeout),
            service_environment: self.config.service_environment.clone(),
            last_error: state.last_error.clone(),
            agent_protocol: self.metrics.snapshot(),
        }
    }

    /// Flushes telemetry before shutdown when SDK providers are installed.
    pub fn shutdown(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(provider) = state.trace_provider.take() {
            if let Err(error) = provider.shutdown_with_timeout(self.config.export_timeout) {
                state.push_error(format!("trace shutdown: {error}"));
            }
            state.trace_initialized = false;
        }
        if let Some(provider) = state.metrics_provider.take() {
            if let Err(error) = provider.shutdown_with_timeout(self.config.export_timeout) {
                state.push_error(format!("metrics shutdown: {error}"));
            }
            state.metrics_initialized = false;
        }
    }

    fn try_initialize(&self) -> InitializedTelemetry {
        let resource = Resource::builder()
            .with_service_name(PROJECT_NAME.to_owned())
            .with_attribute(KeyValue::new(
                "deployment.environment",
                self.config.service_environment.clone(),
            ))
            .build();
        let mut initialized = InitializedTelemetry::default();

        if self.config.metrics_enabled {
            match opentelemetry_otlp::MetricExporter::builder()
                .with_http()
                .with_endpoint(self.config.metric_endpoint())
                .with_timeout(self.config.export_timeout)
                .build()
            {
                Ok(exporter) => {
                    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
                        .with_interval(Duration::from_secs(5))
                        .build();
                    let provider = SdkMeterProvider::builder()
                        .with_resource(resource.clone())
                        .with_reader(reader)
                        .build();
                    global::set_meter_provider(provider.clone());
                    initialized.metrics_provider = Some(provider);
                    initialized.metrics_initialized = true;
                }
                Err(error) => initialized.push_error(format!("metrics exporter: {error}")),
            }
        }

        if self.config.traces_enabled {
            match opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(self.config.trace_endpoint())
                .with_timeout(self.config.export_timeout)
                .build()
            {
                Ok(exporter) => {
                    let provider = SdkTracerProvider::builder()
                        .with_resource(resource)
                        .with_batch_exporter(exporter)
                        .build();
                    let tracer = provider.tracer(PROJECT_NAME.to_owned());
                    global::set_tracer_provider(provider.clone());
                    match install_otel_subscriber(tracer) {
                        Ok(()) => {
                            initialized.trace_provider = Some(provider);
                            initialized.trace_initialized = true;
                        }
                        Err(error) => initialized.push_error(format!("trace subscriber: {error}")),
                    }
                }
                Err(error) => {
                    initialized.push_error(format!("trace exporter: {error}"));
                    install_fallback_subscriber(&mut initialized);
                }
            }
        } else {
            install_fallback_subscriber(&mut initialized);
        }

        initialized
    }
}

#[derive(Default)]
struct InitializedTelemetry {
    trace_initialized: bool,
    metrics_initialized: bool,
    trace_provider: Option<SdkTracerProvider>,
    metrics_provider: Option<SdkMeterProvider>,
    last_error: Option<String>,
}

impl InitializedTelemetry {
    fn push_error(&mut self, error: String) {
        match &mut self.last_error {
            Some(existing) => {
                existing.push_str("; ");
                existing.push_str(&error);
            }
            None => self.last_error = Some(error),
        }
    }
}

impl ObservabilityState {
    fn push_error(&mut self, error: String) {
        match &mut self.last_error {
            Some(existing) => {
                existing.push_str("; ");
                existing.push_str(&error);
            }
            None => self.last_error = Some(error),
        }
    }
}

fn install_otel_subscriber(
    tracer: opentelemetry_sdk::trace::SdkTracer,
) -> Result<(), tracing_subscriber::util::TryInitError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .try_init()
}

fn install_fallback_subscriber(initialized: &mut InitializedTelemetry) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if let Err(error) = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .try_init()
    {
        initialized.push_error(format!("fallback subscriber: {error}"));
    }
}

/// Stable telemetry diagnostics exposed through service status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryStatus {
    pub otlp_endpoint_configured: bool,
    pub traces_enabled: bool,
    pub metrics_enabled: bool,
    pub trace_exporter_initialized: bool,
    pub metrics_exporter_initialized: bool,
    pub export_timeout_ms: u64,
    pub service_environment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub agent_protocol: AgentProtocolMetricsSnapshot,
}

/// Low-cardinality agent protocol metric recorder.
#[derive(Debug, Clone, Default)]
pub struct AgentProtocolMetrics {
    inner: Arc<Mutex<AgentProtocolMetricsSnapshot>>,
}

impl AgentProtocolMetrics {
    /// Records a completed or failed protocol operation.
    pub fn record_request(
        &self,
        protocol: &str,
        operation: &str,
        status: &str,
        duration_ms: u64,
        truncated: bool,
    ) {
        {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.requests_total = inner.requests_total.saturating_add(1);
            inner.request_duration_ms_total =
                inner.request_duration_ms_total.saturating_add(duration_ms);
            if truncated {
                inner.context_truncated_total = inner.context_truncated_total.saturating_add(1);
            }
        }

        let meter = global::meter(PROJECT_NAME);
        meter
            .u64_counter("relay_agent_protocol_requests_total")
            .build()
            .add(
                1,
                &[
                    KeyValue::new("protocol", protocol.to_owned()),
                    KeyValue::new("operation", operation.to_owned()),
                    KeyValue::new("status", status.to_owned()),
                ],
            );
        meter
            .u64_histogram("relay_agent_protocol_request_duration_ms")
            .build()
            .record(
                duration_ms,
                &[
                    KeyValue::new("protocol", protocol.to_owned()),
                    KeyValue::new("operation", operation.to_owned()),
                ],
            );
        if truncated {
            meter
                .u64_counter("relay_agent_context_truncated_total")
                .build()
                .add(
                    1,
                    &[
                        KeyValue::new("protocol", protocol.to_owned()),
                        KeyValue::new("reason", "budget".to_owned()),
                    ],
                );
        }
    }

    /// Records admission or protocol rejection before service execution.
    pub fn record_rejection(&self, protocol: &str, reason: &str) {
        {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.rejections_total = inner.rejections_total.saturating_add(1);
        }
        global::meter(PROJECT_NAME)
            .u64_counter("relay_agent_protocol_rejections_total")
            .build()
            .add(
                1,
                &[
                    KeyValue::new("protocol", protocol.to_owned()),
                    KeyValue::new("reason", reason.to_owned()),
                ],
            );
    }

    /// Records cancellation.
    pub fn record_cancelled(&self, protocol: &str) {
        {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.cancelled_total = inner.cancelled_total.saturating_add(1);
        }
        global::meter(PROJECT_NAME)
            .u64_counter("relay_agent_retrieval_cancelled_total")
            .build()
            .add(1, &[KeyValue::new("protocol", protocol.to_owned())]);
    }

    /// Records the first initialize-to-tools/list discovery latency for a session.
    pub fn record_cold_start(&self, protocol: &str, duration_ms: u64) {
        {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.cold_start_total = inner.cold_start_total.saturating_add(1);
            inner.cold_start_duration_ms_total = inner
                .cold_start_duration_ms_total
                .saturating_add(duration_ms);
        }
        global::meter(PROJECT_NAME)
            .u64_histogram("relay_agent_protocol_cold_start_duration_ms")
            .build()
            .record(
                duration_ms,
                &[KeyValue::new("protocol", protocol.to_owned())],
            );
    }

    /// Returns an in-process metric snapshot for diagnostics and tests.
    pub fn snapshot(&self) -> AgentProtocolMetricsSnapshot {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// In-process agent protocol metric snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProtocolMetricsSnapshot {
    pub requests_total: u64,
    pub request_duration_ms_total: u64,
    pub rejections_total: u64,
    pub cancelled_total: u64,
    pub context_truncated_total: u64,
    #[serde(default)]
    pub cold_start_total: u64,
    #[serde(default)]
    pub cold_start_duration_ms_total: u64,
}

fn signal_endpoint(base: &str, path: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix(OTLP_TRACE_PATH) {
        format!("{prefix}{path}")
    } else if let Some(prefix) = trimmed.strip_suffix(OTLP_METRIC_PATH) {
        format!("{prefix}{path}")
    } else {
        format!("{trimmed}{path}")
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
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
}
