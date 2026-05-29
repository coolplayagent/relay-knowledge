use super::{DependencySeed, SeedInput, push_seed};

pub(super) fn parse_cmake_lists(content: &str, records: &mut Vec<DependencySeed>) {
    let mut pending = String::new();
    let mut start_line = 1usize;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_cmake_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }
        if pending.is_empty() {
            start_line = index + 1;
        } else {
            pending.push(' ');
        }
        pending.push_str(trimmed);
        if !balanced_call(&pending) {
            continue;
        }
        parse_cmake_call(&pending, start_line, records);
        pending.clear();
    }
}

fn parse_cmake_call(call: &str, line: usize, records: &mut Vec<DependencySeed>) {
    let Some((name, args)) = split_call(call) else {
        return;
    };
    match name.to_ascii_lowercase().as_str() {
        "find_package" => {
            let parts = cmake_words(args);
            push_cmake_dependency(
                records,
                parts.first().cloned(),
                find_package_version(&parts),
                "find_package",
                line,
                call,
            )
        }
        "pkg_check_modules" | "pkg_search_module" => {
            let parts = cmake_words(args);
            for spec in pkg_config_module_specs(&parts) {
                push_cmake_dependency(records, Some(spec.name), spec.requirement, name, line, call);
            }
        }
        "fetchcontent_declare" | "externalproject_add" => {
            let words = cmake_words(args);
            let package = words.first().cloned();
            let version = cmake_keyword_value(&words, &["GIT_TAG", "URL_HASH"]);
            push_cmake_dependency(records, package, version, name, line, call);
        }
        "cpmaddpackage" => parse_cpm_add_package(args, line, call, records),
        _ => {}
    }
}

fn parse_cpm_add_package(
    args: &str,
    line: usize,
    excerpt: &str,
    records: &mut Vec<DependencySeed>,
) {
    let words = cmake_words(args);
    if words.len() == 1 && words[0].contains('/') {
        let value = words[0].trim_start_matches("gh:");
        let (name, version) = value
            .split_once('#')
            .map_or((value, None), |(name, version)| {
                (name, Some(version.to_owned()))
            });
        push_cmake_dependency(
            records,
            Some(name.rsplit('/').next().unwrap_or(name).to_owned()),
            version,
            "CPMAddPackage",
            line,
            excerpt,
        );
        return;
    }
    let package = cmake_keyword_value(&words, &["NAME"]).or_else(|| words.first().cloned());
    let version = cmake_keyword_value(&words, &["VERSION", "GIT_TAG"]);
    push_cmake_dependency(records, package, version, "CPMAddPackage", line, excerpt);
}

fn push_cmake_dependency(
    records: &mut Vec<DependencySeed>,
    package: Option<String>,
    requirement: Option<String>,
    group: &str,
    line: usize,
    excerpt: &str,
) {
    let Some(package) = package else {
        return;
    };
    if package.starts_with('.') || package.starts_with('/') || package.contains("${") {
        return;
    }
    push_seed(
        records,
        SeedInput::new(
            "cmake",
            "cpp",
            package,
            requirement,
            group,
            "CMakeLists.txt",
            false,
        )
        .line(line)
        .excerpt(excerpt),
    );
}

fn split_call(call: &str) -> Option<(&str, &str)> {
    let (name, rest) = call.split_once('(')?;
    let args = rest.rsplit_once(')')?.0;
    let name = name.trim();
    (!name.is_empty()).then_some((name, args.trim()))
}

fn balanced_call(value: &str) -> bool {
    value.chars().filter(|character| *character == '(').count()
        <= value.chars().filter(|character| *character == ')').count()
}

struct PkgConfigSpec {
    name: String,
    requirement: Option<String>,
}

fn pkg_config_module_specs(words: &[String]) -> Vec<PkgConfigSpec> {
    let mut specs = Vec::new();
    let mut index = 1;
    while let Some(word) = words.get(index) {
        if pkg_config_flag(word) {
            index += 1;
            if word.eq_ignore_ascii_case("IMPORTED_TARGET")
                && words
                    .get(index)
                    .is_some_and(|next| next.eq_ignore_ascii_case("GLOBAL"))
            {
                index += 1;
            }
            continue;
        }
        if let Some(spec) = split_pkg_config_spec(word) {
            specs.push(spec);
        }
        if words
            .get(index + 1)
            .is_some_and(|next| pkg_config_version_operator(next))
            && let (Some(spec), Some(version)) = (specs.last_mut(), words.get(index + 2))
        {
            spec.requirement = Some(format!("{}{}", words[index + 1], version));
            index += 3;
            continue;
        }
        index += 1;
    }
    specs
}

fn split_pkg_config_spec(value: &str) -> Option<PkgConfigSpec> {
    let operator_start = value
        .find(">=")
        .or_else(|| value.find("<="))
        .or_else(|| value.find('='))
        .or_else(|| value.find('>'))
        .or_else(|| value.find('<'));
    let Some(index) = operator_start else {
        return Some(PkgConfigSpec {
            name: value.to_owned(),
            requirement: None,
        });
    };
    let name = value[..index].trim();
    let requirement = value[index..].trim();
    if name.is_empty() || requirement.is_empty() {
        return None;
    }
    Some(PkgConfigSpec {
        name: name.to_owned(),
        requirement: Some(requirement.to_owned()),
    })
}

fn pkg_config_version_operator(value: &str) -> bool {
    matches!(value, "=" | ">" | "<" | ">=" | "<=")
}

fn find_package_version(words: &[String]) -> Option<String> {
    let value = words.get(1)?;
    let upper = value.to_ascii_uppercase();
    if find_package_option(&upper) || value.contains("${") {
        return None;
    }
    Some(value.to_owned())
}

fn find_package_option(value: &str) -> bool {
    matches!(
        value,
        "EXACT"
            | "QUIET"
            | "MODULE"
            | "REQUIRED"
            | "OPTIONAL"
            | "COMPONENTS"
            | "OPTIONAL_COMPONENTS"
            | "CONFIG"
            | "NO_MODULE"
            | "GLOBAL"
            | "NO_POLICY_SCOPE"
            | "BYPASS_PROVIDER"
            | "UNWIND_INCLUDE"
    )
}

fn pkg_config_flag(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "REQUIRED" | "QUIET" | "NO_CMAKE_PATH" | "NO_CMAKE_ENVIRONMENT_PATH" | "IMPORTED_TARGET"
    )
}

fn cmake_keyword_value(words: &[String], keys: &[&str]) -> Option<String> {
    words.windows(2).find_map(|window| {
        keys.iter()
            .any(|key| window[0].eq_ignore_ascii_case(key))
            .then(|| window[1].clone())
    })
}

fn cmake_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for character in value.chars() {
        if let Some(active) = quote {
            if character == active {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn strip_cmake_comment(line: &str) -> &str {
    let mut quote = None;
    for (index, character) in line.char_indices() {
        if let Some(active) = quote {
            if character == active {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character == '#' {
            return &line[..index];
        }
    }
    line
}
