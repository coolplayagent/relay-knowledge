use std::collections::BTreeMap;

use super::RouteCandidate;

mod arguments;
mod materialize;
mod python_lexical;
mod routers;
mod statements;

use arguments::{
    extract_methods_from_flask_args, extract_python_add_url_rule_positional_handler,
    extract_python_keyword_value, extract_python_route_path, parse_flask_methods_decorator,
    python_handler_name_from_value, trim_one_trailing_paren,
};
use materialize::{PythonRouteBinding, materialize_python_routes};
use python_lexical::python_code_lines_without_triple_quoted_strings;
use routers::{
    PythonRouterInfo, apply_python_include_router_prefix, apply_python_register_blueprint_prefix,
    merge_python_router_declaration, parse_python_router_prefix, route_framework,
};
use statements::{
    flask_decorator_statement, python_add_url_rule_statement, python_include_router_statement,
    python_register_blueprint_statement, python_router_prefix_statement,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(in crate::code::parser) fn detect_flask_routes(content: &str) -> Vec<RouteCandidate> {
    let mut route_bindings = Vec::new();
    let mut pending_routes = Vec::<FlaskRouteInfo>::new();
    let mut routers = BTreeMap::<String, PythonRouterInfo>::new();
    let lines = python_code_lines_without_triple_quoted_strings(content);
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some((prefix_statement, prefix_lines)) =
            python_router_prefix_statement(&lines, index)
        {
            if let Some((router_name, router_info)) = parse_python_router_prefix(&prefix_statement)
            {
                merge_python_router_declaration(&mut routers, router_name, router_info);
                index += prefix_lines;
                continue;
            }
        }
        if let Some((include_statement, include_lines)) =
            python_include_router_statement(&lines, index)
        {
            if apply_python_include_router_prefix(&include_statement, &mut routers) {
                index += include_lines;
                continue;
            }
        }
        if let Some((blueprint_statement, blueprint_lines)) =
            python_register_blueprint_statement(&lines, index)
        {
            if apply_python_register_blueprint_prefix(&blueprint_statement, &mut routers) {
                index += blueprint_lines;
                continue;
            }
        }
        if let Some((url_rule_statement, url_rule_lines)) =
            python_add_url_rule_statement(&lines, index)
        {
            if let Some(bindings) = parse_python_add_url_rule(&url_rule_statement, &routers, index)
            {
                route_bindings.extend(bindings);
                index += url_rule_lines;
                continue;
            }
        }
        if trimmed.starts_with("@") {
            let (decorator_statement, decorator_lines) = flask_decorator_statement(&lines, index);
            if let Some(route_info) = parse_flask_decorator(&decorator_statement, &routers) {
                pending_routes.push(route_info);
                index += decorator_lines;
                continue;
            }
            if let Some(route_info) = pending_routes.last_mut() {
                if let Some(methods) = parse_flask_methods_decorator(&decorator_statement) {
                    route_info.methods = methods;
                    index += decorator_lines;
                    continue;
                }
            }
            index += decorator_lines;
            continue;
        }
        if !pending_routes.is_empty() {
            if let Some(func_name) = parse_python_function_def(trimmed) {
                let handler = func_name;
                for route_info in pending_routes.drain(..) {
                    let methods = if route_info.methods.is_empty() {
                        vec!["get".to_owned()]
                    } else {
                        route_info.methods
                    };
                    for method in methods {
                        route_bindings.push(PythonRouteBinding {
                            receiver_name: route_info.receiver_name.clone(),
                            local_url: route_info.local_url.clone(),
                            http_method: method,
                            handler_name: handler.clone(),
                            framework: route_info.framework.clone(),
                            line: index + 1,
                        });
                    }
                }
            } else if !trimmed.is_empty() {
                pending_routes.clear();
            }
        }
        index += 1;
    }
    materialize_python_routes(route_bindings, &routers)
}

struct FlaskRouteInfo {
    receiver_name: Option<String>,
    local_url: String,
    methods: Vec<String>,
    framework: String,
}

fn parse_flask_decorator(
    line: &str,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> Option<FlaskRouteInfo> {
    let line = line.trim_start_matches('@');
    let paren_pos = line.find('(')?;
    let (func_part, args) = (&line[..paren_pos], &line[paren_pos + 1..]);
    let route_method = extract_flask_http_method(func_part);
    let is_route = func_line_matches_route(func_part);
    if !is_route {
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

fn parse_python_add_url_rule(
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
        .unwrap_or_else(|| super::ANONYMOUS_ROUTE_HANDLER_NAME.to_owned());
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
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_def.len());
    let name = &after_def[..name_end];
    if name.is_empty() {
        return None;
    }
    Some(name.to_owned())
}
