use std::collections::BTreeSet;

mod arguments;
mod bindings;
mod materialize;
mod mounts;
mod registrations;
mod syntax;

use super::RouteCandidate;
use super::javascript::{
    find_javascript_pattern_outside_strings, javascript_code_lines_without_comments,
    statement_ends_with_semicolon,
};
use bindings::{
    express_namespace_names, express_router_factory_names, parse_express_application_alias,
    parse_express_router_alias,
};
use materialize::materialize_express_routes;
use mounts::parse_express_router_mounts;
use registrations::{record_express_method_calls, record_express_route_chain};
use syntax::express_route_start_position;

const MAX_EXPRESS_ROUTE_REGISTRATION_LINES: usize = 12;

pub(in crate::code::parser) fn detect_express_routes(content: &str) -> Vec<RouteCandidate> {
    let mut route_infos = Vec::new();
    let mut mounts = Vec::new();
    let mut router_names = BTreeSet::<String>::from(["app".to_owned(), "router".to_owned()]);
    let mut root_receiver_names = router_names.clone();
    let express_names = express_namespace_names(content);
    let router_factory_names = express_router_factory_names(content);
    let lines = javascript_code_lines_without_comments(content);
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(application_name) = parse_express_application_alias(trimmed, &express_names) {
            router_names.insert(application_name.clone());
            root_receiver_names.insert(application_name);
        } else if let Some(router_name) =
            parse_express_router_alias(trimmed, &router_factory_names, &express_names)
        {
            root_receiver_names.remove(&router_name);
            router_names.insert(router_name);
        }
        let mount_statement;
        let mount_source = if find_javascript_pattern_outside_strings(trimmed, ".use(").is_some() {
            mount_statement = express_use_statement(&lines, index);
            mount_statement.as_str()
        } else {
            trimmed
        };
        let parsed_mounts = parse_express_router_mounts(mount_source, &router_names);
        if !parsed_mounts.is_empty() {
            for mount in parsed_mounts {
                router_names.insert(mount.router_name.clone());
                mounts.push(mount);
            }
        }
        if express_route_start_position(trimmed).is_none() {
            continue;
        };
        let statement = express_route_statement(&lines, index);
        let recorded_chain =
            record_express_route_chain(&statement, index + 1, &router_names, &mut route_infos);
        let recorded_methods =
            record_express_method_calls(&statement, index + 1, &router_names, &mut route_infos);
        if !recorded_chain && !recorded_methods {
            continue;
        }
    }
    materialize_express_routes(route_infos, &mounts, &root_receiver_names)
}

fn express_route_statement(lines: &[String], start: usize) -> String {
    if find_javascript_pattern_outside_strings(&lines[start], ".route(").is_some() {
        return express_route_chain_statement(lines, start);
    }
    express_method_call_statement(lines, start)
}

fn express_method_call_statement(lines: &[String], start: usize) -> String {
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
        if segment.is_empty() {
            continue;
        }
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

fn express_route_chain_statement(lines: &[String], start: usize) -> String {
    let mut statement = String::new();
    for (offset, line) in lines
        .iter()
        .skip(start)
        .take(MAX_EXPRESS_ROUTE_REGISTRATION_LINES)
        .enumerate()
    {
        let segment = line.trim();
        if segment.is_empty() {
            continue;
        }
        if offset > 0 && !segment.starts_with('.') {
            break;
        }
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        if statement_ends_with_semicolon(segment) {
            break;
        }
    }
    statement
}

fn express_use_statement(lines: &[String], start: usize) -> String {
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
        if segment.is_empty() {
            continue;
        }
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        let scan_start = if offset == 0 {
            find_javascript_pattern_outside_strings(segment, ".use(").unwrap_or(0)
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
    let method_pos = express_route_start_position(line)?;
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
