use std::{fs, path::Path};

use serde_json::Value;

use super::merge::merge_case_config;

pub fn load_cases(path: &Path) -> Result<Value, String> {
    let config = load_cases_and_includes(path)?;
    validate_repository_set_isolation(&config)?;
    Ok(config)
}

fn load_cases_and_includes(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut config = serde_json::from_str::<Value>(&text)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    let include_files = config
        .as_object_mut()
        .and_then(|object| object.remove("include_files"))
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    for include_file in include_files {
        let relative = include_file
            .as_str()
            .ok_or_else(|| format!("invalid include file entry in {}", path.display()))?;
        let parent = path.parent().unwrap_or(Path::new("."));
        let included = load_cases_and_includes(&parent.join(relative))?;
        merge_case_config(&mut config, included)?;
    }
    Ok(config)
}

fn validate_repository_set_isolation(config: &Value) -> Result<(), String> {
    let Some(repositories) = config.get("repositories").and_then(Value::as_object) else {
        return Ok(());
    };
    let Some(repository_sets) = config.get("repository_sets").and_then(Value::as_object) else {
        return Ok(());
    };
    for (set_name, set_config) in repository_sets {
        let members = set_config
            .get("members")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for member in members {
            let Some(repository_name) = member.get("repository").and_then(Value::as_str) else {
                continue;
            };
            if repositories
                .get(repository_name)
                .and_then(|repository| repository.get("isolated_index_home"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                return Err(format!(
                    "repository set {set_name:?} member {repository_name:?} cannot set isolated_index_home=true; repository-set members must share one evaluation home"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "loading_tests.rs"]
mod loading_tests;
