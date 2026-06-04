pub(super) fn lifecycle_finalization_bonus(
    query_terms: &[String],
    content_terms: &[String],
    content: &str,
) -> f64 {
    let has_finish_intent = query_terms
        .iter()
        .any(|term| lifecycle_finalization_term(term));
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
        .any(|term| lifecycle_finalization_term(term));
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
        && content.lines().any(line_contains_finalization_flow)
    {
        1.35
    } else {
        0.0
    }
}

fn lifecycle_finalization_term(term: &str) -> bool {
    matches!(term, "finish" | "finished" | "finalize" | "finalized")
}

fn line_contains_finalization_flow(line: &str) -> bool {
    let line = line.trim().to_ascii_lowercase();
    (line.contains("finish") || line.contains("finalize"))
        && (line.contains('.') || line.contains("yield"))
}

#[cfg(test)]
mod tests {
    use super::lifecycle_finalization_bonus;

    #[test]
    fn lifecycle_finalization_bonus_accepts_finalize_spellings() {
        let query_terms = terms("openai tool call delta lifecycle finalized events");
        let content = "tools yield* ToolStream.finalize(lifecycle, events)";
        let content_terms = terms(content);

        assert!(lifecycle_finalization_bonus(&query_terms, &content_terms, content) > 0.0);
    }

    #[test]
    fn lifecycle_finalization_bonus_accepts_capitalized_finish_flow() {
        let query_terms = terms("openai tool call delta lifecycle finish events");
        let content = "return ToolStream.Finish(tool_call_events);";
        let content_terms = terms(content);

        assert!(lifecycle_finalization_bonus(&query_terms, &content_terms, content) > 0.0);
    }

    fn terms(value: &str) -> Vec<String> {
        value
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|term| !term.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    }
}
