use std::collections::BTreeSet;

use super::express_materialize::materialize_express_routes;
use super::javascript::{
    find_javascript_pattern_outside_strings, javascript_code_lines_without_comments,
    statement_ends_with_semicolon,
};
use super::shared::{
    extract_handler_name, extract_handler_name_from_arguments, extract_quoted_string,
};
use super::{ANONYMOUS_ROUTE_HANDLER_NAME, RouteCandidate};

pub(super) const DYNAMIC_EXPRESS_MOUNT_PREFIX: &str = "\0dynamic";
const MAX_EXPRESS_ROUTE_REGISTRATION_LINES: usize = 12;

pub(in crate::code::parser) fn detect_express_routes(content: &str) -> Vec<RouteCandidate> {
    let mut route_infos = Vec::new();
    let mut mounts = Vec::new();
    let mut router_names = BTreeSet::<String>::from(["app".to_owned(), "router".to_owned()]);
    let mut root_receiver_names = router_names.clone();
    let express_names = express_namespace_names(content);
    let router_factory_names = express_router_factory_names(content);
    let lines = javascript_code_lines_without_comments(content);
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(application_name) = parse_express_application_alias(trimmed, &express_names) {
            router_names.insert(application_name.clone());
            root_receiver_names.insert(application_name);
        } else if let Some(router_name) =
            parse_express_router_alias(trimmed, &router_factory_names, &express_names)
        {
            root_receiver_names.remove(&router_name);
            router_names.insert(router_name);
        }
        let mount_statement;
        let mount_source = if find_javascript_pattern_outside_strings(trimmed, ".use(").is_some() {
            mount_statement = express_use_statement(&lines, index);
            mount_statement.as_str()
        } else {
            trimmed
        };
        let parsed_mounts = parse_express_router_mounts(mount_source, &router_names);
        if !parsed_mounts.is_empty() {
            for mount in parsed_mounts {
                router_names.insert(mount.router_name.clone());
                mounts.push(mount);
            }
        }
        if express_route_start_position(trimmed).is_none() {
            continue;
        };
        let statement = express_route_statement(&lines, index);
        let recorded_chain =
            record_express_route_chain(&statement, index + 1, &router_names, &mut route_infos);
        let recorded_methods =
            record_express_method_calls(&statement, index + 1, &router_names, &mut route_infos);
        if !recorded_chain && !recorded_methods {
            continue;
        }
    }
    materialize_express_routes(route_infos, &mounts, &root_receiver_names)
}

pub(super) struct ExpressRouteInfo {
    pub(super) receiver_name: String,
    pub(super) local_url: String,
    pub(super) http_method: String,
    pub(super) handler_name: String,
    pub(super) line: usize,
}

pub(super) struct ExpressRouterMount {
    pub(super) receiver_name: String,
    pub(super) router_name: String,
    pub(super) local_prefix: String,
}

fn parse_express_router_mounts(
    line: &str,
    router_names: &BTreeSet<String>,
) -> Vec<ExpressRouterMount> {
    let mut mounts = Vec::new();
    let mut scan = line;
    while let Some(use_pos) = find_javascript_pattern_outside_strings(scan, ".use(") {
        mounts.extend(parse_express_router_mount_at(scan, use_pos, router_names));
        let Some(after_use) = scan[use_pos..]
            .split_once('(')
            .map(|(_, args)| args.trim_start())
        else {
            break;
        };
        let Some(call_end) = javascript_call_end(after_use) else {
            break;
        };
        scan = after_use.get(call_end..).unwrap_or("");
    }
    mounts
}

