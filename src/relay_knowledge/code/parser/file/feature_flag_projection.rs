//! Feature-flag projection from source text and structured configuration facts.

use crate::code::{
    CodeIndexError, SnapshotBuild, config_files,
    feature_flags::{FeatureFlagFileInput, extract_feature_flags},
};

pub(super) fn record_syntax_flags(
    build: &mut SnapshotBuild,
    input: &super::SyntaxFileInput<'_>,
    root: tree_sitter::Node<'_>,
    file_flags_start: usize,
) -> Result<(), CodeIndexError> {
    let flag_input = FeatureFlagFileInput {
        repository_id: &build.repository_id,
        source_scope: &build.source_scope,
        file_id: input.file_id,
        path: input.path,
        language_id: input.language.id,
        content: input.content,
        config_facts: &[],
    };
    let records = match input.language.id {
        "java" => crate::code::feature_flags::java::extract(&flag_input, root),
        "bash" => crate::code::feature_flags::shell::extract(&flag_input, root),
        _ => return Ok(()),
    }
    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
    // Replace only this file's coarse line facts that have structured evidence.
    // AST identities include byte ranges so distinct calls on one line survive.
    let covered = records
        .iter()
        .map(|record| {
            (
                record.source_kind.clone(),
                record.source_key.clone(),
                record.edge_kind.clone(),
                record.line_range.start,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let lexical = build
        .feature_flags
        .split_off(file_flags_start)
        .into_iter()
        .filter(|record| {
            !covered.contains(&(
                record.source_kind.clone(),
                record.source_key.clone(),
                record.edge_kind.clone(),
                record.line_range.start,
            ))
        });
    let records = lexical
        .chain(records)
        .map(|record| (record.usage_id.clone(), record))
        .collect::<std::collections::BTreeMap<_, _>>();
    build.feature_flags.extend(records.into_values());
    Ok(())
}

pub(super) fn record_feature_flags(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    config_facts: Option<&[config_files::ConfigFact]>,
) -> Result<(), CodeIndexError> {
    let owned_config_facts;
    let config_facts = match config_facts {
        Some(config_facts) => config_facts,
        None => {
            owned_config_facts = config_files::structured_facts(path, language_id, content).0;
            &owned_config_facts
        }
    };
    let records = extract_feature_flags(FeatureFlagFileInput {
        repository_id: &build.repository_id,
        source_scope: &build.source_scope,
        file_id,
        path,
        language_id,
        content,
        config_facts,
    })
    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
    // Every Java path, including text-only/failed syntax fallback, requires AST
    // receiver proof for environment facts. Other lexical SDK facts remain intact.
    build.feature_flags.extend(
        records
            .into_iter()
            .filter(|record| !(language_id == "java" && record.source_kind == "env_var")),
    );

    Ok(())
}

#[cfg(test)]
#[path = "feature_flag_projection_tests.rs"]
mod tests;
