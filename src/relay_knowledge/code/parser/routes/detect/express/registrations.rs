use std::collections::BTreeSet;

use super::super::ANONYMOUS_ROUTE_HANDLER_NAME;
use super::super::lexical::javascript::find_javascript_pattern_outside_strings;
use super::arguments::{extract_handler_name, extract_handler_name_from_arguments};
use super::syntax::{
    express_http_method, express_method_position, express_receiver_name, express_route_urls,
    express_router_name_is_router, javascript_call_end,
};

pub(super) struct ExpressRouteInfo {
    pub(super) receiver_name: String,
    pub(super) local_url: String,
    pub(super) http_method: String,
    pub(super) handler_name: String,
    pub(super) line: usize,
}

pub(super) fn record_express_route_chain(
    statement: &str,
    line: usize,
    router_names: &BTreeSet<String>,
    route_infos: &mut Vec<ExpressRouteInfo>,
) -> bool {
    let mut found_route_method = false;
    let mut statement_scan = statement;
    while let Some(route_pos) = find_javascript_pattern_outside_strings(statement_scan, ".route(") {
        let Some(receiver_name) = express_receiver_name(&statement_scan[..route_pos]) else {
            statement_scan = &statement_scan[route_pos + ".route(".len()..];
            continue;
        };
        if !express_router_name_is_router(&receiver_name, router_names) {
            statement_scan = &statement_scan[route_pos + ".route(".len()..];
            continue;
        }
        let after_route = &statement_scan[route_pos + ".route(".len()..];
        let urls = express_route_urls(after_route);
        if urls.is_empty() {
            statement_scan = after_route;
            continue;
        }
        let mut chain_scan = after_route;
        let mut after_chain = after_route;
        while let Some(method_pos) = express_method_position(chain_scan) {
            let rest = &chain_scan[method_pos..];
            let Some((method_part, after_method)) = rest.split_once('(') else {
                break;
            };
            let next_scan = javascript_call_end(after_method)
                .and_then(|end| after_method.get(end..))
                .unwrap_or("");
            after_chain = next_scan;
            let raw_method = method_part.rsplit('.').next().unwrap_or("");
            let Some(http_method) = express_http_method(raw_method) else {
                let next_scan = next_scan.trim_start();
                if !next_scan.starts_with('.') {
                    break;
                }
                chain_scan = next_scan;
                continue;
            };
            found_route_method = true;
            let handler = extract_handler_name_from_arguments(after_method);
            for local_url in &urls {
                route_infos.push(ExpressRouteInfo {
                    receiver_name: receiver_name.clone(),
                    local_url: local_url.clone(),
                    http_method: http_method.clone(),
                    handler_name: handler
                        .clone()
                        .unwrap_or_else(|| ANONYMOUS_ROUTE_HANDLER_NAME.to_owned()),
                    line,
                });
            }
            let next_scan = next_scan.trim_start();
            if !next_scan.starts_with('.') {
                break;
            }
            chain_scan = next_scan;
        }
        statement_scan = after_chain;
    }
    found_route_method
}

pub(super) fn record_express_method_calls(
    statement: &str,
    line: usize,
    router_names: &BTreeSet<String>,
    route_infos: &mut Vec<ExpressRouteInfo>,
) -> bool {
    let mut found_route_method = false;
    let mut scan = statement;
    while let Some(method_pos) = express_method_position(scan) {
        let rest = &scan[method_pos..];
        let Some((method_part, after_method)) = rest.split_once('(') else {
            break;
        };
        let next_scan = javascript_call_end(after_method)
            .and_then(|end| after_method.get(end..))
            .unwrap_or("");
        let Some(receiver_name) = express_receiver_name(&scan[..method_pos]) else {
            scan = next_scan;
            continue;
        };
        if !express_router_name_is_router(&receiver_name, router_names) {
            scan = next_scan;
            continue;
        }
        let raw_method = method_part.rsplit('.').next().unwrap_or("");
        let Some(http_method) = express_http_method(raw_method) else {
            scan = next_scan;
            continue;
        };
        let after_method = after_method.trim_start();
        let urls = express_route_urls(after_method);
        if urls.is_empty() {
            scan = next_scan;
            continue;
        };
        found_route_method = true;
        let handler = extract_handler_name(after_method);
        for local_url in urls {
            route_infos.push(ExpressRouteInfo {
                receiver_name: receiver_name.clone(),
                local_url,
                http_method: http_method.clone(),
                handler_name: handler
                    .clone()
                    .unwrap_or_else(|| ANONYMOUS_ROUTE_HANDLER_NAME.to_owned()),
                line,
            });
        }
        scan = next_scan;
    }
    found_route_method
}

#[cfg(test)]
#[path = "registrations_tests.rs"]
mod tests;
