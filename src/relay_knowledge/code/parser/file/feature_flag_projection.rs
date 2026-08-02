//! Feature-flag projection from source text and structured configuration facts.

use crate::code::{
    CodeIndexError, SnapshotBuild, config_files,
    feature_flags::{FeatureFlagFileInput, extract_feature_flags},
};

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
    build.feature_flags.extend(records);

    Ok(())
}

#[cfg(test)]
#[path = "feature_flag_projection_tests.rs"]
mod tests;
