use crate::api::CodeRepositoryReportResponse;

use super::{CliError, OutputFormat, render_response};

pub(crate) fn render_report_response(
    response: &CodeRepositoryReportResponse,
    format: OutputFormat,
) -> Result<String, CliError> {
    if format == OutputFormat::Markdown {
        return render_markdown_report(response);
    }

    render_response(
        "code.repo.report",
        response.metadata.clone(),
        response,
        format,
    )
}

fn render_markdown_report(response: &CodeRepositoryReportResponse) -> Result<String, CliError> {
    let report = &response.report;
    let mut output = String::new();
    output.push_str(&format!("# Code Repository Report: {}\n\n", report.alias));
    output.push_str(&format!("- Repository id: `{}`\n", report.repository_id));
    output.push_str(&format!("- Root: `{}`\n", report.root_path));
    output.push_str(&format!(
        "- Indexed commit: `{}`\n",
        report.resolved_commit_sha.as_deref().unwrap_or("unindexed")
    ));
    output.push_str(&format!(
        "- Tree hash: `{}`\n",
        report.tree_hash.as_deref().unwrap_or("unindexed")
    ));
    output.push_str(&format!(
        "- Totals: files={}, symbols={}, references={}, chunks={}, degraded={}\n",
        report.indexed_file_count,
        report.symbol_count,
        report.reference_count,
        report.chunk_count,
        report.degraded_file_count
    ));
    output.push_str(&format!(
        "- Edge resolution: resolved={}, ambiguous={}, unresolved={}\n",
        report.resolved_edge_count, report.ambiguous_edge_count, report.unresolved_edge_count
    ));
    output.push_str(&format!("- Freshness: `{}`\n\n", report.freshness_state));
    output.push_str("## Scope\n\n");
    output.push_str(&format!(
        "- Paths: `{}`\n",
        if report.path_filters.is_empty() {
            ".".to_owned()
        } else {
            report.path_filters.join(", ")
        }
    ));
    output.push_str(&format!(
        "- Languages: `{}`\n\n",
        if report.language_filters.is_empty() {
            "all".to_owned()
        } else {
            report.language_filters.join(", ")
        }
    ));
    output.push_str("## Representative Queries\n\n");
    for query in &report.representative_queries {
        output.push_str(&format!("- `{query}`\n"));
    }
    output.push_str("\n## Latency Samples\n\n");
    for sample in &report.latency_samples {
        output.push_str(&format!(
            "- `{}` {:?}: {} result(s), {} ms\n",
            sample.query, sample.kind, sample.result_count, sample.duration_ms
        ));
    }
    if !report.degradation_summary.is_empty() {
        output.push_str("\n## Degradation\n\n");
        for item in &report.degradation_summary {
            output.push_str(&format!("- {item}\n"));
        }
    }

    Ok(output)
}
