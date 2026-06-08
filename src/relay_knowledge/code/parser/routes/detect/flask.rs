use std::collections::{BTreeMap, BTreeSet};

use super::RouteCandidate;
use super::shared::extract_quoted_string_python;

pub(in crate::code::parser) fn detect_flask_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending_routes = Vec::<FlaskRouteInfo>::new();
    let mut router_prefixes = BTreeMap::<String, String>::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some((router_name, prefix)) = parse_fastapi_router_prefix(trimmed) {
            router_prefixes.insert(router_name, prefix);
            continue;
        }
        if trimmed.starts_with("@") {
            if let Some(route_info) = parse_flask_decorator(trimmed, &router_prefixes) {
                pending_routes.push(route_info);
                continue;
            }
            if let Some(route_info) = pending_routes.last_mut() {
                if let Some(methods) = parse_flask_methods_decorator(trimmed) {
                    route_info.methods = methods;
                    continue;
                }
            }
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
                        let key = (route_info.url.clone(), method.clone());
                        if seen.insert(key) {
                            routes.push(RouteCandidate {
                                url: route_info.url.clone(),
                                http_method: method,
                                handler_name: handler.clone(),
                                framework: "flask".to_owned(),
                                line: index + 1,
                            });
                        }
                    }
                }
            }
            pending_routes.clear();
        }
    }
    routes
}

struct FlaskRouteInfo {
    url: String,
    methods: Vec<String>,
}

fn parse_flask_decorator(
    line: &str,
    router_prefixes: &BTreeMap<String, String>,
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
    let args_trimmed = args.trim_end_matches(')');
    let url = extract_quoted_string_python(args_trimmed)?;
    let url = route_url_with_router_prefix(func_part, &url, router_prefixes);
    let methods = if route_method.is_empty() {
        extract_methods_from_flask_args(args_trimmed)
    } else {
        vec![route_method]
    };
    Some(FlaskRouteInfo { url, methods })
}

fn parse_fastapi_router_prefix(line: &str) -> Option<(String, String)> {
    let (left, right) = line.split_once('=')?;
    if !right.contains("APIRouter(") {
        return None;
    }
    let router_name = left.trim();
    if router_name.is_empty()
        || !router_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    let prefix = extract_python_keyword_string(right, "prefix")?;

    Some((router_name.to_owned(), prefix))
}

fn route_url_with_router_prefix(
    func_part: &str,
    url: &str,
    router_prefixes: &BTreeMap<String, String>,
) -> String {
    let Some((receiver, _)) = func_part.rsplit_once('.') else {
        return url.to_owned();
    };
    let receiver = receiver.rsplit('.').next().unwrap_or(receiver);
    let Some(prefix) = router_prefixes.get(receiver) else {
        return url.to_owned();
    };

    merge_url_parts(prefix, url)
}

fn extract_flask_http_method(func_part: &str) -> String {
    let base = func_part.rsplit('.').next().unwrap_or("");
    match base {
        "get" => "get".to_owned(),
        "post" => "post".to_owned(),
        "put" => "put".to_owned(),
        "delete" => "delete".to_owned(),
        "patch" => "patch".to_owned(),
        _ => String::new(),
    }
}

fn func_line_matches_route(func_part: &str) -> bool {
    func_part.ends_with(".route")
        || func_part.ends_with(".get")
        || func_part.ends_with(".post")
        || func_part.ends_with(".put")
        || func_part.ends_with(".delete")
        || func_part.ends_with(".patch")
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
    let args_trimmed = args.trim_end_matches(')');
    Some(extract_methods_list_python(args_trimmed))
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
    let start = match list_str.find('[') {
        Some(p) => p,
        None => return Vec::new(),
    };
    let end = match list_str.rfind(']') {
        Some(p) => p,
        None => return Vec::new(),
    };
    let inner = &list_str[start + 1..end];
    extract_methods_list_python(inner)
}

fn extract_python_keyword_string(args: &str, keyword: &str) -> Option<String> {
    let keyword_start = args.find(keyword)?;
    let after_keyword = &args[keyword_start + keyword.len()..];
    let eq_pos = after_keyword.find('=')?;
    let after_eq = after_keyword[eq_pos + 1..].trim_start();
    extract_quoted_string_python(after_eq)
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
    let inner = trimmed.trim_start_matches('[').trim_end_matches(']');
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
