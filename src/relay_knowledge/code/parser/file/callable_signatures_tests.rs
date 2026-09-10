use super::*;

fn keys(content: &str) -> Vec<Option<String>> {
    let language = crate::code::languages::detect_language("src/key.cpp").unwrap();
    let tree = crate::code::parser::syntax::parse_tree(language, content).unwrap();
    let mut stack = vec![tree.root_node()];
    let mut result = Vec::new();
    let mut file = MAX_FILE_SIGNATURE_NODES;
    let macros = macro_names(content, tree.root_node(), &mut file).unwrap();
    while let Some(node) = stack.pop() {
        if node.kind() == "function_declarator" {
            let mut budget = Budget {
                remaining: MAX_SIGNATURE_NODES,
                file: &mut file,
            };
            result.push(signature_key(content, node, &macros, &mut budget));
            continue;
        }
        for index in (0..node.child_count()).rev() {
            stack.push(node.child(index as u32).unwrap());
        }
    }
    result
}

#[test]
fn c_family_callable_keys_ignore_parameter_names_defaults_and_comments() {
    let voids = keys("int helper(void); int helper();");
    assert!(voids[0].is_some());
    assert_eq!(voids[0], voids[1]);
    let values = keys(
        "int helper(int); int helper(int old_name); int helper(int new_name = 3); int helper(int /* name */ value) { return value; }",
    );
    assert_eq!(values.len(), 4);
    assert!(values[0].is_some());
    assert!(values.iter().all(|value| value == &values[0]));
    let callbacks =
        keys("int helper(int (*)(const char*)); int helper(int (*callback)(const char* text));");
    assert_eq!(callbacks.len(), 2);
    assert!(callbacks[0].is_some());
    assert_eq!(callbacks[0], callbacks[1]);
}

#[test]
fn c_family_callable_keys_preserve_type_arity_cv_ref_and_variadic_boundaries() {
    let values = keys(
        "int helper(int); int helper(double); int helper(int,int); int helper(const int*); int helper(int*); int helper(int,...); struct Box { int helper() const; int helper(); int helper() &; int helper() &&; };",
    );
    assert_eq!(values.len(), 10);
    assert!(values.iter().all(Option::is_some));
    let distinct = values
        .iter()
        .flatten()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(distinct.len(), values.len());
}

#[test]
fn c_family_callable_keys_reject_incomplete_or_exhausted_evidence() {
    assert!(
        keys("int helper(int);\n#define int double\nint helper(int value) {}\n")
            .iter()
            .all(Option::is_none)
    );
    let scalar_cv =
        keys("int helper(const int); int helper(int value); int helper(volatile int renamed);");
    assert!(scalar_cv[0].is_some());
    assert!(scalar_cv.iter().all(|value| value == &scalar_cv[0]));
    for source in [
        "int helper(int values[3]);",
        "int helper(int * const value);",
        "typedef int T; int helper(T);",
        "#define T int\nint helper(T);\n#undef T\n#define T double\nint helper(T) {}",
    ] {
        assert!(keys(source).iter().all(Option::is_none), "{source}");
    }
    assert_eq!(keys("auto helper(int x) -> decltype(x);"), vec![None]);
    for source in [
        "int helper() noexcept(false);",
        "struct Box { int helper() override; };",
    ] {
        assert_eq!(keys(source), vec![None], "{source}");
    }
    for source in [
        "int helper(int callback(int));",
        "int helper(int (value));",
        "int helper(int const* value);",
        "int helper(const volatile int* value);",
    ] {
        assert_eq!(keys(source), vec![None], "{source}");
    }
    let oversized = format!(
        "void helper({});",
        "Type".repeat(MAX_CALLABLE_SIGNATURE_KEY_BYTES)
    );
    assert_eq!(keys(&oversized), vec![None]);
    let language = crate::code::languages::detect_language("src/key.cpp").unwrap();
    let tree = crate::code::parser::syntax::parse_tree(language, "int helper(int);").unwrap();
    let node = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("declarator")
        .unwrap();
    for (nodes, file_nodes) in [(0, 100), (100, 0), (1, 100)] {
        let mut file = file_nodes;
        assert!(
            signature_key(
                "int helper(int);",
                node,
                &HashSet::new(),
                &mut Budget {
                    remaining: nodes,
                    file: &mut file
                }
            )
            .is_none()
        );
    }
}

#[test]
fn c_family_callable_owner_rejects_template_context_and_wrong_declarator() {
    let content = "template<class T> int helper(T value);";
    let language = crate::code::languages::detect_language("src/key.cpp").unwrap();
    let tree = crate::code::parser::syntax::parse_tree(language, content).unwrap();
    let mut file = MAX_FILE_SIGNATURE_NODES;
    let mut budget = Budget {
        remaining: MAX_SIGNATURE_NODES,
        file: &mut file,
    };
    assert!(
        declarator(
            tree.root_node().named_child(0).unwrap(),
            "helper",
            content,
            &mut budget
        )
        .is_none()
    );
    let content = "int different(int value);";
    let tree = crate::code::parser::syntax::parse_tree(language, content).unwrap();
    assert!(
        declarator(
            tree.root_node().named_child(0).unwrap(),
            "helper",
            content,
            &mut budget
        )
        .is_none()
    );
}

#[test]
fn c_family_macro_context_is_shared_and_admitted_before_wide_traversal() {
    let content = "int helper(int value);\n".repeat(200);
    let result = keys(&content);
    assert_eq!(result.len(), 200);
    assert!(result.iter().all(Option::is_some));
    let language = crate::code::languages::detect_language("src/key.cpp").unwrap();
    let tree = crate::code::parser::syntax::parse_tree(language, &content).unwrap();
    assert!(macro_names(&content, tree.root_node(), &mut 100).is_none());
}
