use crate::domain::FreshnessPolicy;

use super::super::CliError;

#[cfg(test)]
#[path = "values_tests.rs"]
mod tests;

pub(in crate::interfaces::cli) fn value_after(
    tokens: &[String],
    index: usize,
    flag: &'static str,
) -> Result<String, CliError> {
    tokens
        .get(index + 1)
        .cloned()
        .ok_or(CliError::MissingValue(flag))
}

pub(in crate::interfaces::cli) fn parse_freshness(
    value: &str,
) -> Result<FreshnessPolicy, CliError> {
    match value {
        "allow-stale" => Ok(FreshnessPolicy::AllowStale),
        "wait-until-fresh" => Ok(FreshnessPolicy::WaitUntilFresh),
        "graph-only" => Ok(FreshnessPolicy::GraphOnly),
        other => Err(CliError::InvalidFreshness(other.to_owned())),
    }
}
