use std::collections::{BTreeMap, BTreeSet};

use super::arguments::{
    extract_python_router_argument, python_prefix_argument, trim_one_trailing_paren,
};

#[cfg(test)]
#[path = "routers_tests.rs"]
mod tests;

#[derive(Clone)]
pub(super) struct PythonRouterInfo {
    pub(super) local_prefix: String,
    pub(super) mount_prefixes: BTreeSet<String>,
    pub(super) framework: String,
    pub(super) mount_required: bool,
    pub(super) cross_file_mount_candidate: bool,
}

pub(super) fn parse_python_router_prefix(line: &str) -> Option<(String, PythonRouterInfo)> {
    let (left, right) = line.split_once('=')?;
    let router_name = python_assignment_name(left)?;
    if router_name.is_empty()
        || !router_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    let router_info = if let Some(args) = python_call_arguments(right, "APIRouter(") {
        let local_prefix = python_prefix_argument(args, "prefix");
        PythonRouterInfo {
            cross_file_mount_candidate: python_router_name_is_cross_file_candidate(
                &router_name,
                &local_prefix,
            ),
            local_prefix,
            mount_prefixes: BTreeSet::new(),
            framework: "fastapi".to_owned(),
            mount_required: true,
        }
    } else if python_call_arguments(right, "FastAPI(").is_some() {
        PythonRouterInfo {
            local_prefix: String::new(),
            mount_prefixes: BTreeSet::new(),
            framework: "fastapi".to_owned(),
            mount_required: false,
            cross_file_mount_candidate: false,
        }
    } else {
        let args = python_call_arguments(right, "Blueprint(")?;
        let local_prefix = python_prefix_argument(args, "url_prefix");
        PythonRouterInfo {
            cross_file_mount_candidate: python_router_name_is_cross_file_candidate(
                &router_name,
                &local_prefix,
            ),
            local_prefix,
            mount_prefixes: BTreeSet::new(),
            framework: "flask".to_owned(),
            mount_required: true,
        }
    };

    Some((router_name, router_info))
}

fn python_call_arguments<'a>(source: &'a str, marker: &str) -> Option<&'a str> {
    let start = source.find(marker)? + marker.len();
    Some(trim_one_trailing_paren(&source[start..]))
}

pub(super) fn merge_python_router_declaration(
    routers: &mut BTreeMap<String, PythonRouterInfo>,
    router_name: String,
    mut router_info: PythonRouterInfo,
) {
    if let Some(existing) = routers.remove(&router_name) {
        router_info.mount_prefixes = existing.mount_prefixes;
    }
    routers.insert(router_name, router_info);
}

fn python_router_name_is_cross_file_candidate(router_name: &str, local_prefix: &str) -> bool {
    (router_name == "router" && !local_prefix.is_empty())
        || (!matches!(router_name, "bp" | "blueprint")
            && (router_name.ends_with("_router")
                || router_name.ends_with("_blueprint")
                || router_name.ends_with("Router")
                || router_name.ends_with("Blueprint")))
}

pub(super) fn apply_python_include_router_prefix(
    statement: &str,
    routers: &mut BTreeMap<String, PythonRouterInfo>,
) -> bool {
    let Some(paren_pos) = statement.find(".include_router(") else {
        return false;
    };
    let args = trim_one_trailing_paren(&statement[paren_pos + ".include_router(".len()..]);
    let Some(router_name) = extract_python_router_argument(args, "router") else {
        return false;
    };
    let prefix = python_prefix_argument(args, "prefix");
    let router_info = routers
        .entry(router_name)
        .or_insert_with(|| PythonRouterInfo {
            local_prefix: String::new(),
            mount_prefixes: BTreeSet::new(),
            framework: "fastapi".to_owned(),
            mount_required: true,
            cross_file_mount_candidate: false,
        });
    router_info.mount_prefixes.insert(prefix);
    router_info.framework = "fastapi".to_owned();
    true
}

pub(super) fn apply_python_register_blueprint_prefix(
    statement: &str,
    routers: &mut BTreeMap<String, PythonRouterInfo>,
) -> bool {
    let Some(paren_pos) = statement.find(".register_blueprint(") else {
        return false;
    };
    let args = trim_one_trailing_paren(&statement[paren_pos + ".register_blueprint(".len()..]);
    let Some(blueprint_name) = extract_python_router_argument(args, "blueprint") else {
        return false;
    };
    let prefix = python_prefix_argument(args, "url_prefix");
    let router_info = routers
        .entry(blueprint_name)
        .or_insert_with(|| PythonRouterInfo {
            local_prefix: String::new(),
            mount_prefixes: BTreeSet::new(),
            framework: "flask".to_owned(),
            mount_required: true,
            cross_file_mount_candidate: false,
        });
    router_info.mount_prefixes.insert(prefix);
    router_info.framework = "flask".to_owned();
    true
}

pub(super) fn route_framework(
    func_part: &str,
    receiver_name: Option<&str>,
    routers: &BTreeMap<String, PythonRouterInfo>,
) -> String {
    if let Some(receiver_name) = receiver_name {
        if let Some(router_info) = routers.get(receiver_name) {
            return router_info.framework.clone();
        }
    }
    if func_part.ends_with(".api_route") {
        return "fastapi".to_owned();
    }
    "flask".to_owned()
}

fn python_assignment_name(left: &str) -> Option<String> {
    let name = left
        .trim()
        .split_once(':')
        .map_or(left.trim(), |(name, _)| name.trim());
    if name.is_empty() {
        return None;
    }
    Some(name.to_owned())
}
