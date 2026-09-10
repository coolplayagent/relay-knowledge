use super::*;

fn calls(source: &str) -> Vec<(String, Option<String>)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    let mut result = Vec::new();
    let mut cursor = tree.root_node().walk();
    loop {
        let node = cursor.node();
        if node.kind() == "method_invocation" {
            result.push((text(node, source).to_owned(), symbol(node, source)));
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return result;
            }
        }
    }
}

#[test]
fn resolves_field_local_parameter_and_explicit_receivers() {
    let result = calls(
        r#"package demo; class C {
      Config field;
      C(Config parameter) { parameter.getX(); }
      void run(Config parameter) {
        Config local = parameter;
        field.getX(); this.field.getX(); local.getX(); parameter.getX();
        (field).getX(); ((Config) field).getX(); new Config().getX();
        var inferred = new Config(); inferred.getX();
        var alias = local; alias.getX();
      }
    }"#,
    );
    assert_eq!(result.len(), 10);
    for (call, owner) in result {
        assert_eq!(owner.as_deref(), Some("demo.Config.getX"), "{call}");
    }
}

#[test]
fn nearest_declaration_wins_and_finished_blocks_do_not_shadow_fields() {
    let result = calls(
        r#"class C { Config config;
      void run() {
        config.getX();
        { Other config = null; config.getX(); this.config.getX(); }
        config.getX();
        Other config = null; config.getX();
      }
    }"#,
    );
    let owners = result
        .iter()
        .map(|(_, owner)| owner.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        owners,
        [
            Some("Config.getX"),
            Some("Other.getX"),
            Some("Config.getX"),
            Some("Config.getX"),
            Some("Other.getX")
        ]
    );
}

#[test]
fn respects_lambda_loop_and_catch_bindings() {
    let result = calls(
        r#"class C { Config config;
      void run(java.util.List<Other> list) {
        java.util.function.Function<Other, Boolean> a = config -> config.getX();
        java.util.function.Function<Other, Boolean> b = (Other config) -> config.getX();
        for (Other config : list) { config.getX(); }
        for (Other config = null; config.getX(); ) {}
        try {} catch (Other config) { config.getX(); }
        config.getX();
      }
    }"#,
    );
    let owners = result
        .iter()
        .map(|(_, owner)| owner.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        owners,
        [
            None,
            Some("Other.getX"),
            Some("Other.getX"),
            Some("Other.getX"),
            None,
            Some("Config.getX")
        ]
    );
}

#[test]
fn resolves_generic_imported_fields_and_same_file_inheritance() {
    let result = calls(
        r#"package demo; import settings.Config;
      class Parent { Config<String> config; }
      class Child extends Parent { void run() { config.getX(); this.config.getX(); } }
      class Holder { Config<String> value; }
      class App { Holder holder; void run() { holder.value.getX(); } }
    "#,
    );
    assert_eq!(result.len(), 3);
    for (_, owner) in result {
        assert_eq!(owner.as_deref(), Some("settings.Config.getX"));
    }
}

#[test]
fn rejects_unknown_arrays_dynamic_inference_and_non_getters() {
    let result = calls(
        r#"class C { Config field[];
      void run(Config[] array) {
        field.getX(); array.getX(); missing.getX();
        var dynamic = factory(); dynamic.getX();
        Config c = null; c.getX(1); c.execute();
      }
    }"#,
    );
    assert!(
        result.iter().all(|(_, owner)| owner.is_none()),
        "{result:?}"
    );
}

#[test]
fn receiver_expression_and_lookup_budgets_stop_pathological_inputs() {
    let declarations = (0..MAX_EXPRESSION_DEPTH + 2)
        .map(|i| {
            if i == 0 {
                "var v0 = new Config();".to_owned()
            } else {
                format!("var v{i} = v{};", i - 1)
            }
        })
        .collect::<String>();
    let result = calls(&format!(
        "class C {{ void run() {{ {declarations} v{}.getX(); }} }}",
        MAX_EXPRESSION_DEPTH + 1
    ));
    assert_eq!(result[0].1, None);
    let fields = (0..MAX_LOOKUP_STEPS + 2)
        .map(|i| format!("Other f{i};"))
        .collect::<String>();
    let result = calls(&format!(
        "class C {{ {fields} Config config; void run() {{ config.getX(); }} }}"
    ));
    assert_eq!(result[0].1, None);
}

#[test]
fn cyclic_inheritance_and_unknown_parent_do_not_escape_lookup_bounds() {
    let result = calls("class A extends B {} class B extends A { void run() { unknown.getX(); } }");
    assert_eq!(result[0].1, None);
    let result = calls(
        "class Outer { Config config; class Inner extends External { void run() { config.getX(); } } }",
    );
    assert_eq!(result[0].1, None);
}

#[test]
fn resource_and_multiple_declarator_types_do_not_leak_outside_their_scope() {
    let result = calls(
        r#"class C { Config resource;
      void run() {
        Config first = null, second = null; second.getX();
        try (Other resource = new Other()) { resource.getX(); }
        resource.getX();
      }
    }"#,
    );
    let owners = result
        .iter()
        .map(|(_, owner)| owner.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        owners,
        [Some("Config.getX"), Some("Other.getX"), Some("Config.getX")]
    );
}

#[test]
fn inherited_private_fields_and_ambiguous_inherited_names_do_not_bind() {
    let result = calls(
        r#"class Parent { private Config config; }
      class Child extends Parent { void run() { config.getX(); this.config.getX(); } }
      interface Left { Config config = null; }
      interface Right { Config config = null; }
      class Both implements Left, Right { void run() { config.getX(); } }
    "#,
    );
    assert_eq!(result.len(), 3);
    assert!(
        result.iter().all(|(_, owner)| owner.is_none()),
        "{result:?}"
    );
}

#[test]
fn type_parameters_do_not_bind_unrelated_same_named_configuration_classes() {
    let result = calls(
        r#"class Config { boolean getX() { return true; } }
      class C<Config> { Config field; void run() { field.getX(); } }
      class D { <Config> void run(Config value) { value.getX(); } }
    "#,
    );
    assert_eq!(result.len(), 2);
    assert!(
        result.iter().all(|(_, owner)| owner.is_none()),
        "{result:?}"
    );
}
