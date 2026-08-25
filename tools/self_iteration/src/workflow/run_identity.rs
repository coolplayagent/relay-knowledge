use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(0);

pub(super) fn new_run_id() -> String {
    format!("run-{}", unique_suffix())
}

pub(crate) fn new_layer_run_id(layer: &str) -> String {
    format!("run-{}-{layer}", unique_suffix())
}

pub(super) fn new_manual_evaluate_run_id() -> String {
    format!("manual-evaluate-{}", unique_suffix())
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed);
    format!("{nanos}-{sequence}-{}", std::process::id())
}

pub(crate) fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

#[cfg(test)]
#[path = "run_identity_tests.rs"]
mod run_identity_tests;
