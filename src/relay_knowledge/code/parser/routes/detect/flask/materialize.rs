use std::collections::{BTreeMap, BTreeSet};

use super::arguments::DYNAMIC_PYTHON_MOUNT_PREFIX;
use super::routers::PythonRouterInfo;
use crate::code::parser::routes::detect::RouteCandidate;

#[cfg(test)]
#[path = "materialize_tests.rs"]
mod tests;

pub(super) struct PythonRouteBinding {
    pub(super) receiver_name: Option<String>,
    pub(super) local_url: String,
    pub(super) http_method: String,
    pub(super) handler_name: String,
    pub(super) framework: String,
    pub(super) line: usize,
}

pub(super) fn materialize_python_routes(
    route_bindings: Vec<PythonRouteBinding>,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    for route_info in route_bindings {
        for (url, framework) in python_route_urls_and_frameworks(&route_info, routers) {
            let key = (
                url.clone(),
                route_info.http_method.clone(),
                route_info.handler_name.clone(),
                route_info.line,
            );
            if seen.insert(key) {
                routes.push(RouteCandidate {
                    url,
                    http_method: route_info.http_method.clone(),
                    handler_name: route_info.handler_name.clone(),
                    framework,
                    line: route_info.line,
                });
            }
        }
    }
    routes
}

fn python_route_urls_and_frameworks(
    route_info: &PythonRouteBinding,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> Vec<(String, String)> {
    let Some(receiver_name) = route_info.receiver_name.as_deref() else {
        return vec![(route_info.local_url.clone(), route_info.framework.clone())];
    };
    let Some(router_info) = routers.get(receiver_name) else {
        return vec![(route_info.local_url.clone(), route_info.framework.clone())];
    };

    python_router_prefixes(router_info)
        .into_iter()
        .map(|prefix| {
            (
                merge_url_parts(&prefix, &route_info.local_url),
                router_info.framework.clone(),
            )
        })
        .collect()
}

fn python_router_prefixes(router_info: &PythonRouterInfo) -> BTreeSet<String> {
    if router_info.local_prefix == DYNAMIC_PYTHON_MOUNT_PREFIX {
        return BTreeSet::new();
    }
    if router_info.mount_prefixes.is_empty() {
        if router_info.mount_required {
            if router_info.cross_file_mount_candidate {
                return BTreeSet::from([merge_url_parts("/:mount", &router_info.local_prefix)]);
            }
            return BTreeSet::new();
        }
        return BTreeSet::from([router_info.local_prefix.clone()]);
    }
    router_info
        .mount_prefixes
        .iter()
        .filter(|mount_prefix| mount_prefix.as_str() != DYNAMIC_PYTHON_MOUNT_PREFIX)
        .map(|mount_prefix| merge_url_parts(mount_prefix, &router_info.local_prefix))
        .collect()
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
