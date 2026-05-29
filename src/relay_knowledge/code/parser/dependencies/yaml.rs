use std::path::Path;

use super::{DependencySeed, SeedInput, push_seed, strip_comment};

pub(super) fn iac_yaml_path(path: &str, file_name: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file_name = file_name.to_ascii_lowercase();
    (lower.starts_with(".github/workflows/")
        && (file_name.ends_with(".yml") || file_name.ends_with(".yaml")))
        || matches!(
            file_name.as_str(),
            ".gitlab-ci.yml"
                | ".gitlab-ci.yaml"
                | "docker-compose.yml"
                | "docker-compose.yaml"
                | "compose.yml"
                | "compose.yaml"
                | "chart.yaml"
                | "requirements.yml"
                | "requirements.yaml"
        )
}

pub(super) fn parse_iac_yaml(path: &str, content: &str, records: &mut Vec<DependencySeed>) {
    let Some(file_name) = Path::new(path).file_name().and_then(|value| value.to_str()) else {
        return;
    };
    let lower = path.to_ascii_lowercase();
    if lower.starts_with(".github/workflows/") {
        parse_github_actions(content, records);
    } else if matches!(file_name, ".gitlab-ci.yml" | ".gitlab-ci.yaml") {
        parse_image_lines(content, "gitlab-ci", records);
    } else if matches!(
        file_name,
        "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml"
    ) {
        parse_image_lines(content, "compose", records);
    } else if file_name == "Chart.yaml" {
        parse_helm_chart(content, records);
    } else if matches!(file_name, "requirements.yml" | "requirements.yaml") {
        parse_ansible_requirements(content, records);
    } else {
        parse_image_lines(content, "yaml", records);
    }
}

fn parse_github_actions(content: &str, records: &mut Vec<DependencySeed>) {
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        let Some(value) = yaml_scalar_after_key(trimmed, "uses") else {
            continue;
        };
        if value.starts_with("./") || value.starts_with("../") || value.contains("${{") {
            continue;
        }
        let (name, requirement) = split_at_version(value, '@');
        push_dependency(
            records,
            YamlDependencyInput::new(
                "github-actions",
                name,
                requirement,
                "uses",
                ".github/workflows/*.yml",
            )
            .line(index + 1)
            .excerpt(trimmed),
        );
    }
    parse_image_lines(content, "github-actions", records);
}

fn parse_image_lines(content: &str, group: &str, records: &mut Vec<DependencySeed>) {
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        let Some(value) = yaml_scalar_after_key(trimmed, "image") else {
            continue;
        };
        if value.contains("${") || value.contains("{{") || value.starts_with('.') {
            continue;
        }
        let (name, requirement) = container_image(value);
        push_dependency(
            records,
            YamlDependencyInput::new("container", name, requirement, group, "yaml")
                .line(index + 1)
                .excerpt(trimmed),
        );
    }
}

fn parse_helm_chart(content: &str, records: &mut Vec<DependencySeed>) {
    let mut in_dependencies = false;
    let mut name = None::<(String, usize, String)>;
    let mut version = None::<String>;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed == "dependencies:" {
            in_dependencies = true;
            continue;
        }
        if !in_dependencies {
            continue;
        }
        if top_level_yaml_key(line, trimmed) {
            push_pending_helm(records, name.take(), version.take());
            in_dependencies = false;
            continue;
        }
        if trimmed.starts_with("- ") {
            push_pending_helm(records, name.take(), version.take());
        }
        if let Some(value) = trimmed.strip_prefix("- name:") {
            name = Some((clean_yaml_scalar(value), index + 1, trimmed.to_owned()));
        } else if let Some(value) = yaml_scalar_after_key(trimmed, "name") {
            name = Some((value.to_owned(), index + 1, trimmed.to_owned()));
        }
        if let Some(value) = yaml_scalar_after_key(trimmed, "version") {
            version = Some(value.to_owned());
        }
    }
    push_pending_helm(records, name, version);
}

fn push_pending_helm(
    records: &mut Vec<DependencySeed>,
    name: Option<(String, usize, String)>,
    version: Option<String>,
) {
    let Some((name, line, excerpt)) = name else {
        return;
    };
    push_dependency(
        records,
        YamlDependencyInput::new("helm", name, version, "dependencies", "Chart.yaml")
            .line(line)
            .excerpt(excerpt),
    );
}

