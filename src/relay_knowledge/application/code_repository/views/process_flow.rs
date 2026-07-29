use crate::domain::{CodeRouteRecord, CodebaseViewCall, CodebaseViewSnapshot};

use super::builder::{SectionRefs, ViewBuilder};

const PROCESS_FLOW_CALL_LIMIT: usize = 8;

pub(super) fn derive_process_flow(builder: &mut ViewBuilder, snapshot: &CodebaseViewSnapshot) {
    for route in snapshot.routes.iter().take(builder.limit) {
        let route_node_key = format!("route:{}", route.route_id);
        let handler_node_key = route_handler_node_id(route);
        let required_nodes =
            usize::from(builder.existing_node_id(route_node_key.clone()).is_none())
                + usize::from(builder.existing_node_id(handler_node_key.clone()).is_none());
        let route_evidence = builder.evidence(
            "route",
            &route.path,
            Some(route.handler_name.clone()),
            Some(route.line_range.clone()),
            Some(route.http_method.clone()),
            format!("{} {}", route.http_method, route.url),
        );
        if !builder.can_insert_nodes(required_nodes) {
            builder.mark_node_budget_truncated();
            let route_id = builder.node(
                route_node_key,
                format!("{} {}", route.http_method.to_uppercase(), route.url),
                "route",
                Some(route.path.clone()),
                0.86,
                Some(route_evidence.clone()),
            );
            if let Some(route_id) = route_id {
                builder.section(
                    format!("section:route:{}", route.route_id),
                    format!("{} {}", route.http_method.to_uppercase(), route.url),
                    format!(
                        "Request flow starts at route {} {}; handler details were omitted by the node limit.",
                        route.http_method.to_uppercase(),
                        route.url
                    ),
                    0.72,
                    SectionRefs {
                        node_ids: vec![route_id],
                        evidence_ids: vec![route_evidence],
                        diagnostics: vec![
                            "process-flow handler details truncated by node limit".to_owned(),
                        ],
                        ..SectionRefs::default()
                    },
                );
            }
            break;
        }
        let route_id = builder.node(
            route_node_key,
            format!("{} {}", route.http_method.to_uppercase(), route.url),
            "route",
            Some(route.path.clone()),
            0.86,
            Some(route_evidence.clone()),
        );
        let Some(route_id) = route_id else {
            break;
        };
        let matching_calls = snapshot
            .calls
            .iter()
            .filter(|call| call_belongs_to_route_flow(call, route))
            .collect::<Vec<_>>();
        let handler_path = route_handler_path(route, snapshot, &matching_calls);
        let handler_id = builder.node(
            handler_node_key,
            route.handler_name.clone(),
            "handler",
            Some(handler_path),
            0.82,
            Some(route_evidence.clone()),
        );
        let Some(handler_id) = handler_id else {
            break;
        };
        let mut edge_ids = Vec::new();
        if let Some(edge_id) = builder.edge(
            &route_id,
            &handler_id,
            "handled_by",
            0.86,
            Some(route_evidence.clone()),
        ) {
            edge_ids.push(edge_id);
        }
        let mut node_ids = vec![route_id, handler_id.clone()];
        let mut evidence_ids = vec![route_evidence];
        let mut diagnostics = Vec::new();
        if matching_calls.len() > PROCESS_FLOW_CALL_LIMIT {
            builder.mark_edge_budget_truncated();
            diagnostics.push(format!(
                "route handler calls truncated to {PROCESS_FLOW_CALL_LIMIT} matching calls"
            ));
        }
        for call in matching_calls.into_iter().take(PROCESS_FLOW_CALL_LIMIT) {
            let call_evidence = builder.evidence(
                "call",
                &call.call.path,
                call.call.caller_name.clone(),
                Some(call.call.line_range.clone()),
                Some(call.call.resolution_state.clone()),
                format!("handler call to {}", call.call.callee_name),
            );
            let callee_id = builder.node(
                call_target_node_id(call),
                call.call.callee_name.clone(),
                "call_target",
                call.callee_path.clone(),
                0.68,
                Some(call_evidence.clone()),
            );
            if let Some(callee_id) = callee_id {
                if let Some(edge_id) = builder.edge(
                    &handler_id,
                    &callee_id,
                    "calls",
                    0.68,
                    Some(call_evidence.clone()),
                ) {
                    edge_ids.push(edge_id);
                    node_ids.push(callee_id);
                }
            }
            evidence_ids.push(call_evidence);
        }
        builder.section(
            format!("section:route:{}", route.route_id),
            format!("{} {}", route.http_method.to_uppercase(), route.url),
            format!(
                "Request flow starts at route {} {} and reaches handler {}.",
                route.http_method.to_uppercase(),
                route.url,
                route.handler_name
            ),
            0.78,
            SectionRefs {
                node_ids,
                edge_ids,
                evidence_ids,
                diagnostics,
            },
        );
    }
}

