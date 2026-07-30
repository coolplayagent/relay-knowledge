use super::attributes::{
    extract_annotation_string_values, extract_spring_method_attributes,
    spring_annotation_uses_concatenated_path,
};
use super::statements::{spring_route_annotation_offset, spring_statement_after_annotation};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum SpringAnnotationKind {
    RequestMapping,
    MethodMapping,
}

pub(super) struct SpringPendingAnnotation {
    pub(super) http_method: String,
    pub(super) url: String,
    pub(super) kind: SpringAnnotationKind,
}

pub(super) fn spring_route_annotations_and_tail(
    statement: &str,
) -> (Vec<SpringPendingAnnotation>, &str) {
    let mut annotations = Vec::new();
    let mut scan = statement.trim_start();
    while let Some(annotation_offset) = spring_route_annotation_offset(scan) {
        if !scan[..annotation_offset].trim().is_empty() {
            break;
        }
        let annotation_statement = &scan[annotation_offset..];
        let Some(mut spring_routes) = parse_spring_route_annotation(annotation_statement) else {
            break;
        };
        annotations.append(&mut spring_routes);
        scan = spring_statement_after_annotation(annotation_statement).trim_start();
    }
    (annotations, scan)
}

fn parse_spring_route_annotation(line: &str) -> Option<Vec<SpringPendingAnnotation>> {
    let annotation = extract_spring_annotation_name(line)?;
    if spring_annotation_uses_concatenated_path(line) {
        return Some(Vec::new());
    }
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
    let annotation_name = &after_at[..name_end];
    Some(
        annotation_name
            .rsplit('.')
            .next()
            .unwrap_or(annotation_name),
    )
}

#[cfg(test)]
#[path = "annotations_tests.rs"]
mod tests;
