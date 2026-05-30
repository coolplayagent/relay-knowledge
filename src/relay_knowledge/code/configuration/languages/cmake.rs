use super::super::{
    calls::cmake_calls,
    model::{ConfigFact, ConfigImport, ConfigReference},
    source::{
        push_definition, push_import, push_reference, source_lines, strip_line_comment, unquote,
        valid_config_key, valid_target, variable_references,
    },
};

pub(in crate::code::configuration) fn facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    for call in cmake_calls(content) {
        for command in ["add_library", "add_executable", "add_custom_target"] {
            if call.command.eq_ignore_ascii_case(command)
                && let Some(name) = call.args.split_whitespace().next()
                && let Some(name) = literal_target_name(name)
            {
                push_definition(definitions, name, "target", call.range);
            }
        }
        if matches!(call.command.as_str(), "add_library" | "add_executable")
            && let Some(target) = alias_target(&call.args)
        {
            push_reference(references, target, "target", call.range);
        }
        if call.command.eq_ignore_ascii_case("target_link_libraries") {
            for target in call
                .args
                .split_whitespace()
                .skip(1)
                .filter(|part| link_target(part))
            {
                push_reference(references, target, "target", call.range);
            }
        }
        if call.command.eq_ignore_ascii_case("add_dependencies") {
            for target in call
                .args
                .split_whitespace()
                .skip(1)
                .filter_map(literal_target_name)
            {
                push_reference(references, target, "target", call.range);
            }
        }
        if call.command.eq_ignore_ascii_case("set")
            && let Some(name) = call.args.split_whitespace().next()
            && valid_config_key(name)
        {
            push_definition(definitions, name, "variable", call.range);
        }
    }
    for line in source_lines(content) {
        let trimmed = line.text.trim_start();
        for reference in cmake_variables(strip_line_comment(trimmed)) {
            push_reference(references, reference, "variable", line.range());
        }
    }
}

pub(in crate::code::configuration) fn imports(
    path: &str,
    content: &str,
    imports: &mut Vec<ConfigImport>,
) {
    for call in cmake_calls(content) {
        if call.command.eq_ignore_ascii_case("include")
            && let Some(module) = call.args.split_whitespace().next()
            && let Some(module) = include_module(path, module)
        {
            push_import(imports, module, call.range);
        }
        if call.command.eq_ignore_ascii_case("add_subdirectory")
            && let Some(module) = call.args.split_whitespace().next()
            && let Some(module) = subdirectory_module(path, module)
        {
            push_import(imports, module, call.range);
        }
    }
}

fn cmake_variables(line: &str) -> Vec<&str> {
    variable_references(line, "${", "}")
}

fn link_target(value: &str) -> bool {
    let value = unquote(value).trim();
    valid_target(value)
        && !link_keyword(value)
        && !library_file(value)
        && !value.starts_with('-')
        && !value.starts_with("$<")
        && !value.starts_with("LINKER:")
        && !value.starts_with("SHELL:")
}

fn literal_target_name(value: &str) -> Option<&str> {
    let value = unquote(value).trim();
    valid_target(value).then_some(value)
}

fn include_module(path: &str, module: &str) -> Option<String> {
    let module = unquote(module);
    if let Some(relative) = module.strip_prefix("${CMAKE_CURRENT_LIST_DIR}/") {
        return Some(join_parent(path, relative));
    }
    if module.contains('$') {
        None
    } else if module.ends_with(".cmake") {
        Some(module.to_owned())
    } else {
        Some(format!("{module}.cmake"))
    }
}

fn subdirectory_module(path: &str, module: &str) -> Option<String> {
    let module = unquote(module).trim_end_matches('/');
    (!module.contains('$') && !absolute_path(module))
        .then(|| join_parent(path, &format!("{module}/CMakeLists.txt")))
}

fn alias_target(args: &str) -> Option<&str> {
    let mut parts = args.split_whitespace();
    parts.next()?;
    if parts.next()?.eq_ignore_ascii_case("ALIAS") {
        parts.next().and_then(literal_target_name)
    } else {
        None
    }
}

fn join_parent(path: &str, relative: &str) -> String {
    path.rsplit_once('/').map_or_else(
        || relative.to_owned(),
        |(parent, _)| format!("{parent}/{relative}"),
    )
}

fn absolute_path(value: &str) -> bool {
    value.starts_with('/') || value.starts_with('\\') || windows_absolute_path(value)
}

fn windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn link_keyword(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "PRIVATE"
            | "PUBLIC"
            | "INTERFACE"
            | "LINK_PRIVATE"
            | "LINK_PUBLIC"
            | "LINK_INTERFACE_LIBRARIES"
            | "DEBUG"
            | "OPTIMIZED"
            | "GENERAL"
    )
}

fn library_file(value: &str) -> bool {
    value.starts_with('/')
        || value.contains('\\')
        || value.contains('/')
        || value.ends_with(".a")
        || value.ends_with(".lib")
        || value.ends_with(".dll")
        || value.ends_with(".dylib")
        || value.ends_with(".framework")
        || value.ends_with(".so")
        || value.contains(".so.")
}
