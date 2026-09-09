//! Resolves a generated definition before measuring an exact call query.
use std::collections::BTreeSet;

use super::super::runtime::{concurrency::run_limited, contracts::EvalRuntime};
use crate::{
    cases::string_or,
    command::{CommandResult, CommandSpec},
};
use serde_json::Value;

pub(super) fn resolve_selector(result: &CommandResult) -> Result<String, String> {
    if !result.passed() {
        return Err(result.gate_message());
    }
    let payload: Value = serde_json::from_str(&result.stdout).map_err(|error| error.to_string())?;
    let hits = payload
        .get("results")
        .and_then(Value::as_array)
        .ok_or("definition results missing")?;
    let ids = hits
        .iter()
        .filter_map(|hit| hit.get("canonical_symbol_id").and_then(Value::as_str))
        .filter(|id| id.starts_with("repo://"))
        .collect::<BTreeSet<_>>();
    if ids.len() != 1 {
        return Err(format!(
            "expected one canonical definition identity, found {}",
            ids.len()
        ));
    }
    Ok(ids.into_iter().next().expect("one identity").to_owned())
}

pub(super) fn prepare(
    runtime: &EvalRuntime,
    alias: &str,
    reference: &str,
    case: &Value,
) -> (Vec<CommandResult>, Result<Value, String>) {
    let mut definition_case = case.clone();
    definition_case["kind"] = Value::String("definition".into());
    // Definition and caller paths can differ; case-level caller filters belong
    // only to the measured query, while the repository authorization remains.
    definition_case
        .as_object_mut()
        .expect("case object")
        .remove("path_filters");
    let definition = run_limited(
        &runtime.limiter,
        CommandSpec::new(
            format!("{}_definition", string_or(case, "id", "canonical_call")),
            super::cli_cases::query_command(&runtime.binary, alias, reference, &definition_case),
            &runtime.workspace,
            Some(runtime.env.clone()),
            runtime.timeout,
        ),
    );
    let selected = resolve_selector(&definition).map(|id| {
        let mut selected = case.clone();
        selected["query"] = Value::String(id);
        selected
    });
    (vec![definition], selected)
}

#[cfg(test)]
#[path = "canonical_calls_tests.rs"]
mod tests;
