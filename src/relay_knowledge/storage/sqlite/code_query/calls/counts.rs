use std::collections::BTreeMap;

use super::code_query_rows::CallRow;

pub(super) type CallerTargetCallKey = (String, String, String, String);

pub(super) fn caller_target_call_counts(rows: &[CallRow]) -> BTreeMap<CallerTargetCallKey, usize> {
    let mut counts = BTreeMap::new();
    for row in rows {
        if let Some(key) = caller_target_call_key(row) {
            *counts.entry(key).or_insert(0) += 1;
        }
    }

    counts
}

pub(super) fn caller_target_call_key(row: &CallRow) -> Option<CallerTargetCallKey> {
    let caller_symbol_snapshot_id = row.caller_symbol_snapshot_id.as_ref()?;
    Some((
        caller_symbol_snapshot_id.clone(),
        row.callee_symbol_snapshot_id.clone().unwrap_or_default(),
        row.target_hint.clone().unwrap_or_default(),
        row.callee_name.clone(),
    ))
}
