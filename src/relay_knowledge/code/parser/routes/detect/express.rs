use std::collections::{BTreeMap, BTreeSet};

use super::RouteCandidate;
use super::shared::{
    extract_handler_name, extract_handler_name_from_arguments, extract_quoted_string,
};

const MAX_EXPRESS_ROUTE_REGISTRATION_LINES: usize = 12;

pub(in crate::code::parser) fn detect_express_routes(content: &str) -> Vec<RouteCandidate> {
    let mut route_infos = Vec::new();
    let mut mounts = Vec::new();
    let mut router_names = BTreeSet::<String>::from(["app".to_owned(), "router".to_owned()]);
    let router_factory_imported = express_router_factory_is_imported(content);
    let lines = javascript_code_lines_without_comments(content);
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(router_name) = parse_express_router_alias(trimmed, router_factory_imported) {
            router_names.insert(router_name);
            continue;
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
            continue;
        }
        if express_route_start_position(trimmed).is_none() {
            continue;
        };
        let statement = express_route_statement(&lines, index);
        if record_express_route_chain(&statement, index + 1, &router_names, &mut route_infos) {
            continue;
        }
        let Some(method_pos) = express_method_position(&statement) else {
            continue;
        };
        let Some(receiver_name) = express_receiver_name(&statement[..method_pos]) else {
            continue;
        };
        if !express_router_name_is_router(&receiver_name, &router_names) {
            continue;
        }
        let rest = &statement[method_pos..];
        let (method_part, after_method) = match rest.split_once('(') {
            Some(pair) => pair,
            None => continue,
        };
        let raw_method = method_part.rsplit('.').next().unwrap_or("");
        let Some(http_method) = express_http_method(raw_method) else {
            continue;
        };
        let after_method = after_method.trim_start();
        let urls = express_route_urls(after_method);
        if urls.is_empty() {
            continue;
        };
        let handler = extract_handler_name(after_method);
        for local_url in urls {
            route_infos.push(ExpressRouteInfo {
                receiver_name: receiver_name.clone(),
                local_url,
                http_method: http_method.clone(),
                handler_name: handler.clone().unwrap_or_else(|| "anonymous".to_owned()),
                line: index + 1,
            });
        }
    }
    materialize_express_routes(route_infos, &mounts)
}

struct ExpressRouteInfo {
    receiver_name: String,
    local_url: String,
    http_method: String,
    handler_name: String,
    line: usize,
}

struct ExpressRouterMount {
    receiver_name: String,
    router_name: String,
    local_prefix: String,
}

fn parse_express_router_mounts(
    line: &str,
    router_names: &BTreeSet<String>,
) -> Vec<ExpressRouterMount> {
    let Some(use_pos) = find_javascript_pattern_outside_strings(line, ".use(") else {
        return Vec::new();
    };
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
    let Some(mount_path) = extract_quoted_string(after_use) else {
        return Vec::new();
    };
    if !mount_path.starts_with('/') {
        return Vec::new();
    }
    express_router_mount_names(after_use, router_names)
        .into_iter()
        .map(|router_name| ExpressRouterMount {
            receiver_name: receiver_name.clone(),
            router_name,
            local_prefix: mount_path.clone(),
        })
        .collect()
}

fn record_express_route_chain(
    statement: &str,
    line: usize,
    router_names: &BTreeSet<String>,
    route_infos: &mut Vec<ExpressRouteInfo>,
) -> bool {
    let Some(route_pos) = find_javascript_pattern_outside_strings(statement, ".route(") else {
        return false;
    };
    let Some(receiver_name) = express_receiver_name(&statement[..route_pos]) else {
        return false;
    };
    if !express_router_name_is_router(&receiver_name, router_names) {
        return false;
    }
    let after_route = &statement[route_pos + ".route(".len()..];
    let urls = express_route_urls(after_route);
    if urls.is_empty() {
        return false;
    }
    let mut found_route_method = false;
    let mut scan = after_route;
    while let Some(method_pos) = express_method_position(scan) {
        let rest = &scan[method_pos..];
        let Some((method_part, after_method)) = rest.split_once('(') else {
            break;
        };
        let raw_method = method_part.rsplit('.').next().unwrap_or("");
        let http_method = match express_http_method(raw_method) {
            Some(method) => method,
            None => {
                scan = &rest[method_part.len()..];
                continue;
            }
        };
        found_route_method = true;
        let handler = extract_handler_name_from_arguments(after_method);
        for local_url in &urls {
            route_infos.push(ExpressRouteInfo {
                receiver_name: receiver_name.clone(),
                local_url: local_url.clone(),
                http_method: http_method.clone(),
                handler_name: handler.clone().unwrap_or_else(|| "anonymous".to_owned()),
                line,
            });
        }
        scan = after_method;
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
        .filter(|url| url.starts_with('/') || url.starts_with("${"))
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
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
        .map(str::to_owned)
}

fn express_router_name_is_router(receiver_name: &str, router_names: &BTreeSet<String>) -> bool {
    if router_names.contains(receiver_name) {
        return true;
    }
    let receiver_name = receiver_name.to_ascii_lowercase();

    receiver_name == "app" || receiver_name == "router" || receiver_name.ends_with("router")
}

fn parse_express_router_alias(line: &str, router_factory_imported: bool) -> Option<String> {
    let (left, right) = line.split_once('=')?;
    let right = right.trim_start();
    let uses_express_factory =
        find_javascript_pattern_outside_strings(right, "express.Router(").is_some();
    let uses_imported_factory = router_factory_imported && right.starts_with("Router(");
    let uses_express_application = right.starts_with("express(");
    if !uses_express_factory && !uses_imported_factory && !uses_express_application {
        return None;
    }
    js_assignment_variable_name(left)
}

fn express_router_mount_names(arguments: &str, router_names: &BTreeSet<String>) -> Vec<String> {
    let mut names = BTreeSet::new();
    for argument in javascript_top_level_arguments(arguments)
        .into_iter()
        .skip(1)
    {
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
    if express_router_name_is_router(&router_name, router_names) {
        names.insert(router_name);
    }
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

fn find_javascript_pattern_outside_strings(line: &str, pattern: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
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
        if line[index..].starts_with(pattern) {
            return Some(index);
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        }
    }
    None
}

fn js_assignment_variable_name(left: &str) -> Option<String> {
    let left = left.trim();
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
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '$'
        })
    {
        return None;
    }
    Some(name.to_owned())
}