fn parse_express_router_mount_at(
    line: &str,
    use_pos: usize,
    router_names: &BTreeSet<String>,
) -> Vec<ExpressRouterMount> {
    let Some(receiver_name) = express_receiver_name(&line[..use_pos]) else {
        return Vec::new();
    };
    if !express_router_name_is_router(&receiver_name, router_names) {
        return Vec::new();
    }
    let Some(after_use) = line[use_pos..]
        .split_once('(')
        .map(|(_, args)| args.trim_start())
    else {
        return Vec::new();
    };
    let arguments = javascript_top_level_arguments(after_use);
    let Some(first_argument) = arguments.first() else {
        return Vec::new();
    };
    let (mount_paths, router_arguments) = if let Some(path) = extract_quoted_string(first_argument)
    {
        if path.contains("${") {
            (
                vec![DYNAMIC_EXPRESS_MOUNT_PREFIX.to_owned()],
                &arguments[1..],
            )
        } else if path.starts_with('/') {
            (vec![path], &arguments[1..])
        } else {
            return Vec::new();
        }
    } else if let Some(array_inner) = javascript_array_literal_inner(first_argument) {
        let path_values = extract_quoted_strings(array_inner);
        let paths = route_url_literals(path_values.iter().cloned());
        if paths.is_empty() {
            if path_values.iter().any(|path| path.contains("${")) {
                (
                    vec![DYNAMIC_EXPRESS_MOUNT_PREFIX.to_owned()],
                    &arguments[1..],
                )
            } else {
                (vec![String::new()], arguments.as_slice())
            }
        } else {
            (paths, &arguments[1..])
        }
    } else if express_receiver_name(first_argument).is_some_and(|router_name| {
        express_router_mount_argument_is_router(&router_name, router_names)
    }) {
        (vec![String::new()], arguments.as_slice())
    } else if arguments.len() > 1 && !express_use_argument_looks_like_dynamic_path(first_argument) {
        (vec![String::new()], &arguments[1..])
    } else {
        (
            vec![DYNAMIC_EXPRESS_MOUNT_PREFIX.to_owned()],
            &arguments[1..],
        )
    };
    express_router_mount_names(router_arguments, router_names)
        .into_iter()
        .flat_map(|router_name| {
            mount_paths.iter().map({
                let receiver_name = receiver_name.clone();
                move |mount_path| ExpressRouterMount {
                    receiver_name: receiver_name.clone(),
                    router_name: router_name.clone(),
                    local_prefix: mount_path.clone(),
                }
            })
        })
        .collect()
}

fn record_express_route_chain(
    statement: &str,
    line: usize,
    router_names: &BTreeSet<String>,
    route_infos: &mut Vec<ExpressRouteInfo>,
) -> bool {
    let mut found_route_method = false;
    let mut statement_scan = statement;
    while let Some(route_pos) = find_javascript_pattern_outside_strings(statement_scan, ".route(") {
        let Some(receiver_name) = express_receiver_name(&statement_scan[..route_pos]) else {
            statement_scan = &statement_scan[route_pos + ".route(".len()..];
            continue;
        };
        if !express_router_name_is_router(&receiver_name, router_names) {
            statement_scan = &statement_scan[route_pos + ".route(".len()..];
            continue;
        }
        let after_route = &statement_scan[route_pos + ".route(".len()..];
        let urls = express_route_urls(after_route);
        if urls.is_empty() {
            statement_scan = after_route;
            continue;
        }
        let mut chain_scan = after_route;
        let mut after_chain = after_route;
        while let Some(method_pos) = express_method_position(chain_scan) {
            let rest = &chain_scan[method_pos..];
            let Some((method_part, after_method)) = rest.split_once('(') else {
                break;
            };
            let next_scan = javascript_call_end(after_method)
                .and_then(|end| after_method.get(end..))
                .unwrap_or("");
            after_chain = next_scan;
            let raw_method = method_part.rsplit('.').next().unwrap_or("");
            let Some(http_method) = express_http_method(raw_method) else {
                let next_scan = next_scan.trim_start();
                if !next_scan.starts_with('.') {
                    break;
                }
                chain_scan = next_scan;
                continue;
            };
            found_route_method = true;
            let handler = extract_handler_name_from_arguments(after_method);
            for local_url in &urls {
                route_infos.push(ExpressRouteInfo {
                    receiver_name: receiver_name.clone(),
                    local_url: local_url.clone(),
                    http_method: http_method.clone(),
                    handler_name: handler
                        .clone()
                        .unwrap_or_else(|| ANONYMOUS_ROUTE_HANDLER_NAME.to_owned()),
                    line,
                });
            }
            let next_scan = next_scan.trim_start();
            if !next_scan.starts_with('.') {
                break;
            }
            chain_scan = next_scan;
        }
        statement_scan = after_chain;
    }
    found_route_method
}

