use std::collections::BTreeSet;

pub(in crate::code::parser) struct RouteCandidate {
    pub(in crate::code::parser) url: String,
    pub(in crate::code::parser) http_method: String,
    pub(in crate::code::parser) handler_name: String,
    pub(in crate::code::parser) framework: String,
    pub(in crate::code::parser) line: usize,
}

pub(in crate::code::parser) fn detect_routes(
    language_id: &str,
    content: &str,
) -> Vec<RouteCandidate> {
    match language_id {
        "javascript" | "typescript" | "tsx" => detect_express_routes(content),
        "python" => detect_flask_routes(content),
        "java" => detect_spring_routes(content),
        _ => Vec::new(),
    }
}

fn detect_express_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed
            .find(".get(")
            .or_else(|| trimmed.find(".post("))
            .or_else(|| trimmed.find(".put("))
            .or_else(|| trimmed.find(".delete("))
            .or_else(|| trimmed.find(".patch("))
            .map(|pos| &trimmed[pos..])
        else {
            continue;
        };
        let (method_part, after_method) = match rest.split_once('(') {
            Some(pair) => pair,
            None => continue,
        };
        let http_method = match method_part
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "get" | "post" | "put" | "delete" | "patch" => {
                method_part.rsplit('.').next().unwrap().to_ascii_lowercase()
            }
            _ => continue,
        };
        let after_method = after_method.trim_start();
        let url = if let Some(url) = extract_quoted_string(after_method) {
            url
        } else {
            continue;
        };
        if !url.starts_with('/') && !url.starts_with("${") && url != "'" && url != "\"" {
            continue;
        }
        let handler = extract_handler_name(after_method);
        let key = (url.clone(), http_method.clone());
        if seen.insert(key) {
            routes.push(RouteCandidate {
                url,
                http_method,
                handler_name: handler.unwrap_or_else(|| "anonymous".to_owned()),
                framework: "express".to_owned(),
                line: index + 1,
            });
        }
    }
    routes
}

fn detect_flask_routes(content: &str) -> Vec<RouteCandidate> {
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
    let is_route = func_line_matches_route(func_part);
    if !is_route {
        return None;
    }
    let args_trimmed = args.trim_end_matches(')');
    let url = extract_quoted_string_python(args_trimmed)?;
    let methods = extract_methods_from_flask_args(args_trimmed);
    Some(FlaskRouteInfo { url, methods })
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

fn detect_spring_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending_annotations: Vec<(String, String)> = Vec::new();
    let mut class_prefix = String::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("@RequestMapping") {
            if trimmed.contains("method") || trimmed.contains("RequestMethod") {
                if let Some(spring_route) = parse_spring_route_annotation(trimmed) {
                    pending_annotations.push(spring_route);
                    continue;
                }
            }
            if let Some(url) = extract_annotation_string_value(trimmed) {
                class_prefix = url;
            }
            continue;
        }
        if let Some(spring_route) = parse_spring_route_annotation(trimmed) {
            pending_annotations.push(spring_route);
            continue;
        }
        if !pending_annotations.is_empty() {
            if let Some(method_name) = parse_java_method_def(trimmed) {
                for (http_method, suffix) in pending_annotations.drain(..) {
                    let full_url = merge_url_parts(&class_prefix, &suffix);
                    let key = (full_url.clone(), http_method.clone());
                    if seen.insert(key) {
                        routes.push(RouteCandidate {
                            url: full_url,
                            http_method,
                            handler_name: method_name.clone(),
                            framework: "spring".to_owned(),
                            line: index + 1,
                        });
                    }
                }
            } else if !trimmed.starts_with("public")
                && !trimmed.starts_with("private")
                && !trimmed.starts_with("protected")
                && !trimmed.starts_with("@")
                && !trimmed.is_empty()
            {
                pending_annotations.clear();
            }
        }
    }
    routes
}

fn parse_spring_route_annotation(line: &str) -> Option<(String, String)> {
    let annotation = extract_spring_annotation_name(line)?;
    match annotation {
        "GetMapping" => Some((
            "get".to_owned(),
            extract_annotation_string_value(line).unwrap_or_default(),
        )),
        "PostMapping" => Some((
            "post".to_owned(),
            extract_annotation_string_value(line).unwrap_or_default(),
        )),
        "PutMapping" => Some((
            "put".to_owned(),
            extract_annotation_string_value(line).unwrap_or_default(),
        )),
        "DeleteMapping" => Some((
            "delete".to_owned(),
            extract_annotation_string_value(line).unwrap_or_default(),
        )),
        "PatchMapping" => Some((
            "patch".to_owned(),
            extract_annotation_string_value(line).unwrap_or_default(),
        )),
        "RequestMapping" => {
            let method = extract_spring_method_attribute(line);
            let url = extract_annotation_string_value(line).unwrap_or_default();
            Some((method, url))
        }
        _ => None,
    }
}

