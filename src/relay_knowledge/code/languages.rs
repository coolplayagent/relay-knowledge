use std::path::Path;

use tree_sitter::Language;

#[derive(Clone, Copy)]
pub(super) struct LanguageSpec {
    pub(super) id: &'static str,
    pub(super) language: fn() -> Language,
    pub(super) tags_query: &'static str,
}

const KOTLIN_TAGS_QUERY: &str = r#"
(package_header
  (qualified_identifier) @name) @definition.module

(function_declaration
  name: (identifier) @name) @definition.function

(class_declaration
  name: (identifier) @name) @definition.class

(object_declaration
  name: (identifier) @name) @definition.module
"#;

const SCALA_TAGS_QUERY: &str = r#"
(package_clause
  name: (package_identifier) @name) @definition.module

(trait_definition
  name: (identifier) @name) @definition.interface

(class_definition
  name: (identifier) @name) @definition.class

(object_definition
  name: (identifier) @name) @definition.module

(function_definition
  name: (identifier) @name) @definition.function

(function_declaration
  name: (identifier) @name) @definition.function

(call_expression
  (identifier) @name) @reference.call
"#;

const BASH_TAGS_QUERY: &str = r#"
(function_definition
  name: (word) @name) @definition.function

(command
  name: (command_name) @name) @reference.call
"#;

const CONFIG_TAGS_QUERY: &str = "";

pub(super) fn language_id(path: &str) -> Option<&'static str> {
    detect_language(path).map(|language| language.id)
}

pub(super) fn detect_language(path: &str) -> Option<LanguageSpec> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    if matches!(
        file_name,
        ".bash_profile" | ".bashrc" | ".profile" | "bashrc" | "bash_profile"
    ) {
        return Some(bash());
    }
    if matches!(file_name, "Gemfile" | "Rakefile") {
        return Some(ruby());
    }
    if dependency_only_manifest_file(file_name) {
        return None;
    }

    let extension = Path::new(path).extension()?.to_str()?;
    language_for_extension(extension)
}

fn dependency_only_manifest_file(file_name: &str) -> bool {
    matches!(file_name, "package-lock.json")
}

pub(super) fn strip_supported_extension(path: &str) -> &str {
    let Some(extension) = Path::new(path).extension().and_then(|value| value.to_str()) else {
        return path;
    };
    if language_for_extension(extension).is_none() {
        return path;
    }
    let extension_start = path.len().saturating_sub(extension.len() + 1);

    &path[..extension_start]
}

pub(super) fn doc_comment_text<'a>(trimmed: &'a str, language_id: &str) -> Option<&'a str> {
    match language_id {
        "rust" => strip_comment_prefix(trimmed, &["///", "//!"]),
        "python" | "ruby" | "bash" => strip_comment_prefix(trimmed, &["#"]),
        "php" => strip_comment_prefix(trimmed, &["///", "//", "#"]),
        "c" | "cpp" | "csharp" | "go" | "java" | "javascript" | "jsx" | "kotlin" | "scala"
        | "swift" | "typescript" | "tsx" => strip_comment_prefix(trimmed, &["///", "//!", "//"]),
        _ => None,
    }
}

fn strip_comment_prefix<'a>(trimmed: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix).map(str::trim))
}

