use std::collections::BTreeSet;

use super::super::javascript::find_javascript_pattern_outside_strings;
use super::arguments::extract_quoted_string;
use super::syntax::{
    express_receiver_name, express_router_name_is_router, extract_quoted_strings,
    javascript_array_literal_inner, javascript_call_end, javascript_top_level_arguments,
    route_url_literals,
};

pub(super) const DYNAMIC_EXPRESS_MOUNT_PREFIX: &str = "\0dynamic";

pub(super) struct ExpressRouterMount {
    pub(super) receiver_name: String,
    pub(super) router_name: String,
    pub(super) local_prefix: String,
}

pub(super) fn parse_express_router_mounts(
    line: &str,
    router_names: &BTreeSet<String>,
) -> Vec<ExpressRouterMount> {
    let mut mounts = Vec::new();
    let mut scan = line;
    while let Some(use_pos) = find_javascript_pattern_outside_strings(scan, ".use(") {
        mounts.extend(parse_express_router_mount_at(scan, use_pos, router_names));
        let Some(after_use) = scan[use_pos..]
            .split_once('(')
            .map(|(_, args)| args.trim_start())
        else {
            break;
        };
        let Some(call_end) = javascript_call_end(after_use) else {
            break;
        };
        scan = after_use.get(call_end..).unwrap_or("");
    }
    mounts
}

fn parse_express_router_mount_at(
    line: &str,
    use_pos: usize,
    router_names: &BTreeSet<String>,
) -> Vec<ExpressRouterMount> {
    let Some(receiver_name) = express_receiver_name(&line[..use_pos]) else {
        return Vec::new();
    };
    if !express_router_name_is_router(&receiver_name, router_names) {
        return Vec::new();
    }
    let Some(after_use) = line[use_pos..]
        .split_once('(')
        .map(|(_, args)| args.trim_start())
    else {
        return Vec::new();
    };
    let arguments = javascript_top_level_arguments(after_use);
    let Some(first_argument) = arguments.first() else {
        return Vec::new();
    };
    let (mount_paths, router_arguments) = if let Some(path) = extract_quoted_string(first_argument)
    {
        if path.contains("${") {
            (
                vec![DYNAMIC_EXPRESS_MOUNT_PREFIX.to_owned()],
                &arguments[1..],
            )
        } else if path.starts_with('/') {
            (vec![path], &arguments[1..])
        } else {
            return Vec::new();
        }
    } else if let Some(array_inner) = javascript_array_literal_inner(first_argument) {
        let path_values = extract_quoted_strings(array_inner);
        let paths = route_url_literals(path_values.iter().cloned());
        if paths.is_empty() {
            if path_values.iter().any(|path| path.contains("${")) {
                (
                    vec![DYNAMIC_EXPRESS_MOUNT_PREFIX.to_owned()],
                    &arguments[1..],
                )
            } else {
                (vec![String::new()], arguments.as_slice())
            }
        } else {
            (paths, &arguments[1..])
        }
    } else if express_receiver_name(first_argument).is_some_and(|router_name| {
        express_router_mount_argument_is_router(&router_name, router_names)
    }) {
        (vec![String::new()], arguments.as_slice())
    } else if arguments.len() > 1 && !express_use_argument_looks_like_dynamic_path(first_argument) {
        (vec![String::new()], &arguments[1..])
    } else {
        (
            vec![DYNAMIC_EXPRESS_MOUNT_PREFIX.to_owned()],
            &arguments[1..],
        )
    };
    express_router_mount_names(router_arguments, router_names)
        .into_iter()
        .flat_map(|router_name| {
            mount_paths.iter().map({
                let receiver_name = receiver_name.clone();
                move |mount_path| ExpressRouterMount {
                    receiver_name: receiver_name.clone(),
                    router_name: router_name.clone(),
                    local_prefix: mount_path.clone(),
                }
            })
        })
        .collect()
}

fn express_router_mount_names(arguments: &[&str], router_names: &BTreeSet<String>) -> Vec<String> {
    let mut names = BTreeSet::new();
    for argument in arguments {
        collect_express_router_mount_names(argument, router_names, &mut names);
    }
    names.into_iter().collect()
}

fn collect_express_router_mount_names(
    argument: &str,
    router_names: &BTreeSet<String>,
    names: &mut BTreeSet<String>,
) {
    if let Some(inner) = javascript_array_literal_inner(argument) {
        for nested_argument in javascript_top_level_arguments(inner) {
            collect_express_router_mount_names(nested_argument, router_names, names);
        }
        return;
    }
    let Some(router_name) = express_receiver_name(argument) else {
        return;
    };
    if express_router_mount_argument_is_router(&router_name, router_names) {
        names.insert(router_name);
    }
}

fn express_router_mount_argument_is_router(
    router_name: &str,
    router_names: &BTreeSet<String>,
) -> bool {
    if express_router_name_is_router(router_name, router_names) {
        return true;
    }
    router_name.to_ascii_lowercase().ends_with("router")
}

fn express_use_argument_looks_like_dynamic_path(argument: &str) -> bool {
    let Some(name) = express_receiver_name(argument) else {
        return true;
    };
    let name = name.to_ascii_lowercase();
    ["prefix", "path", "url", "route", "base", "mount"]
        .iter()
        .any(|marker| name.contains(marker))
}

#[cfg(test)]
#[path = "mounts_tests.rs"]
mod tests;
