use super::{
    DependencySeed, SeedInput, parse_assignment_dependency, poetry_dependency_group,
    push_python_requirement, push_seed, pyproject_group, quoted_values, strip_comment,
    support::requirements_dependency_line,
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
        if section.starts_with("project.optional-dependencies") && trimmed.contains('=') {
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
            for item in quoted_values(trimmed) {
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
