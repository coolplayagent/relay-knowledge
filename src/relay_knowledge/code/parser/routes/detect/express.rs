use std::collections::BTreeSet;

use super::RouteCandidate;
use super::shared::{extract_handler_name, extract_quoted_string};

const MAX_EXPRESS_ROUTE_REGISTRATION_LINES: usize = 12;

pub(in crate::code::parser) fn detect_express_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let lines: Vec<&str> = content.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if express_method_position(trimmed).is_none() {
            continue;
        };
        let statement = express_route_statement(&lines, index);
        let Some(method_pos) = express_method_position(&statement) else {
            continue;
        };
        if !receiver_looks_like_express_router(&statement[..method_pos]) {
            continue;
        }
        let rest = &statement[method_pos..];
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

fn express_route_statement(lines: &[&str], start: usize) -> String {
    let mut statement = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut saw_route_call = false;
    for (offset, line) in lines
        .iter()
        .skip(start)
        .take(MAX_EXPRESS_ROUTE_REGISTRATION_LINES)
        .enumerate()
    {
        let segment = line.trim();
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        let scan_start = if offset == 0 {
            route_method_open_position(segment).unwrap_or(0)
        } else {
            0
        };
        if route_call_is_closed(
            &segment[scan_start..],
            &mut depth,
            &mut quote,
            &mut escaped,
            &mut saw_route_call,
        ) {
            break;
        }
    }
    statement
}

fn route_method_open_position(line: &str) -> Option<usize> {
    let method_pos = express_method_position(line)?;
    let open_relative_pos = line[method_pos..].find('(')?;
    Some(method_pos + open_relative_pos)
}

fn route_call_is_closed(
    segment: &str,
    depth: &mut usize,
    quote: &mut Option<char>,
    escaped: &mut bool,
    saw_route_call: &mut bool,
) -> bool {
    for character in segment.chars() {
        if let Some(quote_char) = quote {
            if *escaped {
                *escaped = false;
                continue;
            }
            if character == '\\' {
                *escaped = true;
                continue;
            }
            if character == *quote_char {
                *quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => *quote = Some(character),
            '(' => {
                *depth += 1;
                *saw_route_call = true;
            }
            ')' => {
                *depth = depth.saturating_sub(1);
                if *saw_route_call && *depth == 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
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
