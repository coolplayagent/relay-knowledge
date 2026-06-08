use std::collections::BTreeSet;

use super::RouteCandidate;

pub(in crate::code::parser) fn detect_spring_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending_annotations: Vec<SpringPendingAnnotation> = Vec::new();
    let mut class_prefixes = Vec::<String>::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(spring_routes) = parse_spring_route_annotation(trimmed) {
            if pending_request_mapping_can_be_prefix(&pending_annotations) {
                class_prefixes = pending_request_mapping_urls(&pending_annotations);
                pending_annotations.clear();
            }
            pending_annotations.extend(spring_routes);
            continue;
        }
        if line_declares_java_type(trimmed) {
            if pending_request_mapping_can_be_prefix(&pending_annotations) {
                class_prefixes = pending_request_mapping_urls(&pending_annotations);
            } else {
                class_prefixes.clear();
            }
            pending_annotations.clear();
            continue;
        }
        if !pending_annotations.is_empty() {
            if let Some(method_name) = parse_java_method_def(trimmed) {
                let prefixes = route_class_prefixes(&class_prefixes);
                for annotation in pending_annotations.drain(..) {
                    for prefix in &prefixes {
                        let full_url = merge_url_parts(prefix, &annotation.url);
                        let key = (full_url.clone(), annotation.http_method.clone());
                        if seen.insert(key) {
                            routes.push(RouteCandidate {
                                url: full_url,
                                http_method: annotation.http_method.clone(),
                                handler_name: method_name.clone(),
                                framework: "spring".to_owned(),
                                line: index + 1,
                            });
                        }
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

fn pending_request_mapping_urls(annotations: &[SpringPendingAnnotation]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut urls = Vec::new();
    for annotation in annotations {
        if seen.insert(annotation.url.clone()) {
            urls.push(annotation.url.clone());
        }
    }
    urls
}

fn route_class_prefixes(class_prefixes: &[String]) -> Vec<String> {
    if class_prefixes.is_empty() {
        return vec![String::new()];
    }
    class_prefixes.to_vec()
}

fn parse_spring_route_annotation(line: &str) -> Option<Vec<SpringPendingAnnotation>> {
    let annotation = extract_spring_annotation_name(line)?;
    match annotation {
        "GetMapping" => Some(spring_pending_annotations(
            vec!["get".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "PostMapping" => Some(spring_pending_annotations(
            vec!["post".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "PutMapping" => Some(spring_pending_annotations(
            vec!["put".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "DeleteMapping" => Some(spring_pending_annotations(
            vec!["delete".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "PatchMapping" => Some(spring_pending_annotations(
            vec!["patch".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "RequestMapping" => {
            let urls = extract_annotation_string_values(line);
            Some(spring_pending_annotations(
                extract_spring_method_attributes(line),
                urls,
                SpringAnnotationKind::RequestMapping,
            ))
        }
        _ => None,
    }
}

fn spring_pending_annotations(
    methods: Vec<String>,
    urls: Vec<String>,
    kind: SpringAnnotationKind,
) -> Vec<SpringPendingAnnotation> {
    let urls = if urls.is_empty() {
        vec![String::new()]
    } else {
        urls
    };
    let mut annotations = Vec::new();
    for method in methods {
        for url in &urls {
            annotations.push(SpringPendingAnnotation {
                http_method: method.clone(),
                url: url.clone(),
                kind,
            });
        }
    }
    annotations
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

fn extract_annotation_string_values(line: &str) -> Vec<String> {
    let Some(paren_pos) = line.find('(') else {
        return Vec::new();
    };
    let inner_start = &line[paren_pos + 1..];
    let inner = inner_start.trim_start();
    if inner.starts_with('"') {
        extract_double_quoted_java_string(inner)
            .into_iter()
            .collect()
    } else if inner.starts_with('{') {
        extract_java_string_values_from_attribute_value(inner)
    } else {
        find_named_java_attribute_value(inner, "value")
            .or_else(|| find_named_java_attribute_value(inner, "path"))
            .map(extract_java_string_values_from_attribute_value)
            .unwrap_or_default()
    }
}

fn find_named_java_attribute_value<'a>(inner: &'a str, name: &str) -> Option<&'a str> {
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
            return Some(after_name[eq_pos + 1..].trim_start());
        }
        search_start = end;
    }

    None
}

fn extract_java_string_values_from_attribute_value(value: &str) -> Vec<String> {
    let segment = java_attribute_value_segment(value);
    if segment.trim_start().starts_with('{') {
        return extract_double_quoted_java_strings(segment);
    }
    extract_double_quoted_java_string(segment)
        .into_iter()
        .collect()
}

fn java_attribute_value_segment(value: &str) -> &str {
    let value = value.trim_start();
    if value.starts_with('{') {
        let mut depth = 0usize;
        for (index, character) in value.char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return &value[..=index];
                    }
                }
                _ => {}
            }
        }
        return value;
    }
    let end = value.find([',', ')']).unwrap_or(value.len());
    &value[..end]
}

fn extract_double_quoted_java_strings(s: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(relative_start) = s[offset..].find('"') {
        let start = offset + relative_start;
        let mut value = String::new();
        let mut escaped = false;
        let mut closed_at = None;
        for (relative_index, character) in s[start + 1..].char_indices() {
            if escaped {
                value.push(character);
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == '"' {
                closed_at = Some(start + 1 + relative_index + character.len_utf8());
                break;
            }
            value.push(character);
        }
        let Some(next_offset) = closed_at else {
            break;
        };
        values.push(value);
        offset = next_offset;
    }
    values
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
        None => return vec!["any".to_owned()],
    };
    let inner = &line[paren_pos + 1..];
    let Some(after_eq) = find_named_java_attribute_value(inner, "method") else {
        return vec!["any".to_owned()];
    };
    let raw_value = java_attribute_value_segment(after_eq);
    let mut methods = Vec::new();
    for part in raw_value
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
    {
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
        methods.push("any".to_owned());
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
