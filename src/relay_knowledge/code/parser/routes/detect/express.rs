use std::collections::BTreeSet;

use super::RouteCandidate;
use super::shared::{extract_handler_name, extract_quoted_string};

pub(in crate::code::parser) fn detect_express_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(method_pos) = express_method_position(trimmed) else {
            continue;
        };
        if !receiver_looks_like_express_router(&trimmed[..method_pos]) {
            continue;
        }
        let rest = &trimmed[method_pos..];
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

fn express_method_position(line: &str) -> Option<usize> {
    [".get(", ".post(", ".put(", ".delete(", ".patch("]
        .into_iter()
        .filter_map(|method| line.find(method))
        .min()
}

fn receiver_looks_like_express_router(receiver: &str) -> bool {
    let receiver_name = receiver
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
        .unwrap_or_default();
    let receiver_name = receiver_name.to_ascii_lowercase();

    receiver_name == "app" || receiver_name == "router" || receiver_name.ends_with("router")
}
