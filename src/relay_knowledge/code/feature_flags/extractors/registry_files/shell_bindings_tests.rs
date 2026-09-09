use super::*;

fn reads(source: &str) -> Vec<bool> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut cursor = tree.root_node().walk();
    let mut reads = Vec::new();
    loop {
        let node = cursor.node();
        if node.kind() == "simple_expansion" {
            reads.push(environment_read(node, "FLAG", source));
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return reads;
            }
        }
    }
}

#[test]
fn local_assignments_mask_reads_but_not_their_own_rhs_or_explicit_exports() {
    assert_eq!(reads("FLAG=$FLAG\necho $FLAG\n"), [true, false]);
    assert_eq!(reads("FLAG=no\nexport FLAG\necho $FLAG\n"), [true]);
    assert_eq!(reads("export FLAG=yes\nFLAG=no\necho $FLAG\n"), [true]);
    assert_eq!(reads("FLAG=no command\necho $FLAG\n"), [true]);
    assert_eq!(
        reads("export FLAG=yes\nexport -n FLAG\necho $FLAG\n"),
        [false]
    );
    assert_eq!(reads("unset FLAG\necho $FLAG\n"), [false]);
}

#[test]
fn function_local_bindings_do_not_escape_and_exported_locals_remain_evidence() {
    assert_eq!(
        reads("f() { local FLAG=no; echo $FLAG; }\necho $FLAG\n"),
        [false, true]
    );
    assert_eq!(reads("f() { local -x FLAG=yes; echo $FLAG; }\n"), [true]);
    assert_eq!(reads("FLAG=no\nf() { echo $FLAG; }\n"), [false]);
}

#[test]
fn exceeded_binding_budget_does_not_guess_environment_origin() {
    let source = format!(
        "{}echo $FLAG\n",
        "echo unrelated\n".repeat(MAX_BINDING_NODES + 1)
    );
    assert_eq!(reads(&source), [false]);
}
