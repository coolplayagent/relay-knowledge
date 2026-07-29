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
