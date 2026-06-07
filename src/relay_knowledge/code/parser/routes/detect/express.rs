use std::collections::BTreeSet;

use super::RouteCandidate;
use super::shared::{extract_handler_name, extract_quoted_string};

pub(in crate::code::parser) fn detect_express_routes(content: &str) -> Vec<RouteCandidate> {
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
        let raw_method = method_part.rsplit('.').next().unwrap_or("");
        let http_method = match raw_method.to_ascii_lowercase().as_str() {
            "get" | "post" | "put" | "delete" | "patch" => raw_method.to_ascii_lowercase(),
            _ => continue,
        };
        let after_method = after_method.trim_start();
        let url = if let Some(url) = extract_quoted_string(after_method) {
            url
        } else {
            continue;
        };
        if !url.starts_with('/') && !url.starts_with("${") {
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
