//! Explicit software feedback commands; inputs are data, never executable scripts.

use serde::de::DeserializeOwned;
use tokio::io::AsyncReadExt;

use crate::{
    api::{ApiMetadata, RequestContext},
    application::{RelayKnowledgeService, feedback::public_record},
    domain::{GraphVersion, feedback::*},
};

use super::super::{CliAction, CliError, OutputFormat, render_response};

/// Local feedback control plane. Publication always obeys persisted policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackCommand {
    Configure { input: String },
    Report { input: String },
    Status { id: Option<String> },
    Preview { id: String },
    Submit { id: String },
    Retry { id: String },
    Track { id: String },
    LinkFix { id: String, input: String },
    Validate { id: String, input: String },
}

pub(crate) fn parse(tokens: &[String]) -> Result<CliAction, CliError> {
    let values = tokens.iter().map(String::as_str).collect::<Vec<_>>();
    let action = match values.as_slice() {
        ["configure", "--input", input] => FeedbackCommand::Configure {
            input: (*input).into(),
        },
        ["report", "--input", input] => FeedbackCommand::Report {
            input: (*input).into(),
        },
        ["status"] => FeedbackCommand::Status { id: None },
        ["status", id] => FeedbackCommand::Status {
            id: Some((*id).into()),
        },
        ["preview", id] => FeedbackCommand::Preview { id: (*id).into() },
        ["submit", id] => FeedbackCommand::Submit { id: (*id).into() },
        ["retry", id] => FeedbackCommand::Retry { id: (*id).into() },
        ["track", id] => FeedbackCommand::Track { id: (*id).into() },
        ["link-fix", id, "--input", input] => FeedbackCommand::LinkFix {
            id: (*id).into(),
            input: (*input).into(),
        },
        ["validate", id, "--input", input] => FeedbackCommand::Validate {
            id: (*id).into(),
            input: (*input).into(),
        },
        _ => return Err(CliError::UnexpectedArgument(tokens.join(" "))),
    };
    Ok(CliAction::Feedback(action))
}

pub(crate) async fn run(
    service: &RelayKnowledgeService,
    command: FeedbackCommand,
    context: RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    let feedback = service.feedback_service().map_err(CliError::ApiFailed)?;
    let result = match command {
        FeedbackCommand::Configure { input } => {
            let policy = read_input::<FeedbackPolicy>(&input, format).await?;
            feedback
                .configure(policy)
                .await
                .map(|policy| serde_json::json!({"policy":policy}))
        }
        FeedbackCommand::Report { input } => {
            let report = read_input::<FeedbackReport>(&input, format).await?;
            feedback
                .report(report, &context)
                .await
                .map(|record| serde_json::json!({"feedback":public_record(&record)}))
        }
        FeedbackCommand::Status { id } => feedback.status(id.as_deref()).await,
        FeedbackCommand::Preview { id } => feedback.preview(&id).await,
        FeedbackCommand::Submit { id } | FeedbackCommand::Retry { id } => feedback
            .submit(&id)
            .await
            .map(|record| serde_json::json!({"feedback":public_record(&record)})),
        FeedbackCommand::Track { id } => feedback
            .track(&id)
            .await
            .map(|record| serde_json::json!({"feedback":public_record(&record)})),
        FeedbackCommand::LinkFix { id, input } => {
            let fix = read_input::<FeedbackFix>(&input, format).await?;
            feedback
                .link_fix(&id, fix)
                .await
                .map(|record| serde_json::json!({"feedback":public_record(&record)}))
        }
        FeedbackCommand::Validate { id, input } => {
            let validation = read_input::<FeedbackValidation>(&input, format).await?;
            feedback
                .validate(&id, validation)
                .await
                .map(|record| serde_json::json!({"feedback":public_record(&record)}))
        }
    }
    .map_err(|message| CliError::invalid_api_argument(message, format))?;
    let metadata = ApiMetadata::graph_only(&context, GraphVersion::ZERO);
    let mut response = result;
    response["metadata"] = serde_json::to_value(&metadata)
        .map_err(|error| CliError::RenderFailed(error.to_string()))?;
    render_response("feedback", metadata, &response, format)
}

async fn read_input<T: DeserializeOwned>(path: &str, format: OutputFormat) -> Result<T, CliError> {
    // Accept regular files only. Nonblocking open also closes the Unix race in
    // which a regular input is replaced by a FIFO after the metadata check.
    let read = async {
        if !tokio::fs::symlink_metadata(path).await?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "feedback input must be a regular file",
            ));
        }
        let mut options = tokio::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
        let file = options.open(path).await?;
        if !file.metadata().await?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "feedback input must be a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take(65_537).read_to_end(&mut bytes).await?;
        Ok::<_, std::io::Error>(bytes)
    };
    let bytes = tokio::time::timeout(std::time::Duration::from_secs(5), read)
        .await
        .map_err(|_| CliError::invalid_api_argument("feedback input timed out", format))?
        .map_err(|_| CliError::invalid_api_argument("feedback input could not be read", format))?;
    if bytes.len() > 65_536 {
        return Err(CliError::invalid_api_argument(
            "feedback input exceeds 64 KiB",
            format,
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        CliError::invalid_api_argument("feedback input does not match the versioned schema", format)
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
