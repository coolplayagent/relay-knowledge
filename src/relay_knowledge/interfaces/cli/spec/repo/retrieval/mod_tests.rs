use super::{
    repo_context, repo_feature_flags, repo_graph, repo_impact, repo_query, repo_software, repo_view,
};

#[test]
fn retrieval_specs_expose_each_read_surface_with_bounded_result_options() {
    let specs = [
        repo_query(),
        repo_graph(),
        repo_context(),
        repo_feature_flags(),
        repo_impact(),
        repo_view(),
        repo_software(),
    ];
    assert_eq!(
        specs
            .iter()
            .map(|spec| spec.path.last().copied())
            .collect::<Vec<_>>(),
        [
            Some("query"),
            Some("graph"),
            Some("context"),
            Some("feature-flags"),
            Some("impact"),
            Some("view"),
            Some("software"),
        ]
    );
    for spec in specs {
        assert!(
            spec.options.iter().any(|option| option.flag == "--limit")
                || spec.path.last() == Some(&"graph")
                || spec.path.last() == Some(&"view"),
            "{} should bound result size or use a fixed view contract",
            spec.usage
        );
    }
}