fn extract_spring_annotation_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with('@') {
        return None;
    }
    let after_at = &trimmed[1..];
    let name_end = after_at
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_at.len());
    Some(&after_at[..name_end])
}

fn extract_annotation_string_value(line: &str) -> Option<String> {
    let paren_pos = line.find('(')?;
    let inner_start = &line[paren_pos + 1..];
    let inner = inner_start.trim_start();
    if inner.starts_with('"') {
        extract_double_quoted_java_string(inner)
    } else if inner.starts_with("value") || inner.starts_with("path") {
        let eq_pos = inner.find('=')?;
        let after_eq = inner[eq_pos + 1..].trim_start();
        extract_double_quoted_java_string(after_eq)
    } else {
        None
    }
}

fn extract_double_quoted_java_string(s: &str) -> Option<String> {
    if !s.starts_with('"') {
        return None;
    }
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 1usize;
    while i < chars.len() {
        match chars[i] {
            '"' => break,
            '\\' => {
                if i + 1 < chars.len() {
                    result.push(chars[i + 1]);
                    i += 2;
                } else {
                    break;
                }
            }
            c => {
                result.push(c);
                i += 1;
            }
        }
    }
    Some(result)
}

fn extract_spring_method_attribute(line: &str) -> String {
    let paren_pos = match line.find('(') {
        Some(pos) => pos,
        None => return "get".to_owned(),
    };
    let inner = &line[paren_pos + 1..];
    if inner.contains("method") {
        if let Some(eq_pos) = inner.find("method") {
            let after_eq = &inner[eq_pos + 6..];
            let after_eq = after_eq.trim_start_matches(&[' ', '='][..]);
            if let Some(method_part) = after_eq.strip_prefix("RequestMethod.") {
                let end = method_part
                    .find([',', ')', '}'])
                    .unwrap_or(method_part.len());
                return method_part[..end].trim().to_ascii_lowercase();
            }
        }
    }
    "get".to_owned()
}

fn parse_java_method_def(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let marker = trimmed.find('(')?;
    let before_paren = &trimmed[..marker];
    let name_start = before_paren
        .rfind(|c: char| c.is_whitespace() || c == '<' || c == '>')
        .map_or(0, |pos| pos + 1);
    let name = &before_paren[name_start..];
    if name.is_empty() || name.chars().next().is_some_and(|c| !c.is_alphanumeric()) {
        return None;
    }
    Some(name.to_owned())
}

fn merge_url_parts(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        if suffix.is_empty() {
            return "/".to_owned();
        }
        return if suffix.starts_with('/') {
            suffix.to_owned()
        } else {
            format!("/{suffix}")
        };
    }
    if suffix.is_empty() {
        return prefix.to_owned();
    }
    let p = prefix.trim_end_matches('/');
    let s = suffix.trim_start_matches('/');
    format!("{p}/{s}")
}

fn extract_quoted_string(s: &str) -> Option<String> {
    let s = s.trim_start();
    let quote_char = s.chars().next()?;
    if quote_char != '\'' && quote_char != '"' && quote_char != '`' {
        return None;
    }
    let inner = &s[1..];
    let end = inner.find(quote_char)?;
    Some(inner[..end].to_owned())
}

fn extract_handler_name(s: &str) -> Option<String> {
    let quote_char = s.chars().next()?;
    let after_first_string = &s[1..];
    let close_pos = after_first_string.find(quote_char)?;
    let rest = after_first_string[close_pos + 1..].trim_start();
    let rest = rest.strip_prefix(',')?.trim_start();
    let is_func = rest
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    if !is_func {
        return None;
    }
    let end = rest
        .find(|c: char| c == ')' || c == ',' || c.is_whitespace())
        .unwrap_or(rest.len());
    Some(rest[..end].to_owned())
}

fn extract_quoted_string_python(s: &str) -> Option<String> {
    let s = s.trim_start();
    let quote_char = s.chars().next()?;
    if quote_char != '\'' && quote_char != '"' {
        return None;
    }
    let inner = &s[1..];
    let mut result = String::new();
    let mut escaped = false;
    for c in inner.chars() {
        if escaped {
            result.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == quote_char {
            return Some(result);
        }
        result.push(c);
    }
    Some(result)
}
