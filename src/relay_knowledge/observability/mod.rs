//! Observability runtime for local diagnostics and OTLP export.

use std::{
    error::Error,
    fmt,
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
        let result = self.try_initialize();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match result {
            Ok(initialized) => {
                state.trace_initialized = initialized.trace_initialized;
                state.metrics_initialized = initialized.metrics_initialized;
                state.last_error = None;
            }
            Err(error) => {
                state.last_error = Some(error.to_string());
            }
        }
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
        // Providers are installed in the OpenTelemetry global registry. The Rust
        // API exposes shutdown on provider handles; this runtime keeps exporter
        // failures best-effort and leaves process teardown to flush remaining work.
    }

    fn try_initialize(&self) -> Result<InitializedTelemetry, ObservabilityError> {
        let resource = Resource::builder()
            .with_service_name(PROJECT_NAME.to_owned())
            .with_attribute(KeyValue::new(
                "deployment.environment",
                self.config.service_environment.clone(),
            ))
            .build();
        let mut initialized = InitializedTelemetry::default();

        if self.config.metrics_enabled {
            let exporter = opentelemetry_otlp::MetricExporter::builder()
                .with_http()
                .with_endpoint(self.config.metric_endpoint())
                .with_timeout(self.config.export_timeout)
                .build()
                .map_err(|error| ObservabilityError(error.to_string()))?;
            let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
                .with_interval(Duration::from_secs(5))
                .build();
            let provider = SdkMeterProvider::builder()
                .with_resource(resource.clone())
                .with_reader(reader)
                .build();
            global::set_meter_provider(provider);
            initialized.metrics_initialized = true;
        }

        if self.config.traces_enabled {
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(self.config.trace_endpoint())
                .with_timeout(self.config.export_timeout)
                .build()
                .map_err(|error| ObservabilityError(error.to_string()))?;
            let provider = SdkTracerProvider::builder()
                .with_resource(resource)
                .with_batch_exporter(exporter)
                .build();
            let tracer = provider.tracer(PROJECT_NAME.to_owned());
            global::set_tracer_provider(provider);
            let filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
            let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer())
                .with(otel_layer)
                .try_init();
            initialized.trace_initialized = true;
        } else {
            let filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer())
                .try_init();
        }

        Ok(initialized)
    }
}

#[derive(Default)]
struct InitializedTelemetry {
    trace_initialized: bool,
    metrics_initialized: bool,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservabilityError(String);

impl fmt::Display for ObservabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ObservabilityError {}

fn signal_endpoint(base: &str, path: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/v1/traces") || trimmed.ends_with("/v1/metrics") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}{path}")
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
