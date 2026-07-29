use super::*;

#[test]
fn ambiguous_callee_score_prefers_concrete_source_body() {
    let source = candidate(
        "src/main/java/example/AnnotatedService.java",
        "public String handle(String value) { return normalize(value).trim(); }",
    );
    let interface = candidate(
        "src/main/java/example/ServiceContract.java",
        "T handle(T value);",
    );
    let fake = candidate(
        "src/test/java/example/FakeService.java",
        "String handle(String value) { return value; }",
    );

    assert!(
        ambiguous_callee_implementation_score(&source, false, 2.0)
            > ambiguous_callee_implementation_score(&interface, false, 2.0)
    );
    assert!(
        ambiguous_callee_implementation_score(&source, false, 2.0)
            > ambiguous_callee_implementation_score(&fake, false, 2.0)
    );
    assert_eq!(
        ambiguous_callee_implementation_score(&source, false, 4.0),
        AMBIGUOUS_CALLEE_IMPLEMENTATION_MAX_SCORE
    );
}

#[test]
fn ambiguous_callee_excerpt_uses_body_when_available() {
    let excerpt = ambiguous_callee_implementation_excerpt(
        "dispatch",
        "handle",
        "public String handle(String value)",
        Some("public String handle(String value) {\n return normalize(value).trim();\n}"),
    );

    assert!(excerpt.contains("normalize(value).trim()"));
}

#[test]
fn ambiguous_callee_context_accepts_same_directory_implementation() {
    let context = context("src/main/java/example/ServiceFactory.java", None);
    let candidate = candidate(
        "src/main/java/example/AnnotatedService.java",
        "public String handle(String value) { return normalize(value).trim(); }",
    );

    assert!(ambiguous_callee_context_score(&candidate, &context) > 0.0);
}

#[test]
fn ambiguous_callee_context_rejects_same_name_without_local_evidence() {
    let context = context("src/main/java/example/ServiceFactory.java", None);
    let candidate = candidate(
        "src/main/java/other/RemoteHandler.java",
        "public String handle(String value) { return value; }",
    );

    assert_eq!(ambiguous_callee_context_score(&candidate, &context), 0.0);
}

#[test]
fn ambiguous_callee_context_accepts_specific_target_hint() {
    let context = context(
        "src/main/java/example/ServiceFactory.java",
        Some("com.acme.worker.AnnotatedService.handle"),
    );
    let mut candidate = candidate(
        "src/main/java/worker/AnnotatedService.java",
        "public String handle(String value) { return normalize(value).trim(); }",
    );
    candidate.canonical_symbol_id =
        "repo://repo/com::acme::worker::AnnotatedService.handle".to_owned();

    assert!(ambiguous_callee_context_score(&candidate, &context) > 0.0);
}

#[test]
fn ambiguous_callee_candidate_scope_includes_target_hint_identity_terms() {
    let context = context(
        "src/main/java/example/ServiceFactory.java",
        Some("com.acme.worker.AnnotatedService.handle"),
    );
    let target_hint_terms = ambiguous_context_target_hint_term_sets(&[context]);
    let predicate = callee_candidate_scope_predicate(
        &["src/main/java/example/ServiceFactory.java".to_owned()],
        &[],
        &target_hint_terms,
    );

    assert!(target_hint_terms[0].contains(&"worker".to_owned()));
    assert!(target_hint_terms[0].contains(&"annotatedservice".to_owned()));
    assert!(!target_hint_terms[0].contains(&"handle".to_owned()));
    assert!(predicate.contains("canonical_symbol_id"), "{predicate}");
    assert!(predicate.contains("s.path IN (?)"), "{predicate}");
}

#[test]
fn ambiguous_callee_order_prioritizes_target_hint_identity_terms() {
    let context = context(
        "src/main/java/example/ServiceFactory.java",
        Some("com.acme.worker.AnnotatedService.handle"),
    );
    let target_hint_terms = ambiguous_context_target_hint_term_sets(&[context]);
    let expression = callee_candidate_target_hint_order_expression(&target_hint_terms);

    assert!(expression.starts_with("CASE WHEN"), "{expression}");
    assert!(expression.contains("canonical_symbol_id"), "{expression}");
    assert!(expression.contains("THEN 0 ELSE 1"), "{expression}");
}

