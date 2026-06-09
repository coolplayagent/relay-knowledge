use std::collections::{BTreeMap, BTreeSet};

use super::RouteCandidate;
use super::express::{
    DYNAMIC_EXPRESS_MOUNT_PREFIX, ExpressRouteInfo, ExpressRouterMount, merge_url_parts,
};

pub(super) fn materialize_express_routes(
    route_infos: Vec<ExpressRouteInfo>,
    mounts: &[ExpressRouterMount],
) -> Vec<RouteCandidate> {
    let router_prefixes = resolved_express_router_prefixes(mounts);
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    for route_info in route_infos {
        let prefixes = router_prefixes
            .get(&route_info.receiver_name)
            .cloned()
            .unwrap_or_else(|| BTreeSet::from([String::new()]));
        for prefix in prefixes {
            if prefix == DYNAMIC_EXPRESS_MOUNT_PREFIX {
                continue;
            }
            let url = merge_url_parts(&prefix, &route_info.local_url);
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
                    framework: "express".to_owned(),
                    line: route_info.line,
                });
            }
        }
    }
    routes
}

fn resolved_express_router_prefixes(
    mounts: &[ExpressRouterMount],
) -> BTreeMap<String, BTreeSet<String>> {
    let mounted_routers = mounts
        .iter()
        .map(|mount| mount.router_name.clone())
        .collect::<BTreeSet<_>>();
    let mut router_prefixes = BTreeMap::<String, BTreeSet<String>>::new();
    for _ in 0..=mounts.len() {
        let mut changed = false;
        for mount in mounts {
            let Some(receiver_prefixes) =
                express_receiver_prefixes(&mount.receiver_name, &router_prefixes, &mounted_routers)
            else {
                continue;
            };
            for receiver_prefix in receiver_prefixes {
                let prefix = if receiver_prefix == DYNAMIC_EXPRESS_MOUNT_PREFIX
                    || mount.local_prefix == DYNAMIC_EXPRESS_MOUNT_PREFIX
                {
                    DYNAMIC_EXPRESS_MOUNT_PREFIX.to_owned()
                } else {
                    merge_url_parts(&receiver_prefix, &mount.local_prefix)
                };
                if router_prefixes
                    .entry(mount.router_name.clone())
                    .or_default()
                    .insert(prefix)
                {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    router_prefixes
}

fn express_receiver_prefixes(
    receiver_name: &str,
    router_prefixes: &BTreeMap<String, BTreeSet<String>>,
    mounted_routers: &BTreeSet<String>,
) -> Option<BTreeSet<String>> {
    if let Some(prefixes) = router_prefixes.get(receiver_name) {
        return Some(prefixes.clone());
    }
    (!mounted_routers.contains(receiver_name)).then(|| BTreeSet::from([String::new()]))
}
