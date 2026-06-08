use std::collections::{BTreeMap, BTreeSet};

use super::RouteCandidate;
use super::shared::extract_quoted_string_python;

const MAX_FLASK_ROUTE_DECORATOR_LINES: usize = 12;
const MAX_PYTHON_ROUTER_PREFIX_LINES: usize = 12;

pub(in crate::code::parser) fn detect_flask_routes(content: &str) -> Vec<RouteCandidate> {
    let mut route_bindings = Vec::new();
    let mut pending_routes = Vec::<FlaskRouteInfo>::new();
    let mut routers = BTreeMap::<String, PythonRouterInfo>::new();
    let lines = python_code_lines_without_triple_quoted_strings(content);
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some((prefix_statement, prefix_lines)) =
            python_router_prefix_statement(&lines, index)
        {
            if let Some((router_name, router_info)) = parse_python_router_prefix(&prefix_statement)
            {
                merge_python_router_declaration(&mut routers, router_name, router_info);
                index += prefix_lines;
                continue;
            }
        }
        if let Some((include_statement, include_lines)) =
            python_include_router_statement(&lines, index)
        {
            if apply_python_include_router_prefix(&include_statement, &mut routers) {
                index += include_lines;
                continue;
            }
        }
        if let Some((blueprint_statement, blueprint_lines)) =
            python_register_blueprint_statement(&lines, index)
        {
            if apply_python_register_blueprint_prefix(&blueprint_statement, &mut routers) {
                index += blueprint_lines;
                continue;
            }
        }
        if trimmed.starts_with("@") {
            let (decorator_statement, decorator_lines) = flask_decorator_statement(&lines, index);
            if let Some(route_info) = parse_flask_decorator(&decorator_statement, &routers) {
                pending_routes.push(route_info);
                index += decorator_lines;
                continue;
            }
            if let Some(route_info) = pending_routes.last_mut() {
                if let Some(methods) = parse_flask_methods_decorator(&decorator_statement) {
                    route_info.methods = methods;
                    index += decorator_lines;
                    continue;
                }
            }
            index += 1;
            continue;
        }
        if !pending_routes.is_empty() {
            if let Some(func_name) = parse_python_function_def(trimmed) {
                let handler = func_name;
                for route_info in pending_routes.drain(..) {
                    let methods = if route_info.methods.is_empty() {
                        vec!["get".to_owned()]
                    } else {
                        route_info.methods
                    };
                    for method in methods {
                        route_bindings.push(PythonRouteBinding {
                            receiver_name: route_info.receiver_name.clone(),
                            local_url: route_info.local_url.clone(),
                            http_method: method,
                            handler_name: handler.clone(),
                            framework: route_info.framework.clone(),
                            line: index + 1,
                        });
                    }
                }
            }
            pending_routes.clear();
        }
        index += 1;
    }
    materialize_python_routes(route_bindings, &routers)
}

struct FlaskRouteInfo {
    receiver_name: Option<String>,
    local_url: String,
    methods: Vec<String>,
    framework: String,
}

struct PythonRouteBinding {
    receiver_name: Option<String>,
    local_url: String,
    http_method: String,
    handler_name: String,
    framework: String,
    line: usize,
}

#[derive(Clone)]
struct PythonRouterInfo {
    local_prefix: String,
    mount_prefixes: BTreeSet<String>,
    framework: String,
}

