use super::*;

#[path = "unit.rs"]
mod tests;

#[path = "score.rs"]
mod score_tests;

#[path = "identity.rs"]
mod identity_tests;

#[path = "hybrid_symbol_planner.rs"]
mod hybrid_symbol_planner_tests;

#[path = "hybrid_chunk_gate.rs"]
mod hybrid_chunk_gate_tests;

#[path = "call_ranking.rs"]
mod call_ranking_tests;

#[path = "call_generated.rs"]
mod call_generated_tests;

#[path = "indirect_call.rs"]
mod indirect_call_tests;

#[path = "chunk_ranking.rs"]
mod chunk_ranking_tests;

#[path = "symbol_ranking.rs"]
mod symbol_ranking_tests;

#[path = "definition_fallback.rs"]
mod definition_fallback_tests;

#[path = "reference_ranking.rs"]
mod reference_ranking_tests;

#[path = "reference_generated.rs"]
mod reference_generated_tests;

#[path = "excerpts.rs"]
mod excerpt_tests;

#[path = "field_filters.rs"]
mod field_filter_tests;
