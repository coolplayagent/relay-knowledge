use super::*;

#[test]
fn hybrid_chunk_fts_query_uses_bounded_identifier_anchors() {
    let query = "client.Open LoadDefaultOptions workflow client retry timeout";
    let fts_query = hybrid_chunk_fts_match_query(query);

    assert!(fts_query.contains("\"LoadDefaultOptions\""));
    assert!(fts_query.contains("\"workflow\""));
    assert!(!fts_query.contains("\"Open\""));
    assert_eq!(fts_query.matches("\"client\"").count(), 1);
    assert!(
        fts_query.matches(" OR ").count()
            <= MAX_COMPOUND_FTS_ALTERNATIVES + MAX_HYBRID_CHUNK_RECALL_TERMS
    );
}

#[test]
fn fts_query_terms_are_deduplicated_before_planning() {
    assert_eq!(
        hybrid_chunk_fts_match_query("cache cache Lookup Insert"),
        "(\"cache\" OR \"Lookup\" OR \"Insert\") OR \"cachelookupinsert\" OR \"cache_lookup_insert\""
    );
}

#[test]
fn hybrid_chunk_fts_query_keeps_leading_lowercase_intent_terms() {
    let fts_query = hybrid_chunk_fts_match_query(
        "operation table read callback dispatch designated initializer",
    );

    for term in ["operation", "table", "read", "designated", "initializer"] {
        assert!(fts_query.contains(&format!("\"{term}\"")));
    }
}

#[test]
fn hybrid_chunk_fts_query_uses_high_signal_terms_for_api_dense_queries() {
    let fts_query = hybrid_chunk_fts_match_query(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
    );

    for term in ["RegisterWorkflow", "RegisterActivity", "InterruptCh"] {
        assert!(fts_query.contains(&format!("\"{term}\"")));
    }
    for term in ["worker", "task", "queue"] {
        assert!(!fts_query.contains(&format!("\"{term}\"")));
    }
}

#[test]
fn hybrid_chunk_fts_query_limits_broad_context_terms_for_api_dense_queries() {
    let fts_query = hybrid_chunk_fts_match_query(
        "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
    );

    assert!(fts_query.contains("\"MustLoadDefaultClientOptions\""));
    assert!(fts_query.contains("\"envconfig\""));
    assert!(!fts_query.contains("\"workflow\""));
    assert!(!fts_query.contains("\"client\""));
}

#[test]
fn direct_hybrid_chunk_fts_query_omits_compound_alternatives() {
    assert_eq!(
        direct_hybrid_chunk_fts_match_query("cache cache Lookup Insert"),
        "\"cache\" OR \"Lookup\" OR \"Insert\""
    );
    assert!(
        !direct_hybrid_chunk_fts_match_query("checkpoint metadata version constant")
            .contains("\"checkpointmetadataversionconstant\"")
    );
}

#[test]
fn focused_symbol_fts_query_uses_bounded_high_signal_terms() {
    assert_eq!(
        focused_symbol_fts_match_query("NoDestructor variadic constructor template instance type")
            .as_deref(),
        Some("\"NoDestructor\" OR \"constructor\" OR \"variadic\"")
    );
    assert!(focused_symbol_fts_match_query("NoDestructor constructor").is_none());
}

#[test]
fn focused_symbol_fts_query_keeps_workflow_identity_terms() {
    let fts_query = focused_symbol_fts_match_query(
        "background stream discovery reconcile multiplex run event source reconnect",
    )
    .expect("focused symbol query should be planned");

    assert!(fts_query.contains("\"stream\""));
    assert!(fts_query.contains("\"run\""));
}

#[test]
fn focused_hybrid_chunk_fts_query_uses_bounded_neighbor_pairs() {
    let fts_query =
        focused_hybrid_chunk_fts_match_query("typed arrow payload projector trim provider record")
            .expect("focused hybrid query should be planned");

    assert!(fts_query.contains("(\"payload\" \"projector\")"));
    assert!(fts_query.contains("(\"payload\" \"trim\")"));
    assert!(fts_query.contains("(\"payload\" \"provider\")"));
    assert!(fts_query.contains("(\"provider\" \"record\")"));
    assert!(!fts_query.contains("\"payload\" OR \"projector\""));
}

#[test]
fn focused_hybrid_chunk_fts_query_skips_structured_identifier_terms() {
    assert!(
        focused_hybrid_chunk_fts_match_query(
            "EvalCheckpointStore signature mismatch append result",
        )
        .is_none()
    );
    assert!(
        focused_hybrid_chunk_fts_match_query(
            "external session workflow TypeScript client openExternalSession",
        )
        .is_none()
    );
}

