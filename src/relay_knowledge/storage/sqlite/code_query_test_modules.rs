use super::*;

#[path = "code_query_unit_tests.rs"]
mod tests;

#[path = "code_query_score_tests.rs"]
mod score_tests;

#[path = "code_query_identity_tests.rs"]
mod identity_tests;

#[path = "code_query_hybrid_symbol_planner_tests.rs"]
mod hybrid_symbol_planner_tests;

#[path = "code_query_hybrid_chunk_gate_tests.rs"]
mod hybrid_chunk_gate_tests;

#[path = "code_query_call_ranking_tests.rs"]
mod call_ranking_tests;

#[path = "code_query_indirect_call_tests.rs"]
mod indirect_call_tests;

#[path = "code_query_chunk_ranking_tests.rs"]
mod chunk_ranking_tests;

#[path = "code_query_symbol_ranking_tests.rs"]
mod symbol_ranking_tests;

#[path = "code_query_definition_fallback_tests.rs"]
mod definition_fallback_tests;

#[path = "code_query_reference_ranking_tests.rs"]
mod reference_ranking_tests;

#[path = "code_query_excerpt_tests.rs"]
mod excerpt_tests;
