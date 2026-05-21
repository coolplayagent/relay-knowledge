use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::code_query_support::SymbolIdentityQuery;

const HIGH_CONFIDENCE_INFERRED_TARGET_BONUS: f64 = 1.15;
const HIGH_CONFIDENCE_INFERRED_TARGET_MIN_BPS: u16 = 7_000;

pub(super) fn high_confidence_inferred_target_bonus(
    base_score: f64,
    query: &str,
    callee_name: &str,
    target_hint: &str,
    resolution_state: &str,
    confidence_basis_points: u16,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Callers
        || resolution_state != "inferred"
        || confidence_basis_points < HIGH_CONFIDENCE_INFERRED_TARGET_MIN_BPS
        || target_hint.trim().is_empty()
    {
        return 0.0;
    }
    let target_leaf = target_identity_leaf(target_hint);
    if target_leaf == callee_name {
        return 0.0;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(query) else {
        return 0.0;
    };
    if identity.matches_symbol(target_leaf, target_hint, "", target_hint) {
        HIGH_CONFIDENCE_INFERRED_TARGET_BONUS
    } else {
        0.0
    }
}

fn target_identity_leaf(target_hint: &str) -> &str {
    target_hint
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
        .unwrap_or(target_hint)
}
