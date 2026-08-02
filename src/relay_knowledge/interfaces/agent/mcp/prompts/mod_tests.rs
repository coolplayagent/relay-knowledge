use std::collections::HashMap;

use serde_json::json;

use super::{list_prompts, retrieve_context_prompt};

#[test]
fn prompt_catalog_publishes_unique_stable_names() {
    let catalog = list_prompts();
    let names = catalog["prompts"]
        .as_array()
        .expect("prompt array")
        .iter()
        .filter_map(|prompt| prompt["name"].as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        ["relay_retrieve_context_prompt", "relay_code_impact_prompt"]
    );
}

#[test]
fn retrieval_prompt_trims_required_text_and_applies_bounded_defaults() {
    let arguments = HashMap::from([("query".to_owned(), json!("  retry policy  "))]);

    let Ok(prompt) = retrieve_context_prompt(&arguments) else {
        panic!("retrieval prompt should render");
    };
    let text = prompt["messages"][0]["content"]["text"]
        .as_str()
        .expect("prompt text");

    assert!(text.contains("query `retry policy`"));
    assert!(text.contains("freshness `wait-until-fresh`"));
    assert!(retrieve_context_prompt(&HashMap::new()).is_err());
}