fn call_target_node_id(call: &CodebaseViewCall) -> String {
    if let Some(symbol_id) = call.call.callee_symbol_snapshot_id.as_deref() {
        return format!("call_target:symbol:{symbol_id}");
    }
    if let Some(path) = call.callee_path.as_deref() {
        return format!("call_target:path:{path}:{}", call.call.callee_name);
    }
    format!("call_target:{}:{}", call.call.path, call.call.callee_name)
}

fn route_handler_node_id(route: &CodeRouteRecord) -> String {
    route
        .handler_symbol_snapshot_id
        .as_ref()
        .map(|symbol_id| format!("handler:symbol:{symbol_id}"))
        .unwrap_or_else(|| format!("handler:{}:{}", route.path, route.route_id))
}

fn route_handler_path(
    route: &CodeRouteRecord,
    snapshot: &CodebaseViewSnapshot,
    matching_calls: &[&CodebaseViewCall],
) -> String {
    if let Some(handler_symbol_id) = route.handler_symbol_snapshot_id.as_deref() {
        if let Some(symbol) = snapshot
            .symbols
            .iter()
            .find(|symbol| symbol.symbol_snapshot_id == handler_symbol_id)
        {
            return symbol.path.clone();
        }
        if let Some(call) = matching_calls
            .iter()
            .find(|call| call.call.caller_symbol_snapshot_id.as_deref() == Some(handler_symbol_id))
        {
            return call.call.path.clone();
        }
    }
    matching_calls
        .iter()
        .find(|call| call.call.path != route.path)
        .map(|call| call.call.path.clone())
        .unwrap_or_else(|| route.path.clone())
}

fn call_matches_route_handler(call: &CodebaseViewCall, route: &CodeRouteRecord) -> bool {
    if let (Some(caller_symbol_id), Some(handler_symbol_id)) = (
        call.call.caller_symbol_snapshot_id.as_deref(),
        route.handler_symbol_snapshot_id.as_deref(),
    ) {
        return caller_symbol_id == handler_symbol_id;
    }
    let Some(caller_name) = call.call.caller_name.as_deref() else {
        return false;
    };
    same_symbol_leaf(caller_name, &route.handler_name)
}

fn call_belongs_to_route_flow(call: &CodebaseViewCall, route: &CodeRouteRecord) -> bool {
    if call.call.path == route.path {
        return call_matches_route_handler(call, route);
    }
    if let (Some(caller_symbol_id), Some(handler_symbol_id)) = (
        call.call.caller_symbol_snapshot_id.as_deref(),
        route.handler_symbol_snapshot_id.as_deref(),
    ) {
        return caller_symbol_id == handler_symbol_id;
    }
    let Some(caller_name) = call.call.caller_name.as_deref() else {
        return false;
    };
    names_are_qualified(caller_name)
        && names_are_qualified(&route.handler_name)
        && symbol_leaf(caller_name) == symbol_leaf(&route.handler_name)
}

fn same_symbol_leaf(left: &str, right: &str) -> bool {
    left == right
        || symbol_leaf(left) == right
        || symbol_leaf(right) == left
        || symbol_leaf(left) == symbol_leaf(right)
}

fn symbol_leaf(name: &str) -> &str {
    name.rsplit([':', '.', '#', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(name)
}

fn names_are_qualified(name: &str) -> bool {
    name.contains([':', '.', '#', '/'])
}