fn express_router_factory_is_imported(content: &str) -> bool {
    content.lines().any(|line| {
        let line = line.trim();
        line.contains("Router")
            && (line.contains("from 'express'")
                || line.contains("from \"express\"")
                || line.contains("require('express')")
                || line.contains("require(\"express\")"))
    })
}

fn javascript_code_lines_without_comments(content: &str) -> Vec<String> {
    let mut in_block_comment = false;
    content
        .lines()
        .map(|line| javascript_code_line_without_comments(line, &mut in_block_comment))
        .collect()
}

fn javascript_code_line_without_comments(line: &str, in_block_comment: &mut bool) -> String {
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    while let Some(character) = chars.next() {
        if *in_block_comment {
            if character == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block_comment = false;
            }
            continue;
        }
        if let Some(quote_char) = quote {
            result.push(character);
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
        if character == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if character == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *in_block_comment = true;
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        }
        result.push(character);
    }
    result
}

fn statement_ends_with_semicolon(segment: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut last_non_space = None;
    for character in segment.chars() {
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
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
            continue;
        }
        if !character.is_whitespace() {
            last_non_space = Some(character);
        }
    }
    last_non_space == Some(';')
}

fn materialize_express_routes(
    route_infos: Vec<ExpressRouteInfo>,
    mounts: &[ExpressRouterMount],
) -> Vec<RouteCandidate> {
    let router_prefixes = resolved_express_router_prefixes(mounts);
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    for route_info in route_infos {
        let prefixes = router_prefixes
            .get(&route_info.receiver_name)
            .cloned()
            .unwrap_or_else(|| BTreeSet::from([String::new()]));
        for prefix in prefixes {
            let url = merge_url_parts(&prefix, &route_info.local_url);
            let key = (url.clone(), route_info.http_method.clone());
            if seen.insert(key) {
                routes.push(RouteCandidate {
                    url,
                    http_method: route_info.http_method.clone(),
                    handler_name: route_info.handler_name.clone(),
                    framework: "express".to_owned(),
                    line: route_info.line,
                });
            }
        }
    }
    routes
}

fn resolved_express_router_prefixes(
    mounts: &[ExpressRouterMount],
) -> BTreeMap<String, BTreeSet<String>> {
    let mounted_routers = mounts
        .iter()
        .map(|mount| mount.router_name.clone())
        .collect::<BTreeSet<_>>();
    let mut router_prefixes = BTreeMap::<String, BTreeSet<String>>::new();
    for _ in 0..=mounts.len() {
        let mut changed = false;
        for mount in mounts {
            let Some(receiver_prefixes) =
                express_receiver_prefixes(&mount.receiver_name, &router_prefixes, &mounted_routers)
            else {
                continue;
            };
            for receiver_prefix in receiver_prefixes {
                let prefix = merge_url_parts(&receiver_prefix, &mount.local_prefix);
                if router_prefixes
                    .entry(mount.router_name.clone())
                    .or_default()
                    .insert(prefix)
                {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    router_prefixes
}

fn express_receiver_prefixes(
    receiver_name: &str,
    router_prefixes: &BTreeMap<String, BTreeSet<String>>,
    mounted_routers: &BTreeSet<String>,
) -> Option<BTreeSet<String>> {
    if let Some(prefixes) = router_prefixes.get(receiver_name) {
        return Some(prefixes.clone());
    }
    (!mounted_routers.contains(receiver_name)).then(|| BTreeSet::from([String::new()]))
}

fn merge_url_parts(prefix: &str, suffix: &str) -> String {
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
