use std::collections::BTreeSet;

use super::super::RouteCandidate;
use super::annotations::{SpringAnnotationKind, SpringPendingAnnotation};

#[derive(Clone)]
pub(super) struct SpringClassPrefix {
    pub(super) url: String,
    pub(super) http_method: String,
}

pub(super) fn record_spring_pending_routes(
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

pub(super) fn pending_request_mapping_can_be_prefix(
    annotations: &[SpringPendingAnnotation],
) -> bool {
    !annotations.is_empty()
        && annotations
            .iter()
            .all(|annotation| annotation.kind == SpringAnnotationKind::RequestMapping)
}

pub(super) fn pending_request_mapping_prefixes(
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
#[path = "materialize_tests.rs"]
mod tests;
