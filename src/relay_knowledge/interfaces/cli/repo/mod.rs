use crate::domain::{CodeQueryKind, CodeRepositorySelector, FreshnessPolicy, SoftwareGlobalKind};

use super::{
    CliError, OutputFormat,
    command::{parse_freshness, value_after},
    render_response, serialize_line,
};

mod index;
mod parser;
mod query;
mod report;
mod runner;
pub(crate) mod view;

pub use parser::parse_repo;
pub(super) use report::render_report_response;
pub use runner::run_repo;

/// Parsed `repo` CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoCommand {
    List,
    Register {
        root_path: String,
        alias: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    },
    Remove {
        alias: String,
    },
    Index {
        alias: String,
        ref_selector: String,
        dry_run: bool,
    },
    IndexReset {
        alias: String,
    },
    IndexWorker {
        task_id: Option<String>,
    },
    ScopePreview {
        alias: String,
        ref_selector: String,
    },
    Update {
        alias: String,
        base_ref: String,
        head_ref: String,
    },
    Query {
        alias: String,
        query: String,
        kind: CodeQueryKind,
        limit: usize,
        ref_selector: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        freshness: FreshnessPolicy,
        exclude_generated: bool,
    },
    Context {
        alias: String,
        query: String,
        limit: usize,
        ref_selector: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        freshness: FreshnessPolicy,
        max_context_bytes: usize,
        include_code: bool,
        exclude_generated: bool,
    },
    FeatureFlags {
        alias: String,
        query: Option<String>,
        limit: usize,
        ref_selector: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        freshness: FreshnessPolicy,
    },
    Impact {
        alias: String,
        base_ref: String,
        head_ref: String,
        limit: usize,
    },
    Status {
        alias: String,
    },
    Report {
        alias: String,
    },
    Software {
        alias: String,
        ref_selector: String,
        kind: SoftwareGlobalKind,
        freshness: FreshnessPolicy,
        limit: usize,
    },
    View(view::RepoViewCommand),
}

pub(crate) fn selector(
    alias: String,
    ref_selector: impl Into<String>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    format: OutputFormat,
) -> Result<CodeRepositorySelector, CliError> {
    CodeRepositorySelector::new(alias, ref_selector, path_filters, language_filters)
        .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))
}

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;
