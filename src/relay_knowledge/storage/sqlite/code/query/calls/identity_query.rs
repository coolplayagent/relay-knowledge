use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::{
    super::{
        relevance::{CandidateLayer, SymbolIdentityQuery, candidate_limit},
        rows::CallRow,
    },
    identity::specific_call_identity_leaf,
};

pub(super) struct CallIdentityQuery {
    pub(super) direction: CallIdentityDirection,
    symbol: SymbolIdentityQuery,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CallIdentityDirection {
    Caller,
    Callee,
}

impl CallIdentityQuery {
    pub(super) fn leaf_name(&self) -> &str {
        self.symbol.leaf_name()
    }

    fn is_scoped(&self) -> bool {
        self.symbol.is_scoped()
    }

    pub(super) fn match_column(&self) -> &'static str {
        match self.direction {
            CallIdentityDirection::Caller => "c.caller_name",
            CallIdentityDirection::Callee => "c.callee_name",
        }
    }

    pub(super) fn matches_row(&self, row: &CallRow) -> bool {
        match self.direction {
            CallIdentityDirection::Caller => self.symbol.matches_symbol(
                row.caller_name.as_deref().unwrap_or_default(),
                row.caller_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
                row.caller_signature.as_deref().unwrap_or_default(),
                row.caller_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
            ),
            CallIdentityDirection::Callee => self.symbol.matches_symbol(
                &row.callee_name,
                row.target_hint.as_deref().unwrap_or_default(),
                row.callee_signature.as_deref().unwrap_or_default(),
                row.callee_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
            ),
        }
    }
}

pub(super) fn call_identity_query(request: &CodeRetrievalRequest) -> Option<CallIdentityQuery> {
    let direction = match request.code_query_kind {
        CodeQueryKind::Callers => CallIdentityDirection::Callee,
        CodeQueryKind::Callees => CallIdentityDirection::Caller,
        _ => return None,
    };
    let symbol = SymbolIdentityQuery::from_query(&request.query)?;

    Some(CallIdentityQuery { direction, symbol })
}

pub(super) fn call_identity_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    identity: &CallIdentityQuery,
    hit_count: usize,
    saturated: bool,
) -> bool {
    hit_count > 0
        && !saturated
        && matches!(
            request.code_query_kind,
            CodeQueryKind::Callers | CodeQueryKind::Callees
        )
        && (identity.is_scoped()
            || (hit_count <= request.limit
                && call_identity_leaf_or_selector_is_specific(request, identity)))
}

pub(super) fn call_identity_leaf_or_selector_is_specific(
    request: &CodeRetrievalRequest,
    identity: &CallIdentityQuery,
) -> bool {
    specific_call_identity_leaf(identity.leaf_name())
        || !request.repository.path_filters.is_empty()
        || !request.repository.language_filters.is_empty()
}

pub(super) fn call_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Call).min(200)
}

#[cfg(test)]
#[path = "identity_query_tests.rs"]
mod tests;
