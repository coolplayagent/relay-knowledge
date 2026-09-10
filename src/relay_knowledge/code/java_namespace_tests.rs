use super::*;

fn evidence(source: &str) -> JavaFileNamespace {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    collect(tree.root_node(), source)
}

#[test]
fn actual_package_and_only_top_level_types_form_namespace_evidence() {
    let result = evidence(
        "package arbitrary.location; class System {} interface I {} enum E { X } record R(int x) {} @interface A {} class Outer { class Nested {} void method() { class Local {} } }",
    );
    assert!(result.evidence.complete);
    assert_eq!(result.evidence.package, "arbitrary.location");
    assert_eq!(
        result.evidence.top_level_types,
        ["A", "E", "I", "Outer", "R", "System"]
    );
    let default_package = evidence("class DefaultType {}");
    assert!(default_package.evidence.complete);
    assert_eq!(default_package.evidence.package, "");
}

#[test]
fn malformed_escaped_or_budget_limited_namespaces_cannot_prove_absence() {
    assert!(!evidence("package p; class Broken {").evidence.complete);
    assert!(
        !evidence(r"package p; class Syst\u0065m {}")
            .evidence
            .complete
    );
    let source = (0..1100)
        .map(|i| format!("class T{i} {{}}\n"))
        .collect::<String>();
    assert!(!evidence(&source).evidence.complete);
}

#[test]
fn explicit_single_type_imports_are_distinct_from_static_or_wildcard_imports() {
    let result =
        evidence("import java.lang.System; import static java.lang.Boolean.*; class App {}");
    assert_eq!(
        result
            .explicit_platform_types
            .into_iter()
            .collect::<Vec<_>>(),
        ["System"]
    );
    assert!(
        evidence("import java.lang.*; class App {}")
            .explicit_platform_types
            .is_empty()
    );
}

#[test]
fn repeated_package_bytes_bound_namespace_projection_before_storage() {
    let package = "p".repeat(512);
    let source = |count| {
        format!(
            "package {package}; {}",
            (0..count)
                .map(|index| format!("class T{index} {{}}\n"))
                .collect::<String>()
        )
    };
    let under = evidence(&source(100));
    assert!(under.evidence.complete);
    assert!(under.evidence.projected_name_bytes().is_some());
    let over = evidence(&source(130));
    // Source names fit 64 KiB; their repeated package projection does not.
    assert!(source(130).len() < JavaNamespaceEvidence::MAX_PROJECTED_NAME_BYTES);
    assert!(!over.evidence.complete);
    assert!(over.evidence.top_level_types.is_empty());
    assert_eq!(
        over.evidence.projected_name_bytes(),
        Some(package.len() + 7)
    );
}

#[test]
fn package_comments_use_structured_identifiers_and_share_collection_budgets() {
    let result = evidence("package sample /* legal */ . nested; class Main {}");
    assert!(result.evidence.complete);
    assert_eq!(result.evidence.package, "sample.nested");
    let result = evidence("package sample // legal\n . nested; class Main {}");
    assert!(result.evidence.complete);
    assert_eq!(result.evidence.package, "sample.nested");
    assert!(
        !evidence("package sample . ; class Main {}")
            .evidence
            .complete
    );
    let comments = "/* comment */".repeat(1100);
    assert!(
        !evidence(&format!(
            "package sample {comments} . nested; class Main {{}}"
        ))
        .evidence
        .complete
    );
}
