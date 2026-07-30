//! Python annotation-reference scanning contracts.

use super::manual_type_references;

fn reference_names(content: &str) -> Vec<String> {
    manual_type_references(content)
        .into_iter()
        .map(|(name, _, _)| name)
        .collect()
}

#[test]
fn keeps_commas_inside_generic_type_annotations() {
    let names = reference_names(
        "def save(request: dict[str, W3ConnectorSaveRequest]) -> Result[ConnectorItem, SaveError]:\n    pass\n",
    );

    assert!(names.iter().any(|name| name == "W3ConnectorSaveRequest"));
    assert!(names.iter().any(|name| name == "ConnectorItem"));
    assert!(names.iter().any(|name| name == "SaveError"));
}

#[test]
fn ignores_colons_inside_default_expressions() {
    let names = reference_names(
        "def save(request: W3ConnectorSaveRequest = {'fallback': Bar}) -> None:\n    pass\n",
    );

    assert_eq!(names, vec!["W3ConnectorSaveRequest"]);
}

#[test]
fn skips_unannotated_default_expression_colons() {
    let names = reference_names(
        "def save(options={fallback: Bar}, request: W3ConnectorSaveRequest = None) -> None:\n    pass\n",
    );

    assert_eq!(names, vec!["W3ConnectorSaveRequest"]);
}

#[test]
fn preserves_hash_characters_inside_string_defaults() {
    let names = reference_names(
        "def save(request: W3ConnectorSaveRequest = \"#\") -> SaveResult:\n    body: BodyType = BodyType()\n",
    );

    assert_eq!(names, vec!["W3ConnectorSaveRequest", "SaveResult"]);
}

#[test]
fn carries_annotations_across_wrapped_lines() {
    let names = reference_names(
        "def save(\n    request: dict[\n        str, W3ConnectorSaveRequest\n    ],\n) -> None:\n    pass\n",
    );

    assert_eq!(names, vec!["W3ConnectorSaveRequest"]);
}
