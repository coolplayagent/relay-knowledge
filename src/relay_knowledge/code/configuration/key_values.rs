use super::{
    languages::{json, properties, yaml},
    model::ConfigFact,
    source::{config_key_prefix, push_definition, source_lines, valid_config_key},
};

pub(super) fn facts(language_id: &str, content: &str, definitions: &mut Vec<ConfigFact>) {
    let mut yaml_block = yaml::BlockScalarTracker::default();
    let mut properties_continuation = false;
    for line in source_lines(content) {
        let trimmed = line.text.trim();
        if language_id == "yaml" && yaml_block.should_skip(line.text, trimmed) {
            continue;
        }
        if language_id == "properties"
            && properties::skip_continued_value_line(
                line.text,
                trimmed,
                &mut properties_continuation,
            )
        {
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('!')
            || trimmed.starts_with("//")
        {
            continue;
        }
        if language_id == "json" {
            for key in json::object_keys(trimmed)
                .into_iter()
                .filter(|key| valid_config_key(key))
            {
                push_definition(definitions, key, "config_key", line.range());
            }
            continue;
        }
        if language_id == "toml"
            && let Some(section) = toml_section_name(trimmed)
        {
            push_definition(definitions, section, "section", line.range());
            continue;
        }
        if language_id == "ini" && trimmed.starts_with('[') && trimmed.ends_with(']') {
            push_definition(
                definitions,
                &trimmed[1..trimmed.len() - 1],
                "section",
                line.range(),
            );
            continue;
        }
        let key = if language_id == "yaml" {
            yaml::mapping_key(trimmed).map(config_key_prefix)
        } else {
            trimmed
                .split_once('=')
                .or_else(|| trimmed.split_once(':'))
                .map(|(key, _)| config_key_prefix(key))
                .or_else(|| properties_space_key(language_id, trimmed))
        };
        if let Some(key) = key.filter(|key| valid_config_key(key)) {
            push_definition(definitions, key, "config_key", line.range());
        }
    }
}

fn toml_section_name(value: &str) -> Option<&str> {
    value
        .strip_prefix("[[")
        .and_then(|section| section.strip_suffix("]]"))
        .or_else(|| {
            value
                .strip_prefix('[')
                .and_then(|section| section.strip_suffix(']'))
        })
        .map(str::trim)
        .filter(|section| valid_config_key(section))
}

fn properties_space_key<'a>(language_id: &str, value: &'a str) -> Option<&'a str> {
    (language_id == "properties")
        .then(|| value.split_whitespace().next())
        .flatten()
}
