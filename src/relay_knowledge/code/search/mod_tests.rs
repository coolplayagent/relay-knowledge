use super::*;

#[test]
fn internal_scanner_primary_path_does_not_report_ripgrep_unavailable() {
    let mut tree = TempSourceTree::create().expect("temp tree should be created");
    tree.write("src/component.tsx", b"import React from \"react\";\n")
        .expect("source path should be written");
    let request = SourceGrepRequest {
        query: "react".to_owned(),
        paths: vec!["src/component.tsx".to_owned()],
        path_filters: Vec::new(),
        language_filters: vec!["tsx".to_owned()],
        limit: 10,
        kind: SourceGrepKind::Imports,
        exclude_generated: false,
    };

    let outcome =
        source_grep_matches_from_materialized_tree(&tree.root, &request.paths, &request, None)
            .expect("internal scanner should search materialized source");

    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].path, "src/component.tsx");
    assert_eq!(outcome.matches[0].language_id, "tsx");
    assert_eq!(outcome.matches[0].excerpt, "import React from \"react\";");
    assert!(outcome.degraded_reason.is_none());
}