#[test]
fn ambiguous_callee_lookup_uses_leaf_but_keeps_qualified_hint_terms() {
    let mut call_context = context(
        "src/main/java/example/ConnectorFactory.java",
        Some("net::C.connect"),
    );
    call_context.callee_name = "C.connect".to_owned();
    let lookup_names = ambiguous_context_callee_lookup_names(&[call_context]);

    assert_eq!(lookup_names, vec!["connect".to_owned()]);

    let mut call_context = context(
        "src/main/java/example/ConnectorFactory.java",
        Some("net::C.connect"),
    );
    call_context.callee_name = "C.connect".to_owned();
    let hint_terms = ambiguous_context_target_hint_term_sets(&[call_context]);

    assert!(hint_terms[0].contains(&"net".to_owned()));
    assert!(!hint_terms[0].contains(&"connect".to_owned()));
}

#[test]
fn ambiguous_callee_context_accepts_qualified_member_leaf() {
    let mut context = context(
        "src/main/java/example/ConnectorFactory.java",
        Some("net::C.connect"),
    );
    context.callee_name = "C.connect".to_owned();
    let mut candidate = candidate(
        "src/main/java/net/C.java",
        "public Connection connect(Target target) { return target.open(); }",
    );
    candidate.name = "connect".to_owned();
    candidate.signature = "public Connection connect(Target target)".to_owned();
    candidate.canonical_symbol_id = "repo://repo/net::C.connect".to_owned();

    assert!(ambiguous_callee_context_score(&candidate, &context) > 0.0);
}

#[test]
fn ambiguous_callee_contexts_keep_distinct_target_hints() {
    let contexts = ambiguous_callee_contexts(&[
        call_row("handle", Some("primary.Service.handle"), 10),
        call_row("handle", Some("fallback.Service.handle"), 11),
        call_row("handle", Some("primary.Service.handle"), 10),
    ]);

    assert_eq!(contexts.len(), 2);
    assert!(
        contexts
            .iter()
            .any(|context| context.target_hint.as_deref() == Some("primary.Service.handle"))
    );
    assert!(
        contexts
            .iter()
            .any(|context| context.target_hint.as_deref() == Some("fallback.Service.handle"))
    );
}

fn context(path: &str, target_hint: Option<&str>) -> AmbiguousCalleeContext {
    AmbiguousCalleeContext {
        callee_name: "handle".to_owned(),
        path: path.to_owned(),
        language_id: "java".to_owned(),
        line_range: range(10, 10),
        target_hint: target_hint.map(str::to_owned),
        caller_name: Some("dispatch".to_owned()),
        caller_signature: Some("void dispatch(Service service)".to_owned()),
        caller_excerpt: Some("return service.handle(payload);".to_owned()),
        caller_canonical_symbol_id: Some("repo://repo/ServiceFactory.dispatch".to_owned()),
    }
}

fn call_row(callee_name: &str, target_hint: Option<&str>, line: u32) -> CallRow {
    CallRow {
        file_id: "file".to_owned(),
        path: "src/main/java/example/ServiceFactory.java".to_owned(),
        language_id: "java".to_owned(),
        is_generated: false,
        caller_symbol_snapshot_id: Some("caller".to_owned()),
        caller_name: Some("dispatch".to_owned()),
        callee_symbol_snapshot_id: None,
        callee_name: callee_name.to_owned(),
        line_range: range(line, line),
        caller_line_range: Some(range(1, 20)),
        target_hint: target_hint.map(str::to_owned),
        resolution_state: "ambiguous".to_owned(),
        confidence_basis_points: 5_000,
        confidence_tier: "ambiguous".to_owned(),
        caller_canonical_symbol_id: Some("repo://repo/ServiceFactory.dispatch".to_owned()),
        callee_canonical_symbol_id: None,
        caller_signature: Some("void dispatch(Service primary, Service fallback)".to_owned()),
        callee_signature: None,
        caller_excerpt: Some("primary.handle(payload); fallback.handle(payload);".to_owned()),
        callee_excerpt: None,
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

fn candidate(path: &str, body: &str) -> CalleeImplementationCandidate {
    CalleeImplementationCandidate {
        file_id: "file".to_owned(),
        path: path.to_owned(),
        language_id: "java".to_owned(),
        is_generated: false,
        symbol_snapshot_id: "symbol".to_owned(),
        canonical_symbol_id: "repo://repo/handle".to_owned(),
        name: "handle".to_owned(),
        signature: "public String handle(String value)".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 0 },
        line_range: RepositoryCodeRange { start: 1, end: 3 },
        body_excerpt: Some(body.to_owned()),
        parse_status: "parsed".to_owned(),
        degraded_reason: None,
    }
}
