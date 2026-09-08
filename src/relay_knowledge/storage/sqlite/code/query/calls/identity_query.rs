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
    symbol: Option<SymbolIdentityQuery>,
    pub(super) canonical_id: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CallIdentityDirection {
    Caller,
    Callee,
}

impl CallIdentityQuery {
    pub(super) fn leaf_name(&self) -> &str {
        self.symbol
            .as_ref()
            .map_or("", SymbolIdentityQuery::leaf_name)
    }

    fn is_scoped(&self) -> bool {
        self.canonical_id.is_some()
            || self
                .symbol
                .as_ref()
                .is_some_and(SymbolIdentityQuery::is_scoped)
    }

    pub(super) fn match_column(&self) -> &'static str {
        match (self.direction, self.canonical_id.is_some()) {
            (CallIdentityDirection::Caller, true) => "caller.canonical_symbol_id",
            (CallIdentityDirection::Callee, true) => "callee.canonical_symbol_id",
            (CallIdentityDirection::Caller, false) => "c.caller_name",
            (CallIdentityDirection::Callee, false) => "c.callee_name",
        }
    }

    pub(super) fn matches_row(&self, row: &CallRow) -> bool {
        if let Some(canonical_id) = &self.canonical_id {
            let actual = match self.direction {
                CallIdentityDirection::Caller => &row.caller_canonical_symbol_id,
                CallIdentityDirection::Callee => &row.callee_canonical_symbol_id,
            };
            return actual.as_ref() == Some(canonical_id);
        }
        let Some(symbol) = &self.symbol else {
            return false;
        };
        match self.direction {
            CallIdentityDirection::Caller => symbol.matches_symbol(
                row.caller_name.as_deref().unwrap_or_default(),
                row.caller_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
                row.caller_signature.as_deref().unwrap_or_default(),
                row.caller_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
            ),
            CallIdentityDirection::Callee => symbol.matches_symbol(
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
    let query = request.query.trim();
    let canonical_id = query.starts_with("repo://").then(|| query.to_owned());
    let symbol = if canonical_id.is_some() {
        None
    } else {
        Some(SymbolIdentityQuery::from_query(query)?)
    };
    Some(CallIdentityQuery {
        direction,
        symbol,
        canonical_id,
    })
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
