use super::*;

#[test]
fn escapes_authored_labels_and_excludes_non_neighbor_edges() {
    let graph: AuthoredEvidenceGraph = serde_json::from_value(serde_json::json!({"nodes":[{"id":"a","kind":"concept","label":"<&\"|\\`\n\r\u{0001}>"},{"id":"b","kind":"concept","label":"B"},{"id":"c","kind":"concept","label":"C"}],"edges":[{"source":"a","target":"b","relation":"uses","evidence":[]},{"source":"b","target":"c","relation":"uses","evidence":[]}]})).unwrap();
    let view = graph_view(graph.clone(), Some("a")).unwrap();
    assert_eq!(view["graph"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(view["graph"]["edges"].as_array().unwrap().len(), 1);
    assert!(
        view["mermaid"]
            .as_str()
            .unwrap()
            .contains("&lt;&amp;&quot;&#124;&#92;&#96;  &gt;")
    );
    assert!(graph_view(graph, Some("missing")).is_err());
}
