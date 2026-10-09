//! Research artifact CLI adapters with explicit repository and scope authority.
use super::{CliAction, CliError, OutputFormat, command::value_after};
use crate::{
    api::{ApiMetadata, RequestContext},
    application::{RelayKnowledgeService, research::ResearchService},
    domain::GraphVersion,
};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchCommand {
    SourcesAudit {
        root: PathBuf,
        input: PathBuf,
    },
    Status {
        root: PathBuf,
        delivery: crate::domain::research::ResearchDelivery,
        catalog: Option<PathBuf>,
        bundle: Option<PathBuf>,
        scope: Option<String>,
        requirements: Option<PathBuf>,
    },
    EvidenceValidate {
        root: PathBuf,
        input: PathBuf,
        scope: String,
    },
    EvidenceImport {
        root: PathBuf,
        input: PathBuf,
        scope: String,
    },
    EvidenceExport {
        id: String,
        scope: String,
        revision: String,
    },
    EvidenceView {
        root: PathBuf,
        input: PathBuf,
        scope: String,
        focus: Option<String>,
    },
    EvidenceImpact {
        root: PathBuf,
        input: PathBuf,
        scope: String,
        node: String,
        label: String,
    },
}

impl ResearchCommand {
    pub(super) fn needs_runtime(&self) -> bool {
        matches!(
            self,
            Self::EvidenceImport { .. } | Self::EvidenceExport { .. } | Self::Status { .. }
        )
    }
}

pub(super) fn parse(namespace: &str, tokens: &[String]) -> Result<CliAction, CliError> {
    let action = tokens.first().map(String::as_str).unwrap_or("");
    let allowed: &[&str] = match (namespace, action) {
        ("research", "status") => &[
            "--root",
            "--delivery",
            "--catalog",
            "--bundle",
            "--scope",
            "--requirements",
        ],
        ("sources", "audit") => &["--root", "--input"],
        ("evidence", "validate" | "import") => &["--root", "--input", "--scope"],
        ("evidence", "view") => &["--root", "--input", "--scope", "--focus"],
        ("evidence", "export") => &["--id", "--scope", "--revision"],
        ("evidence", "impact") => &["--root", "--input", "--scope", "--node", "--label"],
        _ => {
            return Err(CliError::UnexpectedArgument(format!(
                "{namespace} {action}"
            )));
        }
    };
    let mut values = BTreeMap::new();
    let mut index = 1;
    while index < tokens.len() {
        if !allowed.contains(&tokens[index].as_str()) || values.contains_key(&tokens[index]) {
            return Err(CliError::UnexpectedArgument(tokens[index].clone()));
        }
        values.insert(
            tokens[index].clone(),
            value_after(tokens, index, "research option")?,
        );
        index += 2;
    }
    let required = |flag: &str| {
        values
            .get(flag)
            .cloned()
            .ok_or_else(|| CliError::UnexpectedArgument(format!("missing {flag}")))
    };
    let command = match (namespace, action) {
        ("research", "status") => ResearchCommand::Status {
            root: required("--root")?.into(),
            delivery: match required("--delivery")?.as_str() {
                "archive" => crate::domain::research::ResearchDelivery::Archive,
                "authored_graph" => crate::domain::research::ResearchDelivery::AuthoredGraph,
                "graphrag" => crate::domain::research::ResearchDelivery::Graphrag,
                other => return Err(CliError::UnexpectedArgument(other.into())),
            },
            catalog: values.get("--catalog").map(PathBuf::from),
            bundle: values.get("--bundle").map(PathBuf::from),
            scope: values.get("--scope").cloned(),
            requirements: values.get("--requirements").map(PathBuf::from),
        },
        ("sources", "audit") => ResearchCommand::SourcesAudit {
            root: required("--root")?.into(),
            input: required("--input")?.into(),
        },
        ("evidence", "validate") => ResearchCommand::EvidenceValidate {
            root: required("--root")?.into(),
            input: required("--input")?.into(),
            scope: required("--scope")?,
        },
        ("evidence", "import") => ResearchCommand::EvidenceImport {
            root: required("--root")?.into(),
            input: required("--input")?.into(),
            scope: required("--scope")?,
        },
        ("evidence", "export") => ResearchCommand::EvidenceExport {
            id: required("--id")?,
            scope: required("--scope")?,
            revision: required("--revision")?,
        },
        ("evidence", "view") => ResearchCommand::EvidenceView {
            root: required("--root")?.into(),
            input: required("--input")?.into(),
            scope: required("--scope")?,
            focus: values.get("--focus").cloned(),
        },
        ("evidence", "impact") => ResearchCommand::EvidenceImpact {
            root: required("--root")?.into(),
            input: required("--input")?.into(),
            scope: required("--scope")?,
            node: required("--node")?,
            label: required("--label")?,
        },
        _ => unreachable!("validated command shape"),
    };
    Ok(CliAction::Research(command))
}

pub(crate) async fn run(
    command: ResearchCommand,
    service: Option<&RelayKnowledgeService>,
    context: RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    let runtime = || {
        service.ok_or_else(|| {
            CliError::invalid_api_argument("research command requires graph runtime", format)
        })
    };
    let api_error = |error| CliError::api_failed(error, format);
    let (operation, response) = match command {
        ResearchCommand::Status {
            root,
            delivery,
            catalog,
            bundle,
            scope,
            requirements,
        } => (
            "research.status",
            serde_json::to_value(
                runtime()?
                    .research_status(
                        crate::application::research::ResearchStatusRequest {
                            root,
                            delivery,
                            catalog,
                            bundle,
                            scope,
                            requirements,
                        },
                        context.clone(),
                    )
                    .await
                    .map_err(api_error)?,
            ),
        ),
        ResearchCommand::SourcesAudit { root, input } => (
            "sources.audit",
            serde_json::to_value(
                ResearchService::new(root)
                    .audit_sources(input)
                    .await
                    .map_err(api_error)?,
            ),
        ),
        ResearchCommand::EvidenceValidate { root, input, scope } => (
            "evidence.validate",
            serde_json::to_value(
                ResearchService::new(root)
                    .validate_bundle(input, scope)
                    .await
                    .map_err(api_error)?,
            ),
        ),
        ResearchCommand::EvidenceImport { root, input, scope } => (
            "evidence.import",
            serde_json::to_value(
                runtime()?
                    .import_evidence_bundle(root, input, scope, context.clone())
                    .await
                    .map_err(api_error)?,
            ),
        ),
        ResearchCommand::EvidenceExport {
            id,
            scope,
            revision,
        } => (
            "evidence.export",
            serde_json::to_value(
                runtime()?
                    .export_evidence_bundle(id, scope, revision)
                    .await
                    .map_err(api_error)?,
            ),
        ),
        ResearchCommand::EvidenceView {
            root,
            input,
            scope,
            focus,
        } => (
            "evidence.view",
            serde_json::to_value(
                ResearchService::new(root)
                    .bundle_view(input, scope, focus)
                    .await
                    .map_err(api_error)?,
            ),
        ),
        ResearchCommand::EvidenceImpact {
            root,
            input,
            scope,
            node,
            label,
        } => (
            "evidence.impact",
            serde_json::to_value(
                ResearchService::new(root)
                    .revise_bundle(input, scope, node, label)
                    .await
                    .map_err(api_error)?,
            ),
        ),
    };
    let response =
        response.map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
    super::render_response(
        operation,
        ApiMetadata::graph_only(
            &context,
            GraphVersion::new(
                response
                    .get("graph_version")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            ),
        ),
        &response,
        format,
    )
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
