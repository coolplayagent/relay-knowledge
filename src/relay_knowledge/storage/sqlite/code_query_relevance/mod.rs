//! Code query relevance scoring, identity matching, filtering, and candidate planning.

mod call_scoring;
mod candidate_plan;
mod conversion_scoring;
mod declaration_scoring;
mod filters;
mod fts;
mod symbol_identity;
mod symbol_scoring;
mod text_scoring;
mod tokens;

pub(in crate::storage::sqlite::code::code_query) use call_scoring::{
    call_edge_confidence_bonus, callee_related_name_bonus, directional_call_context_bonus,
    repeated_call_site_bonus, same_named_caller_penalty,
};
#[cfg(test)]
pub(in crate::storage::sqlite::code::code_query) use candidate_plan::candidate_condition;
pub(in crate::storage::sqlite::code::code_query) use candidate_plan::{
    CandidateLayer, candidate_limit, candidate_patterns, chunk_layers_for_request,
    fts_values_for_limited_with_language,
};
pub(in crate::storage::sqlite::code::code_query) use declaration_scoring::declaration_chunk_bonus;
pub(in crate::storage::sqlite::code::code_query) use filters::*;
pub(in crate::storage::sqlite::code::code_query) use fts::{
    compound_hybrid_chunk_fts_match_query, direct_hybrid_chunk_fts_match_query,
    focused_hybrid_chunk_fts_match_query, focused_symbol_fts_match_query, fts_match_query,
    hybrid_chunk_fts_match_query, lifecycle_hybrid_chunk_fts_match_query,
    strict_hybrid_chunk_fts_match_query, structured_hybrid_chunk_fts_match_query,
    symbol_fts_match_query,
};
pub(in crate::storage::sqlite::code::code_query) use symbol_identity::{
    SymbolIdentityQuery, query_is_single_symbol_identity,
};
#[cfg(test)]
pub(in crate::storage::sqlite::code::code_query) use symbol_scoring::symbol_name_query_bonus;
pub(in crate::storage::sqlite::code::code_query) use symbol_scoring::{
    scoped_identity_query_bonus, symbol_excerpt, symbol_kind_bonus, symbol_query_bonus,
};
pub(in crate::storage::sqlite::code::code_query) use text_scoring::{
    ScoreQuery, score_exact_path, score_text,
};
pub(in crate::storage::sqlite::code::code_query) use tokens::{escape_sql_like, query_terms};
