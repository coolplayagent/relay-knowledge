use std::collections::BTreeSet;

use super::RouteCandidate;
use super::shared::extract_quoted_string_python;

pub(in crate::code::parser) fn detect_flask_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut in_decorator = false;
    let mut decorator_url: Option<String> = None;
    let mut decorator_methods: Vec<String> = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("@") {
            if let Some(route_info) = parse_flask_decorator(trimmed) {
                in_decorator = true;
                decorator_url = Some(route_info.url);
                decorator_methods = route_info.methods;
                continue;
            }
            if in_decorator {
                if let Some(methods) = parse_flask_methods_decorator(trimmed) {
                    decorator_methods = methods;
                    continue;
                }
            }
            continue;
        }
        if in_decorator {
            if let Some(func_name) = parse_python_function_def(trimmed) {
                let url = decorator_url.take().unwrap_or_default();
                let handler = func_name;
                let methods = if decorator_methods.is_empty() {
                    vec!["get".to_owned()]
                } else {
                    decorator_methods.clone()
                };
                for method in methods {
                    let key = (url.clone(), method.clone());
                    if seen.insert(key) {
                        routes.push(RouteCandidate {
                            url: url.clone(),
                            http_method: method,
                            handler_name: handler.clone(),
                            framework: "flask".to_owned(),
                            line: index + 1,
                        });
                    }
                }
            }
            in_decorator = false;
            decorator_url = None;
            decorator_methods.clear();
        }
    }
    routes
}

struct FlaskRouteInfo {
    url: String,
    methods: Vec<String>,
}

fn parse_flask_decorator(line: &str) -> Option<FlaskRouteInfo> {
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
    let methods = if route_method.is_empty() {
        extract_methods_from_flask_args(args_trimmed)
    } else {
        vec![route_method]
    };
    Some(FlaskRouteInfo { url, methods })
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
    let methods_start = args.find("methods");
    if methods_start.is_none() {
        let dot_method = extract_shorthand_method_from_route(args);
        return dot_method;
    }
    let after_methods = &args[methods_start.unwrap() + 7..];
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
    if !trimmed.starts_with("def ") {
        return None;
    }
    let after_def = &trimmed[4..];
    let name_end = after_def
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_def.len());
    let name = &after_def[..name_end];
    if name.is_empty() {
        return None;
    }
    Some(name.to_owned())
}
