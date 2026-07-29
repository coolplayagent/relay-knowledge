use super::authorize_context_bytes;
use crate::{
    domain::{
        CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES, CODEGRAPH_CONTEXT_MAX_BYTES,
        CODEGRAPH_CONTEXT_MIN_BYTES,
    },
    interfaces::agent::AgentAdapterErrorKind,
};

#[test]
fn codegraph_context_bytes_default_to_valid_codegraph_budget() {
    let value = authorize_context_bytes(None, CODEGRAPH_CONTEXT_MAX_BYTES * 4, true)
        .expect("default codegraph budget should clamp to codegraph default");

    assert_eq!(value, CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES);
}

#[test]
fn codegraph_context_bytes_reject_explicit_values_outside_codegraph_bounds() {
    let too_small = authorize_context_bytes(Some(CODEGRAPH_CONTEXT_MIN_BYTES - 1), 1_000_000, true)
        .expect_err("small codegraph budget should be rejected");
    let too_large = authorize_context_bytes(Some(CODEGRAPH_CONTEXT_MAX_BYTES + 1), 1_000_000, true)
        .expect_err("large codegraph budget should be rejected");

    assert_eq!(too_small.kind, AgentAdapterErrorKind::InvalidArgument);
    assert_eq!(too_large.kind, AgentAdapterErrorKind::LimitExceeded);
}
