use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn framework_language_filters_use_snapshot_files_before_limiting_nodes_and_edges() {
    let mut connection = Connection::open_in_memory().unwrap();
    super::super::schema::initialize_code_schema(&connection).unwrap();
    insert_repository(&connection);
    for (scope, language) in [("scope", "typescript"), ("other", "python")] {
        connection.execute("INSERT INTO code_repository_files (repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,is_generated) VALUES ('repository',?1,?2,'src/app.ts',?3,'hash',10,1,'parsed',0)", params![scope, format!("file-{scope}"), language]).unwrap();
    }
    let mut nodes = vec![node("component", FrameworkNodeKind::Component, "App")];
    let mut edges = vec![edge("render", FrameworkEdgeKind::Renders, "external")];
    for index in 0..2 {
        let path = format!("src/a-{index}.html");
        let file_id = format!("html-{index}");
        connection.execute("INSERT INTO code_repository_files (repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status,is_generated) VALUES ('repository','scope',?1,?2,'html','hash',10,1,'parsed',0)", params![file_id, path]).unwrap();
        let mut html_node = node(
            &format!("html-{index}"),
            FrameworkNodeKind::Template,
            "HTML",
        );
        html_node.path = path.clone();
        html_node.file_id = file_id.clone();
        nodes.push(html_node);
        let mut html_edge = edge(
            &format!("html-edge-{index}"),
            FrameworkEdgeKind::Renders,
            "external",
        );
        html_edge.path = path;
        html_edge.file_id = file_id;
        edges.push(html_edge);
    }
    let transaction = connection.transaction().unwrap();
    insert_records(&transaction, &nodes, &edges).unwrap();
    transaction.commit().unwrap();
    for language in ["typescript", "html", "python"] {
        let request = FrameworkGraphRequest::new(
            None,
            CodeRepositorySelector::new("fixture", "commit", vec![], vec![language.into()])
                .unwrap(),
            vec![],
            vec![],
            1,
            FreshnessPolicy::WaitUntilFresh,
        )
        .unwrap();
        let graph = search_scope(&mut connection, "scope", request.clone()).unwrap();
        assert_eq!(graph.nodes.len(), usize::from(language != "python"));
        assert_eq!(graph.edges.len(), usize::from(language != "python"));
        assert_eq!(graph.truncated, language == "html");
        if language == "typescript" {
            assert_eq!(graph.nodes[0].node_id, "component");
            assert_eq!(graph.edges[0].edge_id, "render");
        }
        for (sql, values) in [node_query("scope", &request), edge_query("scope", &request)] {
            let mut statement = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            let steps = statement
                .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(
                steps
                    .iter()
                    .any(|step| step.contains("SEARCH language_file")),
                "{steps:?}"
            );
            assert!(
                !steps.iter().any(|step| step.contains("SCAN language_file")),
                "{steps:?}"
            );
        }
    }
}

#[test]
fn framework_records_round_trip_with_bounded_filters() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection).expect("schema should initialize");
    insert_repository(&connection);
    let transaction = connection.transaction().expect("transaction should start");
    insert_records(
        &transaction,
        &[node("component", FrameworkNodeKind::Component, "AppShell")],
        &[edge("renders", FrameworkEdgeKind::Renders, "app-toolbar")],
    )
    .expect("framework facts should persist");
    transaction.commit().expect("facts should commit");

    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = FrameworkGraphRequest::new(
        Some("app".to_owned()),
        selector,
        vec![FrameworkKind::Angular],
        vec![FrameworkNodeKind::Component],
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let graph = search_scope(&mut connection, "scope", request).expect("graph should load");

    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.edges.len(), 1);
    assert!(!graph.truncated);
    assert_eq!(graph.nodes[0].name, "AppShell");
    assert_eq!(graph.edges[0].target_hint.as_deref(), Some("app-toolbar"));
}

#[test]
fn framework_graph_reports_truncation_for_node_or_edge_fan_out() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection).expect("schema should initialize");
    insert_repository(&connection);
    let transaction = connection.transaction().expect("transaction should start");
    insert_records(
        &transaction,
        &[
            node("component-a", FrameworkNodeKind::Component, "AppA"),
            node("component-b", FrameworkNodeKind::Component, "AppB"),
        ],
        &[],
    )
    .expect("framework facts should persist");
    transaction.commit().expect("facts should commit");
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = FrameworkGraphRequest::new(
        None,
        selector,
        Vec::new(),
        Vec::new(),
        1,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let graph = search_scope(&mut connection, "scope", request).expect("graph should load");

    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.truncated);
}

