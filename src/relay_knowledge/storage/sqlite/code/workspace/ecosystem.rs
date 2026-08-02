//! Ecosystem-specific workspace identity and import normalization rules.

use crate::domain::CodeMonorepoWorkspaceFormat;

const WORKSPACE_PACKAGE_SEPARATORS: [&str; 3] = ["::", "/", "."];

pub(super) fn ecosystem_for_format(format: CodeMonorepoWorkspaceFormat) -> &'static str {
    match format {
        CodeMonorepoWorkspaceFormat::Pnpm => "npm",
        CodeMonorepoWorkspaceFormat::GoModules => "go",
        CodeMonorepoWorkspaceFormat::CargoWorkspace => "rust",
    }
}

pub(super) fn ecosystem_for_language(language_id: &str) -> Option<&'static str> {
    match language_id {
        "javascript" | "jsx" | "typescript" | "tsx" => Some("npm"),
        "go" => Some("go"),
        "rust" => Some("rust"),
        _ => None,
    }
}

pub(super) fn workspace_format_key(format: CodeMonorepoWorkspaceFormat) -> &'static str {
    match format {
        CodeMonorepoWorkspaceFormat::Pnpm => "pnpm",
        CodeMonorepoWorkspaceFormat::GoModules => "go_modules",
        CodeMonorepoWorkspaceFormat::CargoWorkspace => "cargo_workspace",
    }
}

pub(super) fn workspace_manifest_file_name(ecosystem: &str) -> Option<&'static str> {
    match ecosystem {
        "npm" => Some("package.json"),
        "go" => Some("go.mod"),
        "rust" => Some("Cargo.toml"),
        _ => None,
    }
}

pub(super) fn workspace_package_candidates(import_module: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut remaining = import_module.trim();
    if remaining.is_empty() {
        return candidates;
    }

    loop {
        if !candidates.iter().any(|candidate| candidate == remaining) {
            candidates.push(remaining.to_owned());
        }

        match rightmost_package_separator(remaining) {
            Some(separator_index) if separator_index > 0 => {
                remaining = &remaining[..separator_index];
            }
            _ => break,
        }
    }

    candidates
}

fn rightmost_package_separator(value: &str) -> Option<usize> {
    WORKSPACE_PACKAGE_SEPARATORS
        .iter()
        .filter_map(|separator| value.rfind(separator))
        .max()
}

pub(super) fn is_local_or_relative_module(module: &str) -> bool {
    let trimmed = module.trim();
    trimmed.is_empty()
        || matches!(trimmed, "crate" | "self" | "super")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with("crate::")
        || trimmed.starts_with("self::")
        || trimmed.starts_with("super::")
}

pub(super) fn workspace_lookup_module<'a>(module: &'a str, ecosystem: &str) -> &'a str {
    let trimmed = module.trim().trim_end_matches(';').trim();
    match ecosystem {
        "go" => go_workspace_lookup_module(trimmed),
        "npm" => npm_workspace_lookup_module(trimmed),
        "rust" => rust_workspace_lookup_module(trimmed),
        _ => trimmed,
    }
}

fn go_workspace_lookup_module(module: &str) -> &str {
    module
        .split_whitespace()
        .last()
        .unwrap_or(module)
        .trim_end_matches(';')
        .trim_matches(|ch| matches!(ch, '"' | '`' | '\''))
        .trim()
}

fn npm_workspace_lookup_module(module: &str) -> &str {
    quoted_workspace_specifier(module).unwrap_or(module).trim()
}

fn rust_workspace_lookup_module(module: &str) -> &str {
    let mut value = module;
    value = value.strip_prefix("pub use ").unwrap_or(value);
    value = value.strip_prefix("use ").unwrap_or(value);
    value = value.strip_prefix("extern crate ").unwrap_or(value);
    let end = value.find([' ', ';', '{']).unwrap_or(value.len());
    value[..end].trim().trim_end_matches("::").trim()
}

fn quoted_workspace_specifier(statement: &str) -> Option<&str> {
    let start = statement.find(['"', '\'', '`'])?;
    let quote = statement.as_bytes()[start] as char;
    let rest = &statement[start + 1..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

#[cfg(test)]
#[path = "ecosystem_tests.rs"]
mod tests;
