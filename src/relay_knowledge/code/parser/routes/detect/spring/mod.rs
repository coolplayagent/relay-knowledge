use std::collections::BTreeSet;

use super::RouteCandidate;

mod annotations;
mod attributes;
mod java;
mod statements;

use annotations::{
    SpringAnnotationKind, SpringPendingAnnotation, spring_route_annotations_and_tail,
};
use java::{
    java_code_lines_without_comments, line_declares_java_type,
    line_declares_nested_java_helper_type, parse_java_method_def, update_java_brace_depth,
};
use statements::{
    spring_annotation_statement_from_offset, spring_route_annotation_offset,
    spring_statement_after_annotation, spring_tail_after_leading_annotations,
};

pub(in crate::code::parser) fn detect_spring_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending_annotations: Vec<SpringPendingAnnotation> = Vec::new();
    let mut class_prefixes = Vec::<SpringClassPrefix>::new();
    let mut nested_type_scopes = Vec::<SpringNestedTypeScope>::new();
    let mut brace_depth = 0usize;
    let lines = java_code_lines_without_comments(content);
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        restore_closed_spring_nested_type_scopes(
            &mut class_prefixes,
            &mut nested_type_scopes,
            brace_depth,
        );
        if let Some(annotation_offset) = spring_route_annotation_offset(trimmed) {
            let (annotation_statement, annotation_lines) =
                spring_annotation_statement_from_offset(&lines, index, annotation_offset);
            let (spring_routes, annotation_tail) =
                spring_route_annotations_and_tail(&annotation_statement);
            if !spring_routes.is_empty() {
                if pending_request_mapping_can_be_prefix(&pending_annotations) {
                    class_prefixes = pending_request_mapping_prefixes(&pending_annotations);
                    pending_annotations.clear();
                }
                pending_annotations.extend(spring_routes);
                let method_tail = spring_tail_after_leading_annotations(annotation_tail);
                if let Some(method_name) = parse_java_method_def(method_tail) {
                    record_spring_pending_routes(
                        &mut routes,
                        &mut seen,
                        &class_prefixes,
                        &mut pending_annotations,
                        method_name,
                        index + 1,
                    );
                    update_java_brace_depth(method_tail, &mut brace_depth);
                }
                index += annotation_lines;
                continue;
            }
        }
        if !pending_annotations.is_empty() && trimmed.starts_with('@') {
            let (annotation_statement, annotation_lines) =
                spring_annotation_statement_from_offset(&lines, index, 0);
            let annotation_tail = spring_statement_after_annotation(&annotation_statement);
            let method_tail = spring_tail_after_leading_annotations(annotation_tail);
            if let Some(method_name) = parse_java_method_def(method_tail) {
                record_spring_pending_routes(
                    &mut routes,
                    &mut seen,
                    &class_prefixes,
                    &mut pending_annotations,
                    method_name,
                    index + 1,
                );
                update_java_brace_depth(method_tail, &mut brace_depth);
            } else {
                update_java_brace_depth(&annotation_statement, &mut brace_depth);
            }
            index += annotation_lines;
            continue;
        }
        if line_declares_java_type(trimmed) {
            if pending_request_mapping_can_be_prefix(&pending_annotations) {
                class_prefixes = pending_request_mapping_prefixes(&pending_annotations);
                nested_type_scopes.clear();
            } else if !class_prefixes.is_empty()
                && line_declares_nested_java_helper_type(trimmed, brace_depth)
            {
                nested_type_scopes.push(SpringNestedTypeScope {
                    restore_at_depth: brace_depth,
                    class_prefixes: class_prefixes.clone(),
                });
                class_prefixes.clear();
            } else {
                class_prefixes.clear();
                nested_type_scopes.clear();
            }
            pending_annotations.clear();
            update_java_brace_depth(trimmed, &mut brace_depth);
            restore_closed_spring_nested_type_scopes(
                &mut class_prefixes,
                &mut nested_type_scopes,
                brace_depth,
            );
            index += 1;
            continue;
        }
        if !pending_annotations.is_empty() {
            if let Some(method_name) = parse_java_method_def(trimmed) {
                record_spring_pending_routes(
                    &mut routes,
                    &mut seen,
                    &class_prefixes,
                    &mut pending_annotations,
                    method_name,
                    index + 1,
                );
            } else if !trimmed.is_empty()
                && !trimmed.starts_with("public")
                && !trimmed.starts_with("private")
                && !trimmed.starts_with("protected")
                && !trimmed.starts_with("@")
            {
                pending_annotations.clear();
            }
        }
        update_java_brace_depth(trimmed, &mut brace_depth);
        index += 1;
    }
    routes
}

