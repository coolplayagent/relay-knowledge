use std::collections::BTreeSet;

use super::super::javascript::javascript_code_lines_without_comments;

pub(super) fn js_assignment_variable_name(left: &str) -> Option<String> {
    let left = left.trim();
    let left = left.strip_prefix("export ").unwrap_or(left).trim_start();
    let left = left
        .strip_prefix("const ")
        .or_else(|| left.strip_prefix("let "))
        .or_else(|| left.strip_prefix("var "))
        .unwrap_or(left)
        .trim();
    let name_end = left
        .find(|character: char| character == ':' || character.is_whitespace())
        .unwrap_or(left.len());
    let name = &left[..name_end];
    if name.is_empty() || !name.chars().all(js_identifier_character) {
        return None;
    }
    Some(name.to_owned())
}

pub(super) fn express_router_factory_names(content: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in javascript_code_lines_without_comments(content) {
        let line = line.trim();
        collect_express_router_import_names(line, &mut names);
        collect_express_router_require_names(line, &mut names);
    }
    names
}

pub(super) fn express_namespace_names(content: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::from(["express".to_owned()]);
    for line in javascript_code_lines_without_comments(content) {
        let line = line.trim();
        let name = express_import_namespace_name(line)
            .or_else(|| express_import_default_name(line))
            .or_else(|| express_require_namespace_name(line));
        if let Some(name) = name {
            names.insert(name);
        }
    }
    names
}

fn collect_express_router_import_names(line: &str, names: &mut BTreeSet<String>) {
    let Some(rest) = line.strip_prefix("import ") else {
        return;
    };
    if !express_imports_from_module(rest) {
        return;
    }
    let Some(imports_start) = rest.find('{') else {
        return;
    };
    let Some(imports_end) = rest[imports_start + 1..].find('}') else {
        return;
    };
    let imports = &rest[imports_start + 1..imports_start + 1 + imports_end];
    collect_express_router_named_bindings(imports, names);
}

fn collect_express_router_require_names(line: &str, names: &mut BTreeSet<String>) {
    let Some((left, right)) = line.split_once('=') else {
        return;
    };
    let right = right.trim_start();
    if !right.starts_with("require('express')")
        && !right.starts_with("require(\"express\")")
        && !right.starts_with("require(`express`)")
    {
        return;
    }
    let Some(imports_start) = left.find('{') else {
        return;
    };
    let Some(imports_end) = left[imports_start + 1..].find('}') else {
        return;
    };
    let imports = &left[imports_start + 1..imports_start + 1 + imports_end];
    collect_express_router_named_bindings(imports, names);
}

fn collect_express_router_named_bindings(imports: &str, names: &mut BTreeSet<String>) {
    for binding in imports.split(',') {
        let binding = binding.trim();
        let Some(alias) = express_router_named_binding_alias(binding) else {
            continue;
        };
        names.insert(alias);
    }
}

fn express_router_named_binding_alias(binding: &str) -> Option<String> {
    if binding == "Router" {
        return Some("Router".to_owned());
    }
    if let Some(alias) = binding.strip_prefix("Router as ") {
        return js_identifier_name(alias.trim());
    }
    if let Some(alias) = binding.strip_prefix("Router:") {
        return js_identifier_name(alias.trim());
    }
    None
}

fn express_import_namespace_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("import * as ")?;
    if !express_imports_from_module(rest) {
        return None;
    }
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    js_identifier_name(&rest[..name_end])
}

fn express_import_default_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("import ")?;
    let rest = rest.strip_prefix("type ").unwrap_or(rest).trim_start();
    if rest.starts_with(['{', '*']) || !express_imports_from_module(rest) {
        return None;
    }
    let name_end = rest
        .find(|character: char| character == ',' || character.is_whitespace())
        .unwrap_or(rest.len());
    js_identifier_name(&rest[..name_end])
}

fn express_require_namespace_name(line: &str) -> Option<String> {
    let (left, right) = line.split_once('=')?;
    let right = right.trim_start();
    if !right.starts_with("require('express')")
        && !right.starts_with("require(\"express\")")
        && !right.starts_with("require(`express`)")
    {
        return None;
    }
    js_assignment_variable_name(left)
}

fn express_imports_from_module(rest: &str) -> bool {
    rest.contains("from 'express'")
        || rest.contains("from \"express\"")
        || rest.contains("from `express`")
}

fn js_identifier_name(name: &str) -> Option<String> {
    (!name.is_empty() && name.chars().all(js_identifier_character)).then(|| name.to_owned())
}

fn js_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '$'
}

#[cfg(test)]
#[path = "bindings_tests.rs"]
mod tests;
