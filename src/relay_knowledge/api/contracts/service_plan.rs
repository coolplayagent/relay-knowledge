use serde::{Deserialize, Serialize};

use crate::domain::{ServiceDefinitionPlan, ServiceManagerAction};

use super::ApiMetadata;

/// Service manager plan request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServicePlanRequest {
    pub action: ServiceManagerAction,
    pub dry_run: bool,
    pub execute: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_dir: Option<String>,
}

impl<'de> Deserialize<'de> for ServicePlanRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ServicePlanRequestWire::deserialize(deserializer)?;
        let execute = wire.execute.unwrap_or(false);
        Ok(Self {
            action: wire.action,
            dry_run: wire.dry_run.unwrap_or(!execute),
            execute,
            target_version: wire.target_version,
            install_dir: wire.install_dir,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ServicePlanRequestWire {
    action: ServiceManagerAction,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    execute: Option<bool>,
    #[serde(default)]
    target_version: Option<String>,
    #[serde(default)]
    install_dir: Option<String>,
}

/// Service manager plan response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePlanResponse {
    pub metadata: ApiMetadata,
    pub plan: ServiceDefinitionPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<crate::domain::ServiceLifecycleExecutionReport>,
}