fn parse_ansible_requirements(content: &str, records: &mut Vec<DependencySeed>) {
    let mut name = None::<(String, usize, String)>;
    let mut version = None::<String>;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed.starts_with("- ") {
            push_pending_ansible(records, name.take(), version.take());
        }
        if let Some(value) = trimmed.strip_prefix("- name:") {
            name = Some((clean_yaml_scalar(value), index + 1, trimmed.to_owned()));
        } else if let Some(value) = bare_ansible_requirement(trimmed) {
            push_dependency(
                records,
                YamlDependencyInput::new(
                    "ansible",
                    value,
                    None,
                    "requirements",
                    "requirements.yml",
                )
                .line(index + 1)
                .excerpt(trimmed),
            );
        } else if let Some(value) = yaml_scalar_after_key(trimmed, "name") {
            name = Some((value.to_owned(), index + 1, trimmed.to_owned()));
        }
        if let Some(value) = yaml_scalar_after_key(trimmed, "version") {
            version = Some(value.to_owned());
        }
    }
    push_pending_ansible(records, name, version);
}

fn push_pending_ansible(
    records: &mut Vec<DependencySeed>,
    name: Option<(String, usize, String)>,
    version: Option<String>,
) {
    let Some((name, line, excerpt)) = name else {
        return;
    };
    push_dependency(
        records,
        YamlDependencyInput::new("ansible", name, version, "requirements", "requirements.yml")
            .line(line)
            .excerpt(excerpt),
    );
}

fn bare_ansible_requirement(trimmed: &str) -> Option<String> {
    let value = trimmed.strip_prefix("- ")?.trim();
    if value.is_empty() || value.contains(':') || value.starts_with('.') {
        return None;
    }
    Some(clean_yaml_scalar(value))
}

struct YamlDependencyInput<'a> {
    ecosystem: &'static str,
    name: String,
    requirement: Option<String>,
    group: &'a str,
    source_kind: &'static str,
    line: usize,
    excerpt: String,
}

impl<'a> YamlDependencyInput<'a> {
    fn new(
        ecosystem: &'static str,
        name: String,
        requirement: Option<String>,
        group: &'a str,
        source_kind: &'static str,
    ) -> Self {
        Self {
            ecosystem,
            name,
            requirement,
            group,
            source_kind,
            line: 1,
            excerpt: String::new(),
        }
    }

    fn line(mut self, line: usize) -> Self {
        self.line = line;
        self
    }

    fn excerpt(mut self, excerpt: impl Into<String>) -> Self {
        self.excerpt = excerpt.into();
        self
    }
}

fn push_dependency(records: &mut Vec<DependencySeed>, input: YamlDependencyInput<'_>) {
    let name = input.name;
    if name.is_empty() || name.contains("${") || name.contains("{{") {
        return;
    }
    push_seed(
        records,
        SeedInput::new(
            input.ecosystem,
            "yaml",
            name,
            input.requirement,
            input.group,
            input.source_kind,
            false,
        )
        .line(input.line)
        .excerpt(input.excerpt),
    );
}

fn yaml_scalar_after_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let line = line.strip_prefix("- ").unwrap_or(line).trim();
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix(':')?.trim();
    (!rest.is_empty()).then_some(rest.trim_matches('"').trim_matches('\''))
}

fn split_at_version(value: &str, delimiter: char) -> (String, Option<String>) {
    value.rsplit_once(delimiter).map_or_else(
        || (value.to_owned(), None),
        |(name, version)| (name.to_owned(), Some(version.to_owned())),
    )
}

fn container_image(value: &str) -> (String, Option<String>) {
    let value = clean_yaml_scalar(value);
    if let Some((name, digest)) = value.split_once('@') {
        return (name.to_owned(), Some(format!("@{digest}")));
    }
    let slash = value.rfind('/').unwrap_or(0);
    let tag_index = value[slash..].rfind(':').map(|index| slash + index);
    match tag_index {
        Some(index) => (
            value[..index].to_owned(),
            Some(value[index + 1..].to_owned()),
        ),
        None => (value, None),
    }
}

fn top_level_yaml_key(line: &str, trimmed: &str) -> bool {
    !trimmed.is_empty()
        && !line.starts_with(char::is_whitespace)
        && !trimmed.starts_with('-')
        && trimmed.contains(':')
}

fn clean_yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_owned()
}
