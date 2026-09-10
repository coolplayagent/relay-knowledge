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

#[test]
fn c_family_builtin_specifiers_normalize_equivalent_orders_and_implicit_int() {
    for spellings in [
        vec!["int", "signed", "signed int", "int signed"],
        vec!["unsigned", "unsigned int", "int unsigned"],
        vec!["short", "short int", "signed short", "int short signed"],
        vec!["unsigned short", "short unsigned int", "int unsigned short"],
        vec!["long", "long int", "signed long int", "int long signed"],
        vec!["unsigned long", "long unsigned int", "int unsigned long"],
        vec![
            "long long",
            "long long int",
            "signed long long int",
            "long int signed long",
        ],
        vec![
            "unsigned long long",
            "long unsigned int long",
            "int long long unsigned",
        ],
        vec!["long double", "double long"],
        vec!["signed char", "char signed"],
        vec!["unsigned char", "char unsigned"],
    ] {
        for shape in ["{}", "const {}*", "{}&", "{}&&"] {
            let source = spellings
                .iter()
                .enumerate()
                .map(|(index, spelling)| {
                    let parameter = shape.replace("{}", spelling);
                    format!("int helper({parameter} value{index});")
                })
                .collect::<Vec<_>>()
                .join("\n");
            let result = keys(&source);
            assert_eq!(result.len(), spellings.len(), "{source}");
            assert!(result[0].is_some(), "{source}");
            assert!(
                result.iter().all(|key| key == &result[0]),
                "{source}: {result:?}"
            );
        }
    }
    let result = keys("int helper(const unsigned); int helper(unsigned int renamed);");
    assert!(result[0].is_some());
    assert_eq!(result[0], result[1]);
}

#[test]
fn c_family_builtin_types_remain_distinct_without_target_width_guesses() {
    let types = [
        "char",
        "signed char",
        "unsigned char",
        "short",
        "unsigned short",
        "int",
        "unsigned",
        "long",
        "unsigned long",
        "long long",
        "unsigned long long",
        "float",
        "double",
        "long double",
        "bool",
    ];
    let source = types
        .map(|kind| format!("int helper({kind} value);"))
        .join("\n");
    let result = keys(&source);
    assert_eq!(result.len(), types.len());
    assert!(result.iter().all(Option::is_some), "{result:?}");
    assert_eq!(
        result.iter().flatten().collect::<HashSet<_>>().len(),
        result.len()
    );
    let result = keys(
        "struct Box { int helper(unsigned) const; int helper(unsigned); int helper(unsigned)&; int helper(unsigned)&&; }; int helper(unsigned*); int helper(const unsigned*); int helper(unsigned,...);",
    );
    assert_eq!(result.len(), 7);
    assert!(result.iter().all(Option::is_some));
    assert_eq!(
        result.iter().flatten().collect::<HashSet<_>>().len(),
        result.len()
    );
}

#[test]
fn c_family_builtin_normalization_keeps_unknown_evidence_and_work_limits_closed() {
    for source in [
        "#define unsigned signed\nint helper(unsigned value);\n",
        "typedef unsigned long T; int helper(T value);",
        "int helper(unsigned values[3]);",
        "int helper(unsigned* const value);",
        "int helper(unsigned const* value);",
        "int helper(unsigned int value) noexcept(false);",
    ] {
        assert_eq!(keys(source), vec![None], "{source}");
    }
    let content = "int helper(unsigned long long int value);";
    let language = crate::code::languages::detect_language("src/key.cpp").unwrap();
    let tree = crate::code::parser::syntax::parse_tree(language, content).unwrap();
    let kind = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("declarator")
        .unwrap()
        .child_by_field_name("parameters")
        .unwrap()
        .named_child(0)
        .unwrap()
        .child_by_field_name("type")
        .unwrap();
    for (nodes, file_nodes) in [(0, 100), (100, 0), (1, 100), (100, 1)] {
        let mut file = file_nodes;
        assert!(
            primitive_types::canonical(
                content,
                kind,
                &mut Budget {
                    remaining: nodes,
                    file: &mut file
                }
            )
            .is_none()
        );
    }
    let mut file = 100;
    let mut budget = Budget {
        remaining: 100,
        file: &mut file,
    };
    assert_eq!(
        primitive_types::canonical(content, kind, &mut budget),
        Some("unsigned long long int")
    );
    assert!(budget.remaining < 100 && *budget.file < 100);
}