fn record_express_method_calls(
    statement: &str,
    line: usize,
    router_names: &BTreeSet<String>,
    route_infos: &mut Vec<ExpressRouteInfo>,
) -> bool {
    let mut found_route_method = false;
    let mut scan = statement;
    while let Some(method_pos) = express_method_position(scan) {
        let rest = &scan[method_pos..];
        let Some((method_part, after_method)) = rest.split_once('(') else {
            break;
        };
        let next_scan = javascript_call_end(after_method)
            .and_then(|end| after_method.get(end..))
            .unwrap_or("");
        let Some(receiver_name) = express_receiver_name(&scan[..method_pos]) else {
            scan = next_scan;
            continue;
        };
        if !express_router_name_is_router(&receiver_name, router_names) {
            scan = next_scan;
            continue;
        }
        let raw_method = method_part.rsplit('.').next().unwrap_or("");
        let Some(http_method) = express_http_method(raw_method) else {
            scan = next_scan;
            continue;
        };
        let after_method = after_method.trim_start();
        let urls = express_route_urls(after_method);
        if urls.is_empty() {
            scan = next_scan;
            continue;
        };
        found_route_method = true;
        let handler = extract_handler_name(after_method);
        for local_url in urls {
            route_infos.push(ExpressRouteInfo {
                receiver_name: receiver_name.clone(),
                local_url,
                http_method: http_method.clone(),
                handler_name: handler
                    .clone()
                    .unwrap_or_else(|| ANONYMOUS_ROUTE_HANDLER_NAME.to_owned()),
                line,
            });
        }
        scan = next_scan;
    }
    found_route_method
}

fn express_route_statement(lines: &[String], start: usize) -> String {
    if find_javascript_pattern_outside_strings(&lines[start], ".route(").is_some() {
        return express_route_chain_statement(lines, start);
    }
    express_method_call_statement(lines, start)
}

fn express_method_call_statement(lines: &[String], start: usize) -> String {
    let mut statement = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut saw_route_call = false;
    for (offset, line) in lines
        .iter()
        .skip(start)
        .take(MAX_EXPRESS_ROUTE_REGISTRATION_LINES)
        .enumerate()
    {
        let segment = line.trim();
        if segment.is_empty() {
            continue;
        }
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        let scan_start = if offset == 0 {
            route_method_open_position(segment).unwrap_or(0)
        } else {
            0
        };
        if route_call_is_closed(
            &segment[scan_start..],
            &mut depth,
            &mut quote,
            &mut escaped,
            &mut saw_route_call,
        ) {
            break;
        }
    }
    statement
}

fn express_route_chain_statement(lines: &[String], start: usize) -> String {
    let mut statement = String::new();
    for (offset, line) in lines
        .iter()
        .skip(start)
        .take(MAX_EXPRESS_ROUTE_REGISTRATION_LINES)
        .enumerate()
    {
        let segment = line.trim();
        if segment.is_empty() {
            continue;
        }
        if offset > 0 && !segment.starts_with('.') {
            break;
        }
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        if statement_ends_with_semicolon(segment) {
            break;
        }
    }
    statement
}

fn express_use_statement(lines: &[String], start: usize) -> String {
    let mut statement = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut saw_route_call = false;
    for (offset, line) in lines
        .iter()
        .skip(start)
        .take(MAX_EXPRESS_ROUTE_REGISTRATION_LINES)
        .enumerate()
    {
        let segment = line.trim();
        if segment.is_empty() {
            continue;
        }
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        let scan_start = if offset == 0 {
            find_javascript_pattern_outside_strings(segment, ".use(").unwrap_or(0)
        } else {
            0
        };
        if route_call_is_closed(
            &segment[scan_start..],
            &mut depth,
            &mut quote,
            &mut escaped,
            &mut saw_route_call,
        ) {
            break;
        }
    }
    statement
}