#[test]
fn framework_graph_resolves_component_selectors_and_external_templates() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection).expect("schema should initialize");
    insert_repository(&connection);
    let transaction = connection.transaction().expect("transaction should start");
    let mut component = node("toolbar", FrameworkNodeKind::Component, "ToolbarComponent");
    component.detail = Some("app-toolbar".to_owned());
    component.path = "src/toolbar.component.ts".to_owned();
    let mut template = node("template", FrameworkNodeKind::Template, "toolbar template");
    template.path = "src/toolbar.component.html".to_owned();
    let mut render = edge("render", FrameworkEdgeKind::Renders, "app-toolbar");
    render.source_node_id = "host-template".to_owned();
    let mut owns = edge(
        "owns",
        FrameworkEdgeKind::OwnsTemplate,
        "src/toolbar.component.html",
    );
    owns.source_node_id = "toolbar".to_owned();
    insert_records(&transaction, &[component, template], &[render, owns])
        .expect("framework facts should persist");
    transaction.commit().expect("facts should commit");
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = FrameworkGraphRequest::new(
        None,
        selector,
        Vec::new(),
        Vec::new(),
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let graph = search_scope(&mut connection, "scope", request).expect("graph should load");

    assert!(
        graph
            .edges
            .iter()
            .all(|edge| edge.resolution_state == "resolved")
    );
    assert!(graph.edges.iter().all(|edge| edge.target_node_id.is_some()));
}

#[test]
fn framework_target_resolution_filters_node_kinds_before_its_candidate_limit() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection).expect("schema should initialize");
    insert_repository(&connection);
    let transaction = connection.transaction().expect("transaction should start");
    let mut wrong_kinds = [
        node("slot", FrameworkNodeKind::Slot, "app-toolbar"),
        node("model", FrameworkNodeKind::Model, "app-toolbar"),
        node("input", FrameworkNodeKind::Input, "app-toolbar"),
    ];
    for (index, node) in wrong_kinds.iter_mut().enumerate() {
        node.path = format!("src/a-{index}.ts");
    }
    let mut component = node("toolbar", FrameworkNodeKind::Component, "ToolbarComponent");
    component.detail = Some("app-toolbar".to_owned());
    component.path = "src/z-toolbar.ts".to_owned();
    let mut nodes = wrong_kinds.into_iter().collect::<Vec<_>>();
    nodes.push(component);
    insert_records(
        &transaction,
        &nodes,
        &[edge("render", FrameworkEdgeKind::Renders, "app-toolbar")],
    )
    .expect("framework facts should persist");
    transaction.commit().expect("facts should commit");
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = FrameworkGraphRequest::new(
        None,
        selector,
        Vec::new(),
        Vec::new(),
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let graph = search_scope(&mut connection, "scope", request).expect("graph should load");

    assert_eq!(graph.edges[0].target_node_id.as_deref(), Some("toolbar"));
}

#[test]
fn framework_graph_rejects_unknown_persisted_enum_values() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection).expect("schema should initialize");
    insert_repository(&connection);
    let transaction = connection.transaction().expect("transaction should start");
    insert_records(
        &transaction,
        &[node("component", FrameworkNodeKind::Component, "AppShell")],
        &[],
    )
    .expect("framework facts should persist");
    transaction.commit().expect("facts should commit");
    connection
        .execute(
            "UPDATE code_repository_framework_nodes SET kind = 'future_kind'",
            [],
        )
        .expect("fixture should corrupt persisted enum");
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = FrameworkGraphRequest::new(
        None,
        selector,
        Vec::new(),
        Vec::new(),
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let error = search_scope(&mut connection, "scope", request)
        .expect_err("unknown storage enums must fail closed");

    assert!(error.to_string().contains("unknown framework node kind"));
}

fn node(id: &str, kind: FrameworkNodeKind, name: &str) -> CodeFrameworkNodeRecord {
    CodeFrameworkNodeRecord {
        repository_id: "repository".to_owned(),
        source_scope: "scope".to_owned(),
        node_id: id.to_owned(),
        file_id: "file".to_owned(),
        path: "src/app.ts".to_owned(),
        framework: FrameworkKind::Angular,
        kind,
        name: name.to_owned(),
        detail: None,
        symbol_snapshot_id: None,
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn edge(id: &str, kind: FrameworkEdgeKind, target: &str) -> CodeFrameworkEdgeRecord {
    CodeFrameworkEdgeRecord {
        repository_id: "repository".to_owned(),
        source_scope: "scope".to_owned(),
        edge_id: id.to_owned(),
        file_id: "file".to_owned(),
        path: "src/app.ts".to_owned(),
        framework: FrameworkKind::Angular,
        kind,
        source_node_id: "component".to_owned(),
        target_node_id: None,
        target_hint: Some(target.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 8_000,
        confidence_tier: "structured".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn insert_repository(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count, stale
             ) VALUES (
                'repository', 'fixture', '/tmp/fixture', '[]', '[]',
                'scope', 'commit', 'tree', 'fresh', 0, 0, 0, 0, 0
             )",
            [],
        )
        .expect("repository fixture should persist");
}
