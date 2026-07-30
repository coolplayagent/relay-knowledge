use std::collections::BTreeMap;

use super::RouteCandidate;

mod arguments;
mod materialize;
mod python_lexical;
mod registrations;
mod routers;
mod statements;

use materialize::materialize_python_routes;
use python_lexical::python_code_lines_without_triple_quoted_strings;
use registrations::{
    FlaskRouteInfo, apply_flask_methods_decorator, bind_pending_routes_to_python_function,
    parse_flask_decorator, parse_python_add_url_rule,
};
use routers::{
    PythonRouterInfo, apply_python_include_router_prefix, apply_python_register_blueprint_prefix,
    merge_python_router_declaration, parse_python_router_prefix,
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
            if apply_flask_methods_decorator(&decorator_statement, &mut pending_routes) {
                index += decorator_lines;
                continue;
            }
            index += decorator_lines;
            continue;
        }
        if !pending_routes.is_empty() {
            if let Some(bindings) =
                bind_pending_routes_to_python_function(trimmed, &mut pending_routes, index + 1)
            {
                route_bindings.extend(bindings);
            } else if !trimmed.is_empty() {
                pending_routes.clear();
            }
        }
        index += 1;
    }
    materialize_python_routes(route_bindings, &routers)
}
