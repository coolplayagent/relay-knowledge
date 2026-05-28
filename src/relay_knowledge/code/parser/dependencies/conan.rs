use super::{DependencySeed, SeedInput, conan_reference, push_seed, quoted_values, strip_comment};

pub(super) fn parse_conanfile_txt(content: &str, records: &mut Vec<DependencySeed>) {
    let mut section = String::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.trim_matches(['[', ']']).to_owned();
            continue;
        }
        if !matches!(
            section.as_str(),
            "requires" | "tool_requires" | "build_requires"
        ) {
            continue;
        }
        if let Some((name, version)) = conan_reference(trimmed) {
            push_seed(
                records,
                SeedInput::new(
                    "conan",
                    "cpp",
                    name,
                    version,
                    section.as_str(),
                    "conanfile.txt",
                    false,
                )
                .line(index + 1)
                .excerpt(trimmed),
            );
        }
    }
}

pub(super) fn parse_conanfile_py(content: &str, records: &mut Vec<DependencySeed>) {
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '#').trim();
        let group = if trimmed.contains("build_requires(") || trimmed.contains("tool_requires(") {
            Some("build_requires")
        } else if trimmed.contains("requires(") || trimmed.starts_with("requires =") {
            Some("requires")
        } else {
            None
        };
        let Some(group) = group else {
            continue;
        };
        for quoted in quoted_values(trimmed) {
            if let Some((name, version)) = conan_reference(quoted) {
                push_seed(
                    records,
                    SeedInput::new("conan", "cpp", name, version, group, "conanfile.py", false)
                        .line(index + 1)
                        .excerpt(trimmed),
                );
            }
        }
    }
}