#[test]
fn lifecycle_hybrid_chunk_fts_query_recalls_tool_finalization_flow() {
    assert_eq!(
        lifecycle_hybrid_chunk_fts_match_query(
            "OpenAI Chat protocol sse tool call delta lifecycle finish events",
        )
        .as_deref(),
        Some("\"delta\" \"finish\"")
    );
    assert_eq!(
        lifecycle_hybrid_chunk_fts_match_query(
            "OpenAI Chat protocol SSE Tool Call Delta Lifecycle Finish Events",
        )
        .as_deref(),
        Some("\"delta\" \"finish\"")
    );
    assert_eq!(
        lifecycle_hybrid_chunk_fts_match_query(
            "OpenAI Chat protocol sse tool call delta lifecycle finalize events",
        )
        .as_deref(),
        Some("\"delta\" \"finalize\"")
    );
    assert_eq!(
        lifecycle_hybrid_chunk_fts_match_query(
            "OpenAI Chat protocol sse tool call delta lifecycle finalized events",
        )
        .as_deref(),
        Some("\"delta\" \"finalized\"")
    );
    assert_eq!(
        lifecycle_hybrid_chunk_fts_match_query(
            "OpenAI Chat protocol sse tool call delta lifecycle finished events",
        )
        .as_deref(),
        Some("\"delta\" \"finish\" OR \"delta\" \"finished\"")
    );
    assert!(lifecycle_hybrid_chunk_fts_match_query("protocol lifecycle events").is_none());
    assert!(lifecycle_hybrid_chunk_fts_match_query("tool call setup delta events").is_none());
}

#[test]
fn structured_hybrid_chunk_fts_query_uses_identifier_terms_only() {
    assert_eq!(
        structured_hybrid_chunk_fts_match_query(
            "external session workflow TypeScript client openExternalSession",
        )
        .as_deref(),
        Some("\"openExternalSession\"")
    );
    assert!(structured_hybrid_chunk_fts_match_query("plain workflow query").is_none());
}

#[test]
fn structured_hybrid_chunk_fts_query_keeps_type_surface_companions() {
    assert_eq!(
        structured_hybrid_chunk_fts_match_query(
            "metricsink plugin component Type MustNewType metric_sink",
        )
        .as_deref(),
        Some("\"MustNewType\" OR \"metric_sink\" OR \"component Type\"")
    );
}

#[test]
fn direct_hybrid_chunk_fts_query_keeps_type_surface_companions() {
    assert_eq!(
        direct_hybrid_chunk_fts_match_query(
            "metricsink plugin component Type MustNewType metric_sink",
        ),
        "\"MustNewType\" OR \"metric_sink\" OR \"metricsink\" OR \"component Type\""
    );
}

#[test]
fn direct_hybrid_chunk_fts_query_keeps_type_surface_companions_for_short_queries() {
    assert_eq!(
        direct_hybrid_chunk_fts_match_query("metricsink component Type MustNewType"),
        "\"metricsink\" OR \"component\" OR \"Type\" OR \"MustNewType\" OR \"component Type\""
    );
}

#[test]
fn hybrid_chunk_fts_query_keeps_compound_identifiers_with_type_surface_companions() {
    let fts_query = hybrid_chunk_fts_match_query("metric_sink component Type");

    assert!(fts_query.contains("\"component Type\""));
    assert!(fts_query.contains("\"metricsink\""));
}

#[test]
fn compound_hybrid_chunk_fts_query_uses_bounded_adjacent_identifier_pairs() {
    let fts_query = compound_hybrid_chunk_fts_match_query(
        "tsx provider panel effect run provider envelope payload",
    )
    .expect("compound hybrid query should be planned");

    assert!(fts_query.contains("\"providerpanel\""));
    assert!(fts_query.contains("\"provider_panel\""));
    assert!(fts_query.contains("\"envelopepayload\""));
    assert!(!fts_query.contains("\"providerpaneleffect\""));
}

#[test]
fn compound_hybrid_chunk_fts_query_recalls_type_identifier_pairs() {
    let fts_query =
        compound_hybrid_chunk_fts_match_query("typed arrow payload projector trim provider record")
            .expect("compound hybrid query should be planned");

    assert!(fts_query.contains("\"payloadprojector\""));
    assert!(fts_query.contains("\"payload_projector\""));
}
