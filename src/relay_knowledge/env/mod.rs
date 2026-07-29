//! Environment variable boundary for runtime configuration.
//!
//! This module is the only production boundary that reads process environment
//! variables. It normalizes platform directory inputs and relay-specific
//! overrides into typed structures before application, path, or network code
//! consumes them.

mod config;
mod error;
mod overrides;
mod platform;
mod value_parser;
mod variables;

pub use config::{EnvironmentConfig, RemoteCliEnvironmentConfig};
pub use error::{EnvError, EnvErrorKind};
pub use overrides::{
    AgentEnvOverrides, FileIndexEnvOverrides, NetworkEnvOverrides, PathEnvOverrides,
    RemoteCliEnvOverrides, RetrievalEnvOverrides, TelemetryEnvOverrides, UpdateEnvOverrides,
    WatcherEnvOverrides, WorkerEnvOverrides,
};
pub use platform::{PlatformEnvironment, PlatformKind};
pub use variables::*;

pub(crate) use platform::windows_system_root_from_process;
