use serde_json::Value;

use crate::domain::{
    CodeIndexMode, CodeIndexRequest, CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig,
    FreshnessPolicy,
};

use super::{WebError, code_selector};

pub(in crate::interfaces) fn code_index_request(
    payload: &Value,
    mode: CodeIndexMode,
) -> Result<CodeIndexRequest, WebError> {
    Ok(CodeIndexRequest {
        repository: code_selector(payload)?,
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
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::{CodeRepositorySelector, CodeWorkspaceDetectionConfig};

    fn payload() -> Value {
        json!({
            "alias": "relay",
            "ref": "main",
            "path_filters": [],
            "language_filters": []
        })
    }

    #[test]
    fn defaults_workspace_detection_when_absent() {
        let request = code_index_request(&payload(), CodeIndexMode::Full).expect("request");
        assert_eq!(
            request.workspace_detection,
            CodeWorkspaceDetectionConfig::default()
        );
    }

    #[test]
    fn parses_workspace_detection_config() {
        let mut payload = payload();
        payload["workspace_detection"] = json!({
            "enabled": true,
            "supported_formats": ["pnpm", "go_modules"]
        });

        let request = code_index_request(&payload, CodeIndexMode::Full).expect("request");

        assert!(request.workspace_detection.enabled);
        assert_eq!(
            request.workspace_detection.supported_formats,
            vec![
                CodeMonorepoWorkspaceFormat::Pnpm,
                CodeMonorepoWorkspaceFormat::GoModules,
            ]
        );
        assert_eq!(
            request.repository,
            CodeRepositorySelector::new("relay", "main", Vec::new(), Vec::new()).expect("selector")
        );
    }

    #[test]
    fn rejects_unsupported_workspace_detection_format() {
        let mut payload = payload();
        payload["workspace_detection"] = json!({
            "enabled": true,
            "supported_formats": ["unknown"]
        });

        let error = code_index_request(&payload, CodeIndexMode::Full).expect_err("error");
        assert!(
            error
                .message
                .contains("unsupported workspace_detection.supported_formats")
        );
    }
}