fn parse_flask_decorator(
    line: &str,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> Option<FlaskRouteInfo> {
    let line = line.trim_start_matches('@');
    let (func_part, args) = if let Some(paren_pos) = line.find('(') {
        (&line[..paren_pos], &line[paren_pos + 1..])
    } else {
        return None;
    };
    let route_method = extract_flask_http_method(func_part);
    let is_route = func_line_matches_route(func_part);
    if !is_route {
        return None;
    }
    let args_trimmed = trim_one_trailing_paren(args);
    let url = extract_python_route_path(args_trimmed)?;
    let receiver_name = python_decorator_receiver(func_part);
    let framework = route_framework(func_part, receiver_name.as_deref(), routers);
    let methods = if route_method.is_empty() {
        extract_methods_from_flask_args(args_trimmed)
    } else {
        vec![route_method]
    };
    Some(FlaskRouteInfo {
        receiver_name,
        local_url: url,
        methods,
        framework,
    })
}

fn parse_python_router_prefix(line: &str) -> Option<(String, PythonRouterInfo)> {
    let (left, right) = line.split_once('=')?;
    let router_name = python_assignment_name(left)?;
    if router_name.is_empty()
        || !router_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    let router_info = if right.contains("APIRouter(") {
        PythonRouterInfo {
            local_prefix: extract_python_keyword_string(right, "prefix").unwrap_or_default(),
            mount_prefixes: BTreeSet::new(),
            framework: "fastapi".to_owned(),
        }
    } else if right.contains("FastAPI(") {
        PythonRouterInfo {
            local_prefix: String::new(),
            mount_prefixes: BTreeSet::new(),
            framework: "fastapi".to_owned(),
        }
    } else if right.contains("Blueprint(") {
        PythonRouterInfo {
            local_prefix: extract_python_keyword_string(right, "url_prefix").unwrap_or_default(),
            mount_prefixes: BTreeSet::new(),
            framework: "flask".to_owned(),
        }
    } else {
        return None;
    };

    Some((router_name, router_info))
}

fn merge_python_router_declaration(
    routers: &mut BTreeMap<String, PythonRouterInfo>,
    router_name: String,
    mut router_info: PythonRouterInfo,
) {
    if let Some(existing) = routers.remove(&router_name) {
        router_info.mount_prefixes = existing.mount_prefixes;
    }
    routers.insert(router_name, router_info);
}

fn python_router_prefix_statement(lines: &[String], start: usize) -> Option<(String, usize)> {
    let first_line = lines[start].trim();
    if !first_line.contains('=')
        || (!first_line.contains("APIRouter(")
            && !first_line.contains("FastAPI(")
            && !first_line.contains("Blueprint("))
    {
        return None;
    }
    Some(python_parenthesized_statement(
        lines,
        start,
        MAX_PYTHON_ROUTER_PREFIX_LINES,
    ))
}

fn python_include_router_statement(lines: &[String], start: usize) -> Option<(String, usize)> {
    let first_line = lines[start].trim();
    if !first_line.contains(".include_router(") {
        return None;
    }
    Some(python_parenthesized_statement(
        lines,
        start,
        MAX_PYTHON_ROUTER_PREFIX_LINES,
    ))
}

fn python_register_blueprint_statement(lines: &[String], start: usize) -> Option<(String, usize)> {
    let first_line = lines[start].trim();
    if !first_line.contains(".register_blueprint(") {
        return None;
    }
    Some(python_parenthesized_statement(
        lines,
        start,
        MAX_PYTHON_ROUTER_PREFIX_LINES,
    ))
}

fn apply_python_include_router_prefix(
    statement: &str,
    routers: &mut BTreeMap<String, PythonRouterInfo>,
) -> bool {
    let Some(paren_pos) = statement.find(".include_router(") else {
        return false;
    };
    let args = trim_one_trailing_paren(&statement[paren_pos + ".include_router(".len()..]);
    let arguments = split_python_top_level_arguments(args);
    let Some(router_name) = arguments.first().map(|argument| argument.trim()) else {
        return false;
    };
    if router_name.is_empty() {
        return false;
    }
    let Some(prefix) = extract_python_keyword_string(args, "prefix") else {
        return false;
    };
    let router_info = routers
        .entry(router_name.to_owned())
        .or_insert_with(|| PythonRouterInfo {
            local_prefix: String::new(),
            mount_prefixes: BTreeSet::new(),
            framework: "fastapi".to_owned(),
        });
    router_info.mount_prefixes.insert(prefix);
    router_info.framework = "fastapi".to_owned();
    true
}

fn apply_python_register_blueprint_prefix(
    statement: &str,
    routers: &mut BTreeMap<String, PythonRouterInfo>,
) -> bool {
    let Some(paren_pos) = statement.find(".register_blueprint(") else {
        return false;
    };
    let args = trim_one_trailing_paren(&statement[paren_pos + ".register_blueprint(".len()..]);
    let arguments = split_python_top_level_arguments(args);
    let Some(blueprint_name) = arguments.first().map(|argument| argument.trim()) else {
        return false;
    };
    if blueprint_name.is_empty() {
        return false;
    }
    let Some(prefix) = extract_python_keyword_string(args, "url_prefix") else {
        return false;
    };
    let router_info =
        routers
            .entry(blueprint_name.to_owned())
            .or_insert_with(|| PythonRouterInfo {
                local_prefix: String::new(),
                mount_prefixes: BTreeSet::new(),
                framework: "flask".to_owned(),
            });
    router_info.mount_prefixes.insert(prefix);
    router_info.framework = "flask".to_owned();
    true
}

fn python_assignment_name(left: &str) -> Option<String> {
    let name = left
        .trim()
        .split_once(':')
        .map_or(left.trim(), |(name, _)| name.trim());
    if name.is_empty() {
        return None;
    }
    Some(name.to_owned())
}

fn flask_decorator_statement(lines: &[String], start: usize) -> (String, usize) {
    python_parenthesized_statement(lines, start, MAX_FLASK_ROUTE_DECORATOR_LINES)
}

fn python_parenthesized_statement(
    lines: &[String],
    start: usize,
    max_lines: usize,
) -> (String, usize) {
    let mut statement = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut saw_open = false;
    let mut consumed = 0usize;
    for line in lines.iter().skip(start).take(max_lines) {
        let segment = line.trim();
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        consumed += 1;
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
            match character {
                '\'' | '"' => quote = Some(character),
                '(' => {
                    depth += 1;
                    saw_open = true;
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    if saw_open && depth == 0 {
                        return (statement, consumed);
                    }
                }
                _ => {}
            }
        }
        if !saw_open {
            return (statement, consumed);
        }
    }
    (statement, consumed.max(1))
}

