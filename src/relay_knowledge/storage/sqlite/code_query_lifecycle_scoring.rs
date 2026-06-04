pub(super) fn lifecycle_finalization_bonus(
    query_terms: &[String],
    content_terms: &[String],
    content: &str,
) -> f64 {
    let has_finish_intent = query_terms
        .iter()
        .any(|term| matches!(term.as_str(), "finish" | "finalize" | "finalized"));
    let has_tool_call_intent = query_terms
        .iter()
        .any(|term| matches!(term.as_str(), "tool" | "tools"))
        && query_terms
            .iter()
            .any(|term| matches!(term.as_str(), "call" | "calls"));
    let has_stream_lifecycle_intent = query_terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "delta" | "event" | "events" | "lifecycle" | "stream"
        )
    });
    if !has_finish_intent || !has_tool_call_intent || !has_stream_lifecycle_intent {
        return 0.0;
    }
    let content_has_finish = content_terms
        .iter()
        .any(|term| matches!(term.as_str(), "finish" | "finalize" | "finished"));
    let content_has_tool = content_terms
        .iter()
        .any(|term| matches!(term.as_str(), "tool" | "tools"));
    let content_has_lifecycle = content_terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "delta" | "event" | "events" | "lifecycle" | "stream"
        )
    });
    if content_has_finish
        && content_has_tool
        && content_has_lifecycle
        && content.lines().any(|line| {
            let line = line.trim();
            line.contains("finish") && (line.contains('.') || line.contains("yield"))
        })
    {
        1.35
    } else {
        0.0
    }
}
