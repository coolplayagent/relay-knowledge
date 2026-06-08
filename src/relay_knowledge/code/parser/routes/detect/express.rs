use std::collections::{BTreeMap, BTreeSet};

use super::RouteCandidate;
use super::shared::{
    extract_handler_name, extract_handler_name_from_arguments, extract_quoted_string,
};

const MAX_EXPRESS_ROUTE_REGISTRATION_LINES: usize = 12;

pub(in crate::code::parser) fn detect_express_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut router_prefixes = BTreeMap::<String, String>::new();
    let lines: Vec<&str> = content.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some((router_name, prefix)) = parse_express_router_mount(trimmed, &router_prefixes) {
            router_prefixes.insert(router_name, prefix);
            continue;
        }
        if express_method_position(trimmed).is_none() {
            continue;
        };
        let statement = express_route_statement(&lines, index);
        if record_express_route_chain(
            &statement,
            index + 1,
            &router_prefixes,
            &mut seen,
            &mut routes,
        ) {
            continue;
        }
        let Some(method_pos) = express_method_position(&statement) else {
            continue;
        };
        let Some(receiver_name) = express_receiver_name(&statement[..method_pos]) else {
            continue;
        };
        if !express_router_name_is_router(&receiver_name) {
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
        let url = route_url_with_router_prefix(&receiver_name, &url, &router_prefixes);
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

fn parse_express_router_mount(
    line: &str,
    router_prefixes: &BTreeMap<String, String>,
) -> Option<(String, String)> {
    let use_pos = line.find(".use(")?;
    let receiver_name = express_receiver_name(&line[..use_pos])?;
    if !express_router_name_is_router(&receiver_name) {
        return None;
    }
    let after_use = line[use_pos..].split_once('(')?.1.trim_start();
    let mount_path = extract_quoted_string(after_use)?;
    if !mount_path.starts_with('/') {
        return None;
    }
    let router_name = extract_handler_name(after_use)?;
    if !express_router_name_is_router(&router_name) {
        return None;
    }
    let receiver_prefix = router_prefixes
        .get(&receiver_name)
        .map_or("", String::as_str);
    let prefix = merge_url_parts(receiver_prefix, &mount_path);
    Some((router_name, prefix))
}

fn record_express_route_chain(
    statement: &str,
    line: usize,
    router_prefixes: &BTreeMap<String, String>,
    seen: &mut BTreeSet<(String, String)>,
    routes: &mut Vec<RouteCandidate>,
) -> bool {
    let Some(route_pos) = statement.find(".route(") else {
        return false;
    };
    let Some(receiver_name) = express_receiver_name(&statement[..route_pos]) else {
        return false;
    };
    if !express_router_name_is_router(&receiver_name) {
        return false;
    }
    let after_route = &statement[route_pos + ".route(".len()..];
    let Some(local_url) = extract_quoted_string(after_route) else {
        return false;
    };
    if !local_url.starts_with('/') && !local_url.starts_with("${") {
        return false;
    }
    let url = route_url_with_router_prefix(&receiver_name, &local_url, router_prefixes);
    let mut found_route_method = false;
    let mut scan = after_route;
    while let Some(method_pos) = express_method_position(scan) {
        let rest = &scan[method_pos..];
        let Some((method_part, after_method)) = rest.split_once('(') else {
            break;
        };
        let raw_method = method_part.rsplit('.').next().unwrap_or("");
        let http_method = match raw_method.to_ascii_lowercase().as_str() {
            "get" | "post" | "put" | "delete" | "patch" => raw_method.to_ascii_lowercase(),
            _ => {
                scan = &rest[method_part.len()..];
                continue;
            }
        };
        found_route_method = true;
        let handler = extract_handler_name_from_arguments(after_method);
        let key = (url.clone(), http_method.clone());
        if seen.insert(key) {
            routes.push(RouteCandidate {
                url: url.clone(),
                http_method,
                handler_name: handler.unwrap_or_else(|| "anonymous".to_owned()),
                framework: "express".to_owned(),
                line,
            });
        }
        scan = after_method;
    }
    found_route_method
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

fn express_receiver_name(receiver: &str) -> Option<String> {
    receiver
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
        .map(str::to_owned)
}

fn express_router_name_is_router(receiver_name: &str) -> bool {
    let receiver_name = receiver_name.to_ascii_lowercase();

    receiver_name == "app" || receiver_name == "router" || receiver_name.ends_with("router")
}

fn route_url_with_router_prefix(
    receiver_name: &str,
    url: &str,
    router_prefixes: &BTreeMap<String, String>,
) -> String {
    let Some(prefix) = router_prefixes.get(receiver_name) else {
        return url.to_owned();
    };
    merge_url_parts(prefix, url)
}

fn merge_url_parts(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        return if suffix.starts_with('/') {
            suffix.to_owned()
        } else {
            format!("/{suffix}")
        };
    }
    if suffix.is_empty() {
        return prefix.to_owned();
    }
    let prefix = prefix.trim_end_matches('/');
    let suffix = suffix.trim_start_matches('/');
    format!("{prefix}/{suffix}")
}
