use super::{
    DependencySeed, SeedInput, parse_cargo_assignment_dependency, push_lock_seed, push_seed,
    strip_comment, support::cargo_lock_source_is_external, unquote,
};

pub(super) fn parse_cargo_toml(content: &str, records: &mut Vec<DependencySeed>) {
    let mut group = None::<String>;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            group = cargo_group(trimmed.trim_matches(['[', ']']));
            continue;
        }
        let Some(group) = group.as_deref() else {
            continue;
        };
        let Some((name, requirement)) = parse_cargo_assignment_dependency(trimmed) else {
            continue;
        };
        push_seed(
            records,
            SeedInput::new(
                "cargo",
                "rust",
                name,
                requirement,
                group,
                "Cargo.toml",
                false,
            )
            .line(index + 1)
            .excerpt(trimmed),
        );
    }
}

fn cargo_group(section: &str) -> Option<String> {
    if section == "dependencies" || section.ends_with(".dependencies") {
        Some("dependencies".to_owned())
    } else if section == "dev-dependencies" || section.ends_with(".dev-dependencies") {
        Some("dev".to_owned())
    } else if section == "build-dependencies" || section.ends_with(".build-dependencies") {
        Some("build".to_owned())
    } else {
        None
    }
}

pub(super) fn parse_cargo_lock(content: &str, records: &mut Vec<DependencySeed>) {
    let mut in_package = false;
    let mut name = None::<(String, usize)>;
    let mut version = None::<String>;
    let mut source = None::<String>;
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            push_cargo_lock_seed(records, name.take(), version.take(), source.take());
            in_package = true;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("name = ") {
            name = Some((unquote(value), index + 1));
        } else if let Some(value) = trimmed.strip_prefix("version = ") {
            version = Some(unquote(value));
        } else if let Some(value) = trimmed.strip_prefix("source = ") {
            source = Some(unquote(value));
        }
    }
    push_cargo_lock_seed(records, name, version, source);
}

fn push_cargo_lock_seed(
    records: &mut Vec<DependencySeed>,
    name: Option<(String, usize)>,
    version: Option<String>,
    source: Option<String>,
) {
    if !cargo_lock_source_is_external(source.as_deref()) {
        return;
    }
    push_lock_seed(records, "cargo", "rust", "Cargo.lock", name, version);
}
