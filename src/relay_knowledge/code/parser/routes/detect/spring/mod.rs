use std::collections::BTreeSet;

use super::RouteCandidate;

mod annotations;
mod attributes;
mod java;
mod materialize;
mod statements;

use annotations::{SpringPendingAnnotation, spring_route_annotations_and_tail};
use java::{
    java_code_lines_without_comments, line_declares_java_type,
    line_declares_nested_java_helper_type, parse_java_method_def, update_java_brace_depth,
};
use materialize::{
    SpringClassPrefix, pending_request_mapping_can_be_prefix, pending_request_mapping_prefixes,
    record_spring_pending_routes,
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

struct SpringNestedTypeScope {
    restore_at_depth: usize,
    class_prefixes: Vec<SpringClassPrefix>,
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