fn language_for_extension(extension: &str) -> Option<LanguageSpec> {
    match extension.to_ascii_lowercase().as_str() {
        "rs" => Some(LanguageSpec {
            id: "rust",
            language: || tree_sitter_rust::LANGUAGE.into(),
            tags_query: tree_sitter_rust::TAGS_QUERY,
        }),
        "py" | "pyw" => Some(LanguageSpec {
            id: "python",
            language: || tree_sitter_python::LANGUAGE.into(),
            tags_query: tree_sitter_python::TAGS_QUERY,
        }),
        "js" | "mjs" | "cjs" => Some(javascript()),
        "json" => Some(LanguageSpec {
            id: "json",
            language: || tree_sitter_json::LANGUAGE.into(),
            tags_query: CONFIG_TAGS_QUERY,
        }),
        "yaml" | "yml" => Some(LanguageSpec {
            id: "yaml",
            language: || tree_sitter_yaml::LANGUAGE.into(),
            tags_query: CONFIG_TAGS_QUERY,
        }),
        "properties" => Some(LanguageSpec {
            id: "properties",
            language: || tree_sitter_properties::LANGUAGE.into(),
            tags_query: CONFIG_TAGS_QUERY,
        }),
        "jsx" => Some(LanguageSpec {
            id: "jsx",
            language: || tree_sitter_javascript::LANGUAGE.into(),
            tags_query: tree_sitter_javascript::TAGS_QUERY,
        }),
        "ts" | "mts" | "cts" => Some(LanguageSpec {
            id: "typescript",
            language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tags_query: tree_sitter_typescript::TAGS_QUERY,
        }),
        "tsx" => Some(LanguageSpec {
            id: "tsx",
            language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
            tags_query: tree_sitter_typescript::TAGS_QUERY,
        }),
        "go" => Some(LanguageSpec {
            id: "go",
            language: || tree_sitter_go::LANGUAGE.into(),
            tags_query: tree_sitter_go::TAGS_QUERY,
        }),
        "java" => Some(LanguageSpec {
            id: "java",
            language: || tree_sitter_java::LANGUAGE.into(),
            tags_query: tree_sitter_java::TAGS_QUERY,
        }),
        "kt" | "kts" => Some(LanguageSpec {
            id: "kotlin",
            language: || tree_sitter_kotlin_ng::LANGUAGE.into(),
            tags_query: KOTLIN_TAGS_QUERY,
        }),
        "scala" | "sc" => Some(LanguageSpec {
            id: "scala",
            language: || tree_sitter_scala::LANGUAGE.into(),
            tags_query: SCALA_TAGS_QUERY,
        }),
        "c" => Some(LanguageSpec {
            id: "c",
            language: || tree_sitter_c::LANGUAGE.into(),
            tags_query: tree_sitter_c::TAGS_QUERY,
        }),
        "h" => Some(LanguageSpec {
            id: "c",
            language: || tree_sitter_c::LANGUAGE.into(),
            tags_query: tree_sitter_c::TAGS_QUERY,
        }),
        "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++" => Some(LanguageSpec {
            id: "cpp",
            language: || tree_sitter_cpp::LANGUAGE.into(),
            tags_query: tree_sitter_cpp::TAGS_QUERY,
        }),
        "cs" => Some(LanguageSpec {
            id: "csharp",
            language: || tree_sitter_c_sharp::LANGUAGE.into(),
            tags_query: tree_sitter_c_sharp::TAGS_QUERY,
        }),
        "rb" => Some(ruby()),
        "php" | "phtml" => Some(LanguageSpec {
            id: "php",
            language: || tree_sitter_php::LANGUAGE_PHP.into(),
            tags_query: tree_sitter_php::TAGS_QUERY,
        }),
        "swift" => Some(LanguageSpec {
            id: "swift",
            language: || tree_sitter_swift::LANGUAGE.into(),
            tags_query: tree_sitter_swift::TAGS_QUERY,
        }),
        "sh" | "bash" | "bats" => Some(bash()),
        _ => None,
    }
}

fn javascript() -> LanguageSpec {
    LanguageSpec {
        id: "javascript",
        language: || tree_sitter_javascript::LANGUAGE.into(),
        tags_query: tree_sitter_javascript::TAGS_QUERY,
    }
}

fn ruby() -> LanguageSpec {
    LanguageSpec {
        id: "ruby",
        language: || tree_sitter_ruby::LANGUAGE.into(),
        tags_query: tree_sitter_ruby::TAGS_QUERY,
    }
}

fn bash() -> LanguageSpec {
    LanguageSpec {
        id: "bash",
        language: || tree_sitter_bash::LANGUAGE.into(),
        tags_query: BASH_TAGS_QUERY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_supported_extensions_without_rewriting_unknown_paths() {
        assert_eq!(strip_supported_extension("src/app.tsx"), "src/app");
        assert_eq!(strip_supported_extension("Gemfile"), "Gemfile");
        assert_eq!(strip_supported_extension("README.md"), "README.md");
    }

    #[test]
    fn doc_comment_rules_cover_known_and_unknown_languages() {
        assert_eq!(
            doc_comment_text("/// Runs retry.", "go"),
            Some("Runs retry.")
        );
        assert_eq!(
            doc_comment_text("# Runs retry.", "python"),
            Some("Runs retry.")
        );
        assert_eq!(doc_comment_text("-- Runs retry.", "sql"), None);
    }
}
