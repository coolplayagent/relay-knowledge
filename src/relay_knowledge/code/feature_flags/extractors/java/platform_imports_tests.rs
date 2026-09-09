use super::*;

fn imported_receiver(source: &str) -> Option<&'static str> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut cursor = tree.root_node().walk();
    loop {
        let node = cursor.node();
        if node.kind() == "method_invocation" {
            let name = text(node.child_by_field_name("name").unwrap(), source);
            return receiver(node, name, source);
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                panic!("fixture requires a call");
            }
        }
    }
}

#[test]
fn explicit_and_wildcard_imports_preserve_precedence_and_platform_method_ownership() {
    for imports in [
        "import static java.lang.System.getenv;",
        "import static java.lang.System.*; import static java.lang.Boolean.*;",
        "import static java.lang.System.getenv; import static other.Unknown.*;",
    ] {
        assert_eq!(
            imported_receiver(&format!(
                "{imports} class App {{ void run(String getenv) {{ getenv(\"FLAG\"); }} }}"
            )),
            Some("java.lang.System")
        );
    }
    assert_eq!(
        imported_receiver(
            "import static java.lang.Boolean.*; class App { void run() { getBoolean(\"FLAG\"); } }"
        ),
        Some("java.lang.Boolean")
    );
    for imports in [
        "import static java.lang.System.getenv; import static other.Custom.getenv;",
        "import static java.lang.System.*; import static other.Custom.getenv;",
        "import static java.lang.System.*; import static other.Custom.*;",
        "import java.lang.System;",
    ] {
        assert_eq!(
            imported_receiver(&format!(
                "{imports} class App {{ void run() {{ getenv(\"FLAG\"); }} }}"
            )),
            None
        );
    }
}

#[test]
fn visible_methods_and_unproven_inheritance_prevent_static_import_assumptions() {
    for body in [
        "class App { String getenv(String s) { return s; } void run() { getenv(\"FLAG\"); } }",
        "class Base { String getenv(String s) { return s; } } class App extends Base { void run() { getenv(\"FLAG\"); } }",
        "class App extends Unknown { void run() { getenv(\"FLAG\"); } }",
        "class Outer { String getenv(String s) { return s; } class Inner { void run() { getenv(\"FLAG\"); } } }",
        "class Object { String getenv(String s) { return s; } } class App extends Object { void run() { getenv(\"FLAG\"); } }",
    ] {
        assert_eq!(
            imported_receiver(&format!("import static java.lang.System.getenv; {body}")),
            None,
            "{body}"
        );
    }
    assert_eq!(
        imported_receiver(
            "import static java.lang.System.getenv; interface Marker {} class App implements Marker { void run() { getenv(\"FLAG\"); } }"
        ),
        Some("java.lang.System")
    );
    assert_eq!(
        imported_receiver(
            "import static java.lang.System.getenv; class App extends java.lang.Object { void run() { getenv(\"FLAG\"); } }"
        ),
        Some("java.lang.System")
    );
}

#[test]
fn import_resolution_budget_exhaustion_does_not_guess_a_platform_receiver() {
    let imports = "import static java.lang.System.getenv;\n".repeat(MAX_RESOLUTION_NODES);
    assert_eq!(
        imported_receiver(&format!(
            "{imports}class App {{ void run() {{ getenv(\"FLAG\"); }} }}"
        )),
        None
    );
}

#[test]
fn inherited_type_searches_spend_the_same_budget_including_sibling_scans() {
    let interfaces = (0..40)
        .map(|n| format!("interface Marker{n} {{}}\n"))
        .collect::<String>();
    let parents = (0..40)
        .map(|n| format!("Marker{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        "import static java.lang.System.getenv; {interfaces} class App implements {parents} {{ void run() {{ getenv(\"FLAG\"); }} }}"
    );
    assert_eq!(imported_receiver(&source), None);
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(&source, None).unwrap();
    let mut remaining = 3;
    assert!(visible_parent(tree.root_node(), "Absent", &source, &mut remaining).is_none());
    assert_eq!(remaining, 0);
}

#[test]
fn unresolved_object_needs_explicit_platform_identity_and_noninherited_methods_do_not_shadow() {
    assert_eq!(
        imported_receiver(
            "import static java.lang.System.getenv; class App extends Object { void run() { getenv(\"FLAG\"); } }"
        ),
        None
    );
    assert_eq!(
        imported_receiver(
            "import java.lang.Object; import static java.lang.System.getenv; class App extends Object { void run() { getenv(\"FLAG\"); } }"
        ),
        Some("java.lang.System")
    );
    for body in [
        "class Base { private String getenv(String key) { return key; } } class App extends Base { void run() { getenv(\"FLAG\"); } }",
        "interface Base { static String getenv(String key) { return key; } } class App implements Base { void run() { getenv(\"FLAG\"); } }",
        "interface Base { private String getenv(String key) { return key; } } class App implements Base { void run() { getenv(\"FLAG\"); } }",
    ] {
        assert_eq!(
            imported_receiver(&format!("import static java.lang.System.getenv; {body}")),
            Some("java.lang.System"),
            "{body}"
        );
    }
    for body in [
        "class Base { protected static String getenv(String key) { return key; } } class App extends Base { void run() { getenv(\"FLAG\"); } }",
        "interface Base { default String getenv(String key) { return key; } } class App implements Base { void run() { getenv(\"FLAG\"); } }",
        "interface Own { static String getenv(String key) { return key; } default void run() { getenv(\"FLAG\"); } }",
        "class Own { private String getenv(String key) { return key; } void run() { getenv(\"FLAG\"); } }",
    ] {
        assert_eq!(
            imported_receiver(&format!("import static java.lang.System.getenv; {body}")),
            None,
            "{body}"
        );
    }
}
