use std::collections::BTreeSet;

mod arguments;
mod bindings;
mod materialize;
mod mounts;
mod registrations;
mod statements;
mod syntax;

use super::RouteCandidate;
use super::javascript::{
    find_javascript_pattern_outside_strings, javascript_code_lines_without_comments,
};
use bindings::{
    express_namespace_names, express_router_factory_names, parse_express_application_alias,
    parse_express_router_alias,
};
use materialize::materialize_express_routes;
use mounts::parse_express_router_mounts;
use registrations::{record_express_method_calls, record_express_route_chain};
use statements::{express_route_statement, express_use_statement};
use syntax::express_route_start_position;

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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