#[derive(Clone)]
struct SpringClassPrefix {
    url: String,
    http_method: String,
}

struct SpringNestedTypeScope {
    restore_at_depth: usize,
    class_prefixes: Vec<SpringClassPrefix>,
}

fn record_spring_pending_routes(
    routes: &mut Vec<RouteCandidate>,
    seen: &mut BTreeSet<(String, String, String, usize)>,
    class_prefixes: &[SpringClassPrefix],
    pending_annotations: &mut Vec<SpringPendingAnnotation>,
    method_name: String,
    line: usize,
) {
    let prefixes = route_class_prefixes(class_prefixes);
    for annotation in pending_annotations.drain(..) {
        for prefix in &prefixes {
            let full_url = merge_url_parts(&prefix.url, &annotation.url);
            for http_method in route_http_methods_with_class_prefix(prefix, &annotation.http_method)
            {
                let key = (
                    full_url.clone(),
                    http_method.clone(),
                    method_name.clone(),
                    line,
                );
                if seen.insert(key) {
                    routes.push(RouteCandidate {
                        url: full_url.clone(),
                        http_method,
                        handler_name: method_name.clone(),
                        framework: "spring".to_owned(),
                        line,
                    });
                }
            }
        }
    }
}

fn pending_request_mapping_can_be_prefix(annotations: &[SpringPendingAnnotation]) -> bool {
    !annotations.is_empty()
        && annotations
            .iter()
            .all(|annotation| annotation.kind == SpringAnnotationKind::RequestMapping)
}

fn pending_request_mapping_prefixes(
    annotations: &[SpringPendingAnnotation],
) -> Vec<SpringClassPrefix> {
    let mut seen = BTreeSet::new();
    let mut prefixes = Vec::new();
    for annotation in annotations {
        let key = (annotation.url.clone(), annotation.http_method.clone());
        if seen.insert(key) {
            prefixes.push(SpringClassPrefix {
                url: annotation.url.clone(),
                http_method: annotation.http_method.clone(),
            });
        }
    }
    prefixes
}

fn route_class_prefixes(class_prefixes: &[SpringClassPrefix]) -> Vec<SpringClassPrefix> {
    if class_prefixes.is_empty() {
        return vec![SpringClassPrefix {
            url: String::new(),
            http_method: "any".to_owned(),
        }];
    }
    class_prefixes.to_vec()
}

fn route_http_methods_with_class_prefix(prefix: &SpringClassPrefix, method: &str) -> Vec<String> {
    if method == "any" {
        return vec![prefix.http_method.clone()];
    }
    if prefix.http_method == "any" || prefix.http_method == method {
        return vec![method.to_owned()];
    }
    vec![prefix.http_method.clone(), method.to_owned()]
}

fn restore_closed_spring_nested_type_scopes(
    class_prefixes: &mut Vec<SpringClassPrefix>,
    nested_type_scopes: &mut Vec<SpringNestedTypeScope>,
    brace_depth: usize,
) {
    while nested_type_scopes
        .last()
        .is_some_and(|scope| brace_depth <= scope.restore_at_depth)
    {
        if let Some(scope) = nested_type_scopes.pop() {
            *class_prefixes = scope.class_prefixes;
        }
    }
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