fn route_method_open_position(line: &str) -> Option<usize> {
    let method_pos = express_route_start_position(line)?;
    let open_relative_pos = line[method_pos..].find('(')?;
    Some(method_pos + open_relative_pos)
}

fn route_call_is_closed(
    segment: &str,
    depth: &mut usize,
    quote: &mut Option<char>,
    escaped: &mut bool,
    saw_route_call: &mut bool,
) -> bool {
    for character in segment.chars() {
        if let Some(quote_char) = quote {
            if *escaped {
                *escaped = false;
                continue;
            }
            if character == '\\' {
                *escaped = true;
                continue;
            }
            if character == *quote_char {
                *quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => *quote = Some(character),
            '(' => {
                *depth += 1;
                *saw_route_call = true;
            }
            ')' => {
                *depth = depth.saturating_sub(1);
                if *saw_route_call && *depth == 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn express_method_position(line: &str) -> Option<usize> {
    [
        ".get(",
        ".post(",
        ".put(",
        ".delete(",
        ".patch(",
        ".head(",
        ".options(",
        ".all(",
    ]
    .into_iter()
    .filter_map(|method| find_javascript_pattern_outside_strings(line, method))
    .min()
}

fn express_route_start_position(line: &str) -> Option<usize> {
    [
        express_method_position(line),
        find_javascript_pattern_outside_strings(line, ".route("),
    ]
    .into_iter()
    .flatten()
    .min()
}

fn express_http_method(raw_method: &str) -> Option<String> {
    let method = raw_method.to_ascii_lowercase();
    match method.as_str() {
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" => Some(method),
        "all" => Some("any".to_owned()),
        _ => None,
    }
}

fn express_route_urls(arguments: &str) -> Vec<String> {
    let Some(first_argument) = first_top_level_argument(arguments) else {
        return Vec::new();
    };
    if let Some(url) = extract_quoted_string(first_argument) {
        return route_url_literals([url]);
    }
    let Some(array_inner) = javascript_array_literal_inner(first_argument) else {
        return Vec::new();
    };
    route_url_literals(extract_quoted_strings(array_inner))
}

fn route_url_literals(urls: impl IntoIterator<Item = String>) -> Vec<String> {
    urls.into_iter()
        .filter(|url| url.starts_with('/') && !url.contains("${"))
        .collect()
}

fn first_top_level_argument(arguments: &str) -> Option<&str> {
    let arguments = arguments.trim_start();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in arguments.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' | ',' if depth == 0 => {
                let argument = arguments[..index].trim();
                return (!argument.is_empty()).then_some(argument);
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let argument = arguments.trim();
    (!argument.is_empty()).then_some(argument)
}

fn javascript_call_end(arguments: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in arguments.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => return Some(index + character.len_utf8()),
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn javascript_array_literal_inner(value: &str) -> Option<&str> {
    let value = value.trim();
    if !value.starts_with('[') {
        return None;
    }
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&value[1..index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_quoted_strings(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(start_relative) = value[offset..].find(['\'', '"', '`']) {
        let start = offset + start_relative;
        if let Some(url) = extract_quoted_string(&value[start..]) {
            offset = start + url.len() + 2;
            values.push(url);
        } else {
            break;
        }
    }
    values
}

fn express_receiver_name(receiver: &str) -> Option<String> {
    receiver
        .rsplit(|character: char| {
            !(character.is_ascii_alphanumeric() || character == '_' || character == '$')
        })
        .find(|part| !part.is_empty())
        .map(str::to_owned)
}

fn express_router_name_is_router(receiver_name: &str, router_names: &BTreeSet<String>) -> bool {
    if router_names.contains(receiver_name) {
        return true;
    }
    let receiver_name = receiver_name.to_ascii_lowercase();

    receiver_name == "app" || receiver_name == "router"
}

fn parse_express_router_alias(
    line: &str,
    router_factory_names: &BTreeSet<String>,
    express_names: &BTreeSet<String>,
) -> Option<String> {
    let (left, right) = line.split_once('=')?;
    let right = right.trim_start();
    let uses_express_factory = express_names.iter().any(|name| {
        find_javascript_pattern_outside_strings(right, &format!("{name}.Router(")).is_some()
    });
    let uses_imported_factory = router_factory_names
        .iter()
        .any(|name| right.starts_with(&format!("{name}(")));
    let uses_required_factory =
        find_javascript_pattern_outside_strings(right, "require('express').Router(").is_some()
            || find_javascript_pattern_outside_strings(right, "require(\"express\").Router(")
                .is_some();
    if !uses_express_factory && !uses_imported_factory && !uses_required_factory {
        return None;
    }
    js_assignment_variable_name(left)
}

fn parse_express_application_alias(line: &str, express_names: &BTreeSet<String>) -> Option<String> {
    let (left, right) = line.split_once('=')?;
    let right = right.trim_start();
    if !express_names
        .iter()
        .any(|name| right.starts_with(&format!("{name}(")))
    {
        return None;
    }
    js_assignment_variable_name(left)
}

fn express_router_mount_names(arguments: &[&str], router_names: &BTreeSet<String>) -> Vec<String> {
    let mut names = BTreeSet::new();
    for argument in arguments {
        collect_express_router_mount_names(argument, router_names, &mut names);
    }
    names.into_iter().collect()
}

fn collect_express_router_mount_names(
    argument: &str,
    router_names: &BTreeSet<String>,
    names: &mut BTreeSet<String>,
) {
    if let Some(inner) = javascript_array_literal_inner(argument) {
        for nested_argument in javascript_top_level_arguments(inner) {
            collect_express_router_mount_names(nested_argument, router_names, names);
        }
        return;
    }
    let Some(router_name) = express_receiver_name(argument) else {
        return;
    };
    if express_router_mount_argument_is_router(&router_name, router_names) {
        names.insert(router_name);
    }
}

fn express_router_mount_argument_is_router(
    router_name: &str,
    router_names: &BTreeSet<String>,
) -> bool {
    if express_router_name_is_router(router_name, router_names) {
        return true;
    }
    router_name.to_ascii_lowercase().ends_with("router")
}

fn express_use_argument_looks_like_dynamic_path(argument: &str) -> bool {
    let Some(name) = express_receiver_name(argument) else {
        return true;
    };
    let name = name.to_ascii_lowercase();
    ["prefix", "path", "url", "route", "base", "mount"]
        .iter()
        .any(|marker| name.contains(marker))
}

fn javascript_top_level_arguments(rest: &str) -> Vec<&str> {
    let mut arguments = Vec::new();
    let mut argument_start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in rest.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                let argument = rest[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                return arguments;
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let argument = rest[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                argument_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    let argument = rest[argument_start..].trim();
    if !argument.is_empty() {
        arguments.push(argument);
    }
    arguments
}

fn js_assignment_variable_name(left: &str) -> Option<String> {
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

fn js_identifier_name(name: &str) -> Option<String> {
    (!name.is_empty() && name.chars().all(js_identifier_character)).then(|| name.to_owned())
}

fn js_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '$'
}

fn express_router_factory_names(content: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in javascript_code_lines_without_comments(content) {
        let line = line.trim();
        collect_express_router_import_names(line, &mut names);
        collect_express_router_require_names(line, &mut names);
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

fn express_namespace_names(content: &str) -> BTreeSet<String> {
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

pub(super) fn merge_url_parts(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        return if suffix.starts_with('/') {
            suffix.to_owned()
        } else {
            format!("/{suffix}")
        };
    }
    if suffix.is_empty() {
        return prefix.to_owned();
    }
    let prefix = prefix.trim_end_matches('/');
    let suffix = suffix.trim_start_matches('/');
    format!("{prefix}/{suffix}")
}