fn materialize_python_routes(
    route_bindings: Vec<PythonRouteBinding>,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    for route_info in route_bindings {
        for (url, framework) in python_route_urls_and_frameworks(&route_info, routers) {
            let key = (url.clone(), route_info.http_method.clone());
            if seen.insert(key) {
                routes.push(RouteCandidate {
                    url,
                    http_method: route_info.http_method.clone(),
                    handler_name: route_info.handler_name.clone(),
                    framework,
                    line: route_info.line,
                });
            }
        }
    }
    routes
}

fn python_route_urls_and_frameworks(
    route_info: &PythonRouteBinding,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> Vec<(String, String)> {
    let Some(receiver_name) = route_info.receiver_name.as_deref() else {
        return vec![(route_info.local_url.clone(), route_info.framework.clone())];
    };
    let Some(router_info) = routers.get(receiver_name) else {
        return vec![(route_info.local_url.clone(), route_info.framework.clone())];
    };

    python_router_prefixes(router_info)
        .into_iter()
        .map(|prefix| {
            (
                merge_url_parts(&prefix, &route_info.local_url),
                router_info.framework.clone(),
            )
        })
        .collect()
}

fn python_router_prefixes(router_info: &PythonRouterInfo) -> BTreeSet<String> {
    if router_info.mount_prefixes.is_empty() {
        return BTreeSet::from([router_info.local_prefix.clone()]);
    }
    router_info
        .mount_prefixes
        .iter()
        .map(|mount_prefix| merge_url_parts(mount_prefix, &router_info.local_prefix))
        .collect()
}

fn python_decorator_receiver(func_part: &str) -> Option<String> {
    let (receiver, _) = func_part.rsplit_once('.')?;
    Some(receiver.rsplit('.').next().unwrap_or(receiver).to_owned())
}

fn route_framework(
    func_part: &str,
    receiver_name: Option<&str>,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> String {
    if let Some(receiver_name) = receiver_name {
        if let Some(router_info) = routers.get(receiver_name) {
            return router_info.framework.clone();
        }
    }
    if func_part.ends_with(".api_route") {
        return "fastapi".to_owned();
    }
    "flask".to_owned()
}

fn extract_flask_http_method(func_part: &str) -> String {
    let base = func_part.rsplit('.').next().unwrap_or("");
    match base {
        "get" => "get".to_owned(),
        "post" => "post".to_owned(),
        "put" => "put".to_owned(),
        "delete" => "delete".to_owned(),
        "patch" => "patch".to_owned(),
        "head" => "head".to_owned(),
        "options" => "options".to_owned(),
        _ => String::new(),
    }
}

fn func_line_matches_route(func_part: &str) -> bool {
    func_part.ends_with(".route")
        || func_part.ends_with(".api_route")
        || func_part.ends_with(".get")
        || func_part.ends_with(".post")
        || func_part.ends_with(".put")
        || func_part.ends_with(".delete")
        || func_part.ends_with(".patch")
        || func_part.ends_with(".head")
        || func_part.ends_with(".options")
}

fn split_python_top_level_arguments(args: &str) -> Vec<&str> {
    let mut arguments = Vec::new();
    let mut argument_start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in args.char_indices() {
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
            '\'' | '"' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let argument = args[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                argument_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    let argument = args[argument_start..].trim();
    if !argument.is_empty() {
        arguments.push(argument);
    }
    arguments
}

fn parse_flask_methods_decorator(line: &str) -> Option<Vec<String>> {
    let line = line.trim_start_matches('@');
    let (func_part, args) = if let Some(paren_pos) = line.find('(') {
        (&line[..paren_pos], &line[paren_pos + 1..])
    } else {
        return None;
    };
    if func_part != ".methods" {
        let base = func_part.rsplit('.').next().unwrap_or("");
        if base != "methods" {
            return None;
        }
    }
    let args_trimmed = trim_one_trailing_paren(args);
    Some(extract_methods_list_python(args_trimmed))
}

fn trim_one_trailing_paren(args: &str) -> &str {
    let trimmed = args.trim_end();
    trimmed.strip_suffix(')').unwrap_or(trimmed)
}

fn extract_methods_from_flask_args(args: &str) -> Vec<String> {
    let Some(methods_start) = args.find("methods") else {
        let dot_method = extract_shorthand_method_from_route(args);
        return dot_method;
    };
    let after_methods = &args[methods_start + 7..];
    let eq_pos = match after_methods.find('=') {
        Some(p) => p,
        None => return Vec::new(),
    };
    let list_str = &after_methods[eq_pos + 1..];
    extract_methods_list_python(list_str)
}

fn extract_python_keyword_string(args: &str, keyword: &str) -> Option<String> {
    let keyword_start = args.find(keyword)?;
    let after_keyword = &args[keyword_start + keyword.len()..];
    let eq_pos = after_keyword.find('=')?;
    let after_eq = after_keyword[eq_pos + 1..].trim_start();
    extract_quoted_string_python(after_eq)
}

fn extract_python_route_path(args: &str) -> Option<String> {
    extract_quoted_string_python(args)
        .or_else(|| extract_python_keyword_string(args, "path"))
        .or_else(|| extract_python_keyword_string(args, "rule"))
}

fn extract_shorthand_method_from_route(args: &str) -> Vec<String> {
    let first_part = args.split(',').next().unwrap_or("");
    let after_close = first_part.find(')');
    let relevant = match after_close {
        Some(pos) => &first_part[..pos],
        None => first_part,
    };
    let url = extract_quoted_string_python(relevant);
    let Some(url) = url else {
        return Vec::new();
    };
    let after_url_byte_count = relevant.find(&url).map(|start| start + url.len() + 1);
    let Some(after_url_pos) = after_url_byte_count else {
        return vec!["get".to_owned()];
    };
    let remaining = relevant.get(after_url_pos..).unwrap_or("").trim();
    if remaining.starts_with(')') || remaining.starts_with(',') || remaining.is_empty() {
        return vec!["get".to_owned()];
    }
    if remaining.starts_with('"') || remaining.starts_with('\'') {
        if let Some(m) = extract_quoted_string_python(remaining) {
            let method = m.to_ascii_lowercase();
            if matches!(
                method.as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
            ) {
                return vec![method];
            }
        }
    }
    vec!["get".to_owned()]
}

fn extract_methods_list_python(args: &str) -> Vec<String> {
    let trimmed = args.trim();
    let inner = python_collection_literal_inner(trimmed).unwrap_or(trimmed);
    let mut methods = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if let Some(m) = extract_quoted_string_python(item) {
            let method = m.to_ascii_lowercase();
            if matches!(
                method.as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
            ) {
                methods.push(method);
            }
        }
    }
    methods
}

fn python_collection_literal_inner(value: &str) -> Option<&str> {
    let trimmed = value.trim_start();
    let (open_pos, close_char) =
        trimmed
            .char_indices()
            .find_map(|(index, character)| match character {
                '[' => Some((index, ']')),
                '(' => Some((index, ')')),
                _ => None,
            })?;
    let close_pos = trimmed.rfind(close_char)?;
    if close_pos <= open_pos {
        return None;
    }
    Some(&trimmed[open_pos + 1..close_pos])
}

fn parse_python_function_def(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let after_def = trimmed
        .strip_prefix("def ")
        .or_else(|| trimmed.strip_prefix("async def "))?;
    let name_end = after_def
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_def.len());
    let name = &after_def[..name_end];
    if name.is_empty() {
        return None;
    }
    Some(name.to_owned())
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

fn python_code_lines_without_triple_quoted_strings(content: &str) -> Vec<String> {
    let mut delimiter = None;
    content
        .lines()
        .map(|line| {
            let line = python_code_line_without_triple_quoted_strings(line, &mut delimiter);
            python_code_line_without_comment(&line)
        })
        .collect()
}

fn python_code_line_without_triple_quoted_strings(
    line: &str,
    delimiter: &mut Option<&'static str>,
) -> String {
    let mut result = String::new();
    let mut rest = line;
    loop {
        if let Some(active_delimiter) = *delimiter {
            let Some(end_pos) = rest.find(active_delimiter) else {
                return result;
            };
            rest = &rest[end_pos + active_delimiter.len()..];
            *delimiter = None;
            continue;
        }
        let Some((start_pos, next_delimiter)) = next_python_triple_quote(rest) else {
            result.push_str(rest);
            return result;
        };
        result.push_str(&rest[..start_pos]);
        rest = &rest[start_pos + next_delimiter.len()..];
        *delimiter = Some(next_delimiter);
    }
}

fn next_python_triple_quote(value: &str) -> Option<(usize, &'static str)> {
    let single = value.find("'''").map(|index| (index, "'''"));
    let double = value.find("\"\"\"").map(|index| (index, "\"\"\""));
    match (single, double) {
        (Some(single), Some(double)) => Some(if single.0 <= double.0 { single } else { double }),
        (Some(single), None) => Some(single),
        (None, Some(double)) => Some(double),
        (None, None) => None,
    }
}

fn python_code_line_without_comment(line: &str) -> String {
    let mut result = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
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
        if character == '#' {
            break;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        }
        result.push(character);
    }
    result
}
