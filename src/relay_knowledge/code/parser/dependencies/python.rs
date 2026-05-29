use super::{
    DependencySeed, SeedInput, parse_assignment_dependency, poetry_dependency_group,
    push_lock_seed, push_python_requirement, push_seed, pyproject_group, quoted_values,
    strip_comment, support::requirements_dependency_line, unquote,
};

pub(super) fn parse_pyproject(content: &str, records: &mut Vec<DependencySeed>) {
    let mut section = String::new();
    let mut group = "dependencies".to_owned();
    let mut poetry_group = None::<String>;
    let mut in_array = false;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.trim_matches(['[', ']']).to_owned();
            group = pyproject_group(&section);
            poetry_group = poetry_dependency_group(&section);
            in_array = false;
            continue;
        }
        if section == "project" && trimmed.starts_with("dependencies") {
            in_array = trimmed.contains('[') && !trimmed.contains(']');
            for item in quoted_values(trimmed) {
                push_python_requirement(records, item, &group, "pyproject.toml", index + 1);
            }
            continue;
        }
        if !in_array && section == "dependency-groups" && trimmed.contains('=') {
            group = trimmed
                .split_once('=')
                .map(|(left, _)| left.trim().trim_matches('"').trim_matches('\'').to_owned())
                .unwrap_or_else(|| "dependencies".to_owned());
            in_array = trimmed.contains('[') && !trimmed.contains(']');
            for item in dependency_group_requirement_values(trimmed) {
                push_python_requirement(records, item, &group, "pyproject.toml", index + 1);
            }
            continue;
        }
        if section == "tool.uv" && trimmed.starts_with("dev-dependencies") {
            group = "dev".to_owned();
            in_array = trimmed.contains('[') && !trimmed.contains(']');
            for item in quoted_values(trimmed) {
                push_python_requirement(records, item, &group, "pyproject.toml", index + 1);
            }
            continue;
        }
        if !in_array
            && section.starts_with("project.optional-dependencies")
            && trimmed.contains('=')
        {
            group = trimmed
                .split_once('=')
                .map(|(left, _)| left.trim().to_owned())
                .unwrap_or_else(|| "optional".to_owned());
            in_array = trimmed.contains('[') && !trimmed.contains(']');
            for item in quoted_values(trimmed) {
                push_python_requirement(records, item, &group, "pyproject.toml", index + 1);
            }
            continue;
        }
        if in_array {
            let items = if section == "dependency-groups" {
                dependency_group_requirement_values(trimmed)
            } else {
                quoted_values(trimmed)
            };
            for item in items {
                push_python_requirement(records, item, &group, "pyproject.toml", index + 1);
            }
            if trimmed.contains(']') {
                in_array = false;
            }
        } else if let Some(group) = poetry_group.as_deref() {
            let Some((name, requirement)) = parse_assignment_dependency(trimmed) else {
                continue;
            };
            if name != "python" {
                push_seed(
                    records,
                    SeedInput::new(
                        "python",
                        "python",
                        name,
                        requirement,
                        group,
                        "pyproject.toml",
                        false,
                    )
                    .line(index + 1)
                    .excerpt(trimmed),
                );
            }
        }
    }
}

fn dependency_group_requirement_values(value: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut start = None::<usize>;
    let mut quote = '\0';
    for (index, character) in value.char_indices() {
        if start.is_none() && matches!(character, '"' | '\'') {
            start = Some(index + character.len_utf8());
            quote = character;
        } else if start.is_some() && character == quote {
            let value_start = start.take().unwrap_or_default();
            if !dependency_group_include_value(&value[..value_start]) {
                values.push(&value[value_start..index]);
            }
        }
    }
    values
}

fn dependency_group_include_value(prefix: &str) -> bool {
    let after_delimiter = prefix
        .rsplit_once(['{', ',', '['])
        .map_or(prefix, |(_, right)| right);
    after_delimiter.contains("include-group")
}

pub(super) fn parse_uv_lock(content: &str, records: &mut Vec<DependencySeed>) {
    let mut in_package = false;
    let mut name = None::<(String, usize)>;
    let mut version = None::<String>;
    let mut local = false;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed == "[[package]]" {
            push_uv_lock_seed(records, name.take(), version.take(), local);
            in_package = true;
            local = false;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("name = ") {
            name = Some((unquote(value), index + 1));
        } else if let Some(value) = trimmed.strip_prefix("version = ") {
            version = Some(unquote(value));
        } else if uv_lock_local_source(trimmed) {
            local = true;
        }
    }
    push_uv_lock_seed(records, name, version, local);
}

fn uv_lock_local_source(value: &str) -> bool {
    value.starts_with("source =")
        && (uv_lock_source_has_key(value, "path")
            || uv_lock_source_has_key(value, "directory")
            || uv_lock_source_has_key(value, "editable")
            || uv_lock_source_has_key(value, "virtual")
            || uv_lock_source_has_key(value, "workspace"))
}

fn uv_lock_source_has_key(value: &str, key: &str) -> bool {
    value
        .split(['{', ',', '}'])
        .filter_map(|part| part.split_once('='))
        .any(|(left, _)| left.trim() == key)
}

fn push_uv_lock_seed(
    records: &mut Vec<DependencySeed>,
    name: Option<(String, usize)>,
    version: Option<String>,
    local: bool,
) {
    if local {
        return;
    }
    push_lock_seed(records, "python", "python", "uv.lock", name, version);
}

pub(super) fn parse_requirements(content: &str, records: &mut Vec<DependencySeed>) {
    for (index, line) in content.lines().enumerate() {
        let Some(requirement) = requirements_dependency_line(line) else {
            continue;
        };
        push_python_requirement(
            records,
            requirement,
            "requirements",
            "requirements.txt",
            index + 1,
        );
    }
}
