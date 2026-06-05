use std::path::Path;

use tree_sitter::Language;

use super::super::languages::LanguageSpec;

const EMPTY_TAGS_QUERY: &str = "";

pub(in crate::code) fn detect(path: &str) -> Option<LanguageSpec> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    if let Some(spec) = language_for_file_name(path, file_name) {
        return Some(spec);
    }

    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    language_for_extension(path, &extension)
}

pub(super) fn static_template_language(path: &str) -> Option<&'static str> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "json" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        _ => None,
    }
}

pub(in crate::code) fn recoverable_parse_error(language_id: &str, content: &str) -> bool {
    language_id == "cmake" && cmake_recoverable_content(content)
}

pub(in crate::code) fn manual_parse_status(language_id: &str, content: &str) -> bool {
    match language_id {
        "gotemplate" => gotemplate_actions_balanced(content),
        "ninja" => ninja_manifest_shape(content),
        _ => false,
    }
}

fn cmake_recoverable_content(content: &str) -> bool {
    let mut balance = 0i32;
    for line in content.lines() {
        let code = line.split('#').next().unwrap_or(line).trim();
        if code.is_empty() {
            continue;
        }
        if balance == 0 && !cmake_command_start(code) {
            return false;
        }
        for character in code.chars() {
            match character {
                '(' => balance += 1,
                ')' => balance -= 1,
                _ => {}
            }
            if balance < 0 {
                return false;
            }
        }
    }

    balance == 0
}

fn cmake_command_start(line: &str) -> bool {
    let Some((command, _)) = line.split_once('(') else {
        return false;
    };
    let command = command.trim();
    !command.is_empty()
        && command
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn gotemplate_actions_balanced(content: &str) -> bool {
    let mut rest = content;
    loop {
        let Some(start) = rest.find("{{") else {
            return !rest.contains("}}");
        };
        if rest[..start].contains("}}") {
            return false;
        }
        let after_start = &rest[start + "{{".len()..];
        let Some(end) = after_start.find("}}") else {
            return false;
        };
        rest = &after_start[end + "}}".len()..];
    }
}

fn ninja_manifest_shape(content: &str) -> bool {
    let mut has_manifest_line = false;
    for line in content.lines() {
        let trimmed = line.split('#').next().unwrap_or(line).trim();
        if trimmed.is_empty() || line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("build ") {
            if !rest.contains(':') {
                return false;
            }
        } else if !ninja_declaration_line(trimmed) {
            return false;
        }
        has_manifest_line = true;
    }

    has_manifest_line
}

fn ninja_declaration_line(line: &str) -> bool {
    line.split_once('=').is_some()
        || ["rule ", "include ", "subninja ", "default ", "pool "]
            .iter()
            .any(|prefix| line.starts_with(prefix))
}

fn language_for_file_name(path: &str, file_name: &str) -> Option<LanguageSpec> {
    match file_name {
        "BUILD" | "BUILD.bazel" | "WORKSPACE" | "WORKSPACE.bazel" | "MODULE.bazel" => {
            Some(spec("starlark", || tree_sitter_starlark::LANGUAGE.into()))
        }
        "Makefile" | "GNUmakefile" | "BSDmakefile" => {
            Some(spec("make", || tree_sitter_make::LANGUAGE.into()))
        }
        "CMakeLists.txt" => Some(spec("cmake", || tree_sitter_cmake::LANGUAGE.into())),
        "Dockerfile" | "Containerfile" => Some(dockerfile()),
        "go.mod" => Some(spec("gomod", || tree_sitter_gomod_orchard::LANGUAGE.into())),
        "build.ninja" => Some(spec("ninja", || tree_sitter_make::LANGUAGE.into())),
        _ if file_name.starts_with("Dockerfile.")
            || file_name.starts_with("Containerfile.")
            || file_name.ends_with(".Dockerfile")
            || file_name.ends_with(".Containerfile") =>
        {
            Some(dockerfile())
        }
        _ if template_directory_path(path) && content_template_name(file_name) => {
            Some(spec("gotemplate", || tree_sitter_jinja2::LANGUAGE.into()))
        }
        _ => None,
    }
}

fn language_for_extension(path: &str, extension: &str) -> Option<LanguageSpec> {
    match extension {
        "md" | "markdown" => Some(spec("markdown", || tree_sitter_md::LANGUAGE.into())),
        "xml" | "xsd" | "xsl" | "xslt" => {
            Some(spec("xml", || tree_sitter_xml::LANGUAGE_XML.into()))
        }
        "bzl" => Some(spec("starlark", || tree_sitter_starlark::LANGUAGE.into())),
        "mk" => Some(spec("make", || tree_sitter_make::LANGUAGE.into())),
        "cmake" => Some(spec("cmake", || tree_sitter_cmake::LANGUAGE.into())),
        "properties" => Some(spec("properties", || {
            tree_sitter_properties::LANGUAGE.into()
        })),
        "toml" => Some(spec("toml", || tree_sitter_toml_ng::LANGUAGE.into())),
        "conf" | "ini" | "cfg" => Some(spec("ini", || tree_sitter_ini::LANGUAGE.into())),
        "yaml" | "yml" => Some(spec("yaml", || tree_sitter_yaml::LANGUAGE.into())),
        "json" => Some(spec("json", || tree_sitter_json::LANGUAGE.into())),
        "ninja" => Some(spec("ninja", || tree_sitter_make::LANGUAGE.into())),
        "j2" | "jinja" | "jinja2" => Some(spec("jinja2", || tree_sitter_jinja2::LANGUAGE.into())),
        "gotmpl" | "tmpl" | "tpl" => {
            Some(spec("gotemplate", || tree_sitter_jinja2::LANGUAGE.into()))
        }
        _ if template_suffix(path) => Some(spec("jinja2", || tree_sitter_jinja2::LANGUAGE.into())),
        _ => None,
    }
}

fn spec(id: &'static str, language: fn() -> Language) -> LanguageSpec {
    LanguageSpec {
        id,
        language,
        tags_query: EMPTY_TAGS_QUERY,
    }
}

fn dockerfile() -> LanguageSpec {
    spec("dockerfile", || tree_sitter_containerfile::LANGUAGE.into())
}

fn template_suffix(path: &str) -> bool {
    path.ends_with(".yaml.j2")
        || path.ends_with(".yml.j2")
        || path.ends_with(".json.j2")
        || path.ends_with(".toml.j2")
        || path.ends_with(".xml.j2")
}

fn content_template_name(file_name: &str) -> bool {
    file_name.ends_with(".yaml")
        || file_name.ends_with(".yml")
        || file_name.ends_with(".json")
        || file_name.ends_with(".toml")
        || file_name.ends_with(".tpl")
        || file_name == "NOTES.txt"
}

fn template_directory_path(path: &str) -> bool {
    path.starts_with("templates/") || path.contains("/templates/")
}
