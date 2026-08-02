use std::collections::BTreeMap;

use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::super::rows::CallRow;

type CalleeExecutionGroupKey = (String, String, u32, u32);
type CalleeExecutionSiteKey = (CalleeExecutionGroupKey, u32, u32, String, String);
pub(super) type CalleeExecutionOrder = BTreeMap<CalleeExecutionSiteKey, (usize, usize)>;

const CALLEE_EXECUTION_ORDER_STEP: f64 = 0.18;

pub(super) fn callee_execution_order(
    rows: &[CallRow],
    request: &CodeRetrievalRequest,
) -> CalleeExecutionOrder {
    if request.code_query_kind != CodeQueryKind::Callees {
        return BTreeMap::new();
    }

    let mut grouped = BTreeMap::<CalleeExecutionGroupKey, Vec<CalleeExecutionSiteKey>>::new();
    for row in rows {
        let Some(group_key) = callee_execution_group_key(row) else {
            continue;
        };
        let site_key = callee_execution_site_key(group_key.clone(), row);
        grouped.entry(group_key).or_default().push(site_key);
    }

    let mut order = BTreeMap::new();
    for sites in grouped.values_mut() {
        sites.sort();
        sites.dedup();
        if sites.len() <= 1 {
            continue;
        }
        let site_count = sites.len();
        for (position, site) in sites.iter().cloned().enumerate() {
            order.insert(site, (position, site_count));
        }
    }

    order
}

fn callee_execution_group_key(row: &CallRow) -> Option<CalleeExecutionGroupKey> {
    let caller = row
        .caller_symbol_snapshot_id
        .as_deref()
        .or(row.caller_name.as_deref())?;
    let (caller_start, caller_end) = row
        .caller_line_range
        .as_ref()
        .map_or((0, 0), |range| (range.start, range.end));

    Some((
        row.path.clone(),
        caller.to_owned(),
        caller_start,
        caller_end,
    ))
}

fn callee_execution_site_key(
    group_key: CalleeExecutionGroupKey,
    row: &CallRow,
) -> CalleeExecutionSiteKey {
    (
        group_key,
        row.line_range.start,
        row.line_range.end,
        row.callee_name.clone(),
        row.target_hint.clone().unwrap_or_default(),
    )
}

pub(super) fn callee_execution_order_bonus(
    order: &CalleeExecutionOrder,
    row: &CallRow,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Callees {
        return 0.0;
    }
    let Some(group_key) = callee_execution_group_key(row) else {
        return 0.0;
    };
    let site_key = callee_execution_site_key(group_key, row);
    let Some((position, site_count)) = order.get(&site_key) else {
        return 0.0;
    };

    site_count.saturating_sub(*position).min(5) as f64 * CALLEE_EXECUTION_ORDER_STEP
}

#[cfg(test)]
#[path = "execution_order_tests.rs"]
mod tests;
