use serde_json::Value;

use crate::domain::{
    CodeIndexMode, CodeIndexRequest, CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig,
    FreshnessPolicy,
};

use super::super::{WebError, code_selector};
use crate::interfaces::code_index_mode::{mode_for_index_ref, selector_for_index_request};

pub(in crate::interfaces::web) fn code_index_request(
    payload: &Value,
    mode: CodeIndexMode,
) -> Result<CodeIndexRequest, WebError> {
    let repository = code_selector(payload)?;
    let mode = if mode == CodeIndexMode::Full {
        mode_for_index_ref(&repository.ref_selector)
    } else {
        mode
    };
    Ok(CodeIndexRequest {
        repository: selector_for_index_request(repository, &mode),
        mode,
        workspace_detection: workspace_detection_config(payload)?,
        freshness_policy: FreshnessPolicy::AllowStale,
    })
}

fn workspace_detection_config(payload: &Value) -> Result<CodeWorkspaceDetectionConfig, WebError> {
    let Some(value) = payload.get("workspace_detection") else {
        return Ok(CodeWorkspaceDetectionConfig::default());
    };
    if value.is_null() {
        return Ok(CodeWorkspaceDetectionConfig::default());
    }
    let Some(object) = value.as_object() else {
        return Err(WebError::bad_request(
            "workspace_detection must be an object".to_owned(),
        ));
    };

    let enabled = match object.get("enabled") {
        Some(value) => value.as_bool().ok_or_else(|| {
            WebError::bad_request("workspace_detection.enabled must be a boolean".to_owned())
        })?,
        None => false,
    };
    let supported_formats = match object.get("supported_formats") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .ok_or_else(|| {
                        WebError::bad_request(
                            "workspace_detection.supported_formats contains a non-string value"
                                .to_owned(),
                        )
                    })
                    .and_then(parse_workspace_format)
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(WebError::bad_request(
                "workspace_detection.supported_formats must be an array".to_owned(),
            ));
        }
        None => CodeWorkspaceDetectionConfig::enabled_all().supported_formats,
    };

    Ok(CodeWorkspaceDetectionConfig {
        enabled,
        supported_formats,
    })
}

fn parse_workspace_format(value: &str) -> Result<CodeMonorepoWorkspaceFormat, WebError> {
    match value {
        "pnpm" => Ok(CodeMonorepoWorkspaceFormat::Pnpm),
        "go_modules" => Ok(CodeMonorepoWorkspaceFormat::GoModules),
        "cargo_workspace" => Ok(CodeMonorepoWorkspaceFormat::CargoWorkspace),
        other => Err(WebError::bad_request(format!(
            "unsupported workspace_detection.supported_formats '{other}'"
        ))),
    }
}

#[cfg(test)]
#[path = "index_request_tests.rs"]
mod tests;
