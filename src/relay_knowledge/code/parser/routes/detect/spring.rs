use std::collections::BTreeSet;

use super::RouteCandidate;

pub(in crate::code::parser) fn detect_spring_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending_annotations: Vec<SpringPendingAnnotation> = Vec::new();
    let mut class_prefix = String::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(spring_routes) = parse_spring_route_annotation(trimmed) {
            if pending_request_mapping_can_be_prefix(&pending_annotations) {
                if let Some(annotation) = pending_annotations.first() {
                    class_prefix = annotation.url.clone();
                }
                pending_annotations.clear();
            }
            pending_annotations.extend(spring_routes);
            continue;
        }
        if !pending_annotations.is_empty() {
            if line_declares_java_type(trimmed) {
                if let Some(annotation) = pending_annotations.first() {
                    class_prefix = annotation.url.clone();
                }
                pending_annotations.clear();
            } else if let Some(method_name) = parse_java_method_def(trimmed) {
                for annotation in pending_annotations.drain(..) {
                    let full_url = merge_url_parts(&class_prefix, &annotation.url);
                    let key = (full_url.clone(), annotation.http_method.clone());
                    if seen.insert(key) {
                        routes.push(RouteCandidate {
                            url: full_url,
                            http_method: annotation.http_method,
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum SpringAnnotationKind {
    RequestMapping,
    MethodMapping,
}

struct SpringPendingAnnotation {
    http_method: String,
    url: String,
    kind: SpringAnnotationKind,
}

fn pending_request_mapping_can_be_prefix(annotations: &[SpringPendingAnnotation]) -> bool {
    !annotations.is_empty()
        && annotations
            .iter()
            .all(|annotation| annotation.kind == SpringAnnotationKind::RequestMapping)
}

fn parse_spring_route_annotation(line: &str) -> Option<Vec<SpringPendingAnnotation>> {
    let annotation = extract_spring_annotation_name(line)?;
    match annotation {
        "GetMapping" => Some(vec![SpringPendingAnnotation {
            http_method: "get".to_owned(),
            url: extract_annotation_string_value(line).unwrap_or_default(),
            kind: SpringAnnotationKind::MethodMapping,
        }]),
        "PostMapping" => Some(vec![SpringPendingAnnotation {
            http_method: "post".to_owned(),
            url: extract_annotation_string_value(line).unwrap_or_default(),
            kind: SpringAnnotationKind::MethodMapping,
        }]),
        "PutMapping" => Some(vec![SpringPendingAnnotation {
            http_method: "put".to_owned(),
            url: extract_annotation_string_value(line).unwrap_or_default(),
            kind: SpringAnnotationKind::MethodMapping,
        }]),
        "DeleteMapping" => Some(vec![SpringPendingAnnotation {
            http_method: "delete".to_owned(),
            url: extract_annotation_string_value(line).unwrap_or_default(),
            kind: SpringAnnotationKind::MethodMapping,
        }]),
        "PatchMapping" => Some(vec![SpringPendingAnnotation {
            http_method: "patch".to_owned(),
            url: extract_annotation_string_value(line).unwrap_or_default(),
            kind: SpringAnnotationKind::MethodMapping,
        }]),
        "RequestMapping" => {
            let url = extract_annotation_string_value(line).unwrap_or_default();
            Some(
                extract_spring_method_attributes(line)
                    .into_iter()
                    .map(|method| SpringPendingAnnotation {
                        http_method: method,
                        url: url.clone(),
                        kind: SpringAnnotationKind::RequestMapping,
                    })
                    .collect(),
            )
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
    } else {
        extract_named_java_string_attribute(inner, "value")
            .or_else(|| extract_named_java_string_attribute(inner, "path"))
    }
}

fn extract_named_java_string_attribute(inner: &str, name: &str) -> Option<String> {
    let mut search_start = 0usize;
    while let Some(relative_pos) = inner[search_start..].find(name) {
        let start = search_start + relative_pos;
        let end = start + name.len();
        let before_is_boundary = start == 0
            || inner[..start]
                .chars()
                .next_back()
                .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_');
        let after_name = &inner[end..];
        let after_is_boundary = after_name
            .chars()
            .next()
            .is_some_and(|character| character.is_whitespace() || character == '=');
        if before_is_boundary && after_is_boundary {
            let eq_pos = after_name.find('=')?;
            let after_eq = after_name[eq_pos + 1..].trim_start();
            return extract_double_quoted_java_string(after_eq);
        }
        search_start = end;
    }

    None
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

fn extract_spring_method_attributes(line: &str) -> Vec<String> {
    let paren_pos = match line.find('(') {
        Some(pos) => pos,
        None => return vec!["get".to_owned()],
    };
    let inner = &line[paren_pos + 1..];
    let Some(method_start) = inner.find("method") else {
        return vec!["get".to_owned()];
    };
    let after_method = &inner[method_start + 6..];
    let Some(eq_pos) = after_method.find('=') else {
        return vec!["get".to_owned()];
    };
    let after_eq = after_method[eq_pos + 1..].trim_start();
    let value_end = after_eq.find(')').unwrap_or(after_eq.len());
    let raw_value = after_eq[..value_end].trim();
    let raw_value = raw_value
        .strip_prefix('{')
        .unwrap_or(raw_value)
        .trim_end_matches('}');
    let mut methods = Vec::new();
    for part in raw_value.split(',') {
        let part = part.trim();
        let Some(method_part) = part.strip_prefix("RequestMethod.") else {
            continue;
        };
        let method = method_part
            .trim_matches(|character: char| !character.is_ascii_alphabetic())
            .to_ascii_lowercase();
        if matches!(
            method.as_str(),
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
        ) {
            methods.push(method);
        }
    }
    if methods.is_empty() {
        methods.push("get".to_owned());
    }

    methods
}

fn line_declares_java_type(line: &str) -> bool {
    let declaration = line.split('{').next().unwrap_or(line);
    declaration.contains(" class ")
        || declaration.contains(" interface ")
        || declaration.contains(" enum ")
        || declaration.starts_with("class ")
        || declaration.starts_with("interface ")
        || declaration.starts_with("enum ")
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
