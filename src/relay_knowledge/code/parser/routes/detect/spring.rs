use std::collections::BTreeSet;

use super::RouteCandidate;

pub(in crate::code::parser) fn detect_spring_routes(content: &str) -> Vec<RouteCandidate> {
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
