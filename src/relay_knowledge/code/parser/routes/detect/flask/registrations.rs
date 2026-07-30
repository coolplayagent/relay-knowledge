use std::collections::BTreeMap;

use super::arguments::{
    extract_methods_from_flask_args, extract_python_add_url_rule_positional_handler,
    extract_python_keyword_value, extract_python_route_path, parse_flask_methods_decorator,
    python_handler_name_from_value, trim_one_trailing_paren,
};
use super::materialize::PythonRouteBinding;
use super::routers::{PythonRouterInfo, route_framework};

#[cfg(test)]
#[path = "registrations_tests.rs"]
mod tests;

pub(super) struct FlaskRouteInfo {
    receiver_name: Option<String>,
    local_url: String,
    methods: Vec<String>,
    framework: String,
}

pub(super) fn parse_flask_decorator(
    line: &str,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> Option<FlaskRouteInfo> {
    let line = line.trim_start_matches('@');
    let paren_pos = line.find('(')?;
    let (func_part, args) = (&line[..paren_pos], &line[paren_pos + 1..]);
    let route_method = extract_flask_http_method(func_part);
    if !func_line_matches_route(func_part) {
        return None;
    }
    let args_trimmed = trim_one_trailing_paren(args);
    let url = extract_python_route_path(args_trimmed)?;
    let receiver_name = python_decorator_receiver(func_part);
    let framework = route_framework(func_part, receiver_name.as_deref(), routers);
    let methods = if route_method.is_empty() {
        extract_methods_from_flask_args(args_trimmed)
    } else {
        vec![route_method]
    };
    Some(FlaskRouteInfo {
        receiver_name,
        local_url: url,
        methods,
        framework,
    })
}

pub(super) fn apply_flask_methods_decorator(
    line: &str,
    pending_routes: &mut [FlaskRouteInfo],
) -> bool {
    let Some(route_info) = pending_routes.last_mut() else {
        return false;
    };
    let Some(methods) = parse_flask_methods_decorator(line) else {
        return false;
    };
    route_info.methods = methods;
    true
}

pub(super) fn bind_pending_routes_to_python_function(
    line: &str,
    pending_routes: &mut Vec<FlaskRouteInfo>,
    line_number: usize,
) -> Option<Vec<PythonRouteBinding>> {
    let handler_name = parse_python_function_def(line)?;
    let mut bindings = Vec::new();
    for route_info in pending_routes.drain(..) {
        let methods = if route_info.methods.is_empty() {
            vec!["get".to_owned()]
        } else {
            route_info.methods
        };
        for http_method in methods {
            bindings.push(PythonRouteBinding {
                receiver_name: route_info.receiver_name.clone(),
                local_url: route_info.local_url.clone(),
                http_method,
                handler_name: handler_name.clone(),
                framework: route_info.framework.clone(),
                line: line_number,
            });
        }
    }
    Some(bindings)
}

pub(super) fn parse_python_add_url_rule(
    statement: &str,
    routers: &BTreeMap<String, PythonRouterInfo>,
    line_index: usize,
) -> Option<Vec<PythonRouteBinding>> {
    let paren_pos = statement.find(".add_url_rule(")?;
    let func_part = &statement[..paren_pos];
    let receiver_name = python_decorator_receiver(func_part);
    let args = trim_one_trailing_paren(&statement[paren_pos + ".add_url_rule(".len()..]);
    let local_url = extract_python_route_path(args)?;
    let methods = extract_methods_from_flask_args(args);
    let methods = if methods.is_empty() {
        vec!["get".to_owned()]
    } else {
        methods
    };
    let handler_name = extract_python_keyword_value(args, "view_func")
        .and_then(python_handler_name_from_value)
        .or_else(|| extract_python_add_url_rule_positional_handler(args))
        .unwrap_or_else(|| super::super::ANONYMOUS_ROUTE_HANDLER_NAME.to_owned());
    let framework = route_framework("add_url_rule", receiver_name.as_deref(), routers);
    Some(
        methods
            .into_iter()
            .map(|http_method| PythonRouteBinding {
                receiver_name: receiver_name.clone(),
                local_url: local_url.clone(),
                http_method,
                handler_name: handler_name.clone(),
                framework: framework.clone(),
                line: line_index + 1,
            })
            .collect(),
    )
}

fn python_decorator_receiver(func_part: &str) -> Option<String> {
    let (receiver, _) = func_part.rsplit_once('.')?;
    Some(receiver.rsplit('.').next().unwrap_or(receiver).to_owned())
}

fn extract_flask_http_method(func_part: &str) -> String {
    let base = func_part.rsplit('.').next().unwrap_or("");
    match base {
        "get" => "get".to_owned(),
        "post" => "post".to_owned(),
        "put" => "put".to_owned(),
        "delete" => "delete".to_owned(),
        "patch" => "patch".to_owned(),
        "head" => "head".to_owned(),
        "options" => "options".to_owned(),
        _ => String::new(),
    }
}

fn func_line_matches_route(func_part: &str) -> bool {
    func_part.ends_with(".route")
        || func_part.ends_with(".api_route")
        || func_part.ends_with(".get")
        || func_part.ends_with(".post")
        || func_part.ends_with(".put")
        || func_part.ends_with(".delete")
        || func_part.ends_with(".patch")
        || func_part.ends_with(".head")
        || func_part.ends_with(".options")
}

fn parse_python_function_def(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let after_def = trimmed
        .strip_prefix("def ")
        .or_else(|| trimmed.strip_prefix("async def "))?;
    let name_end = after_def
        .find(|character: char| character == '(' || character.is_whitespace())
        .unwrap_or(after_def.len());
    let name = &after_def[..name_end];
    if name.is_empty() {
        return None;
    }
    Some(name.to_owned())
}
