use super::*;

#[test]
fn direct_execution_requires_a_synchronous_path_to_this_decorator() {
    for (header, before, after, safe) in [
        ("def", "", "return target", true),
        ("async def", "", "return target", false),
        ("def", "", "yield target", false),
        ("def", "", "def nested(value=(yield None)): pass", false),
        (
            "def",
            "",
            "value = lambda default=(yield None): None",
            false,
        ),
        ("def", "", "class Nested((yield None)): pass", false),
        ("def", "def later(): yield 1\n ", "return target", true),
        ("def", "if gate: return\n ", "return target", false),
        ("def", "raise RuntimeError\n ", "return target", false),
        ("def", "unknown()\n ", "return target", false),
        (
            "def",
            "match gate:\n  case True:\n   ",
            "return target",
            false,
        ),
    ] {
        let indent = if before.starts_with("match") {
            "   "
        } else {
            " "
        };
        let prefix = if before.is_empty() {
            String::new()
        } else {
            format!(" {}\n", before.trim_end())
        };
        let source = format!(
            "{header} outer():\n{prefix}{indent}@overload\n{indent}def target(): pass\n {after}\n"
        );
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let function = tree.root_node().named_child(0).unwrap();
        let mut stack = vec![function];
        let mut decorated = None;
        while let Some(node) = stack.pop() {
            if node.kind() == "decorated_definition" {
                decorated = Some(node);
                break;
            }
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor));
        }
        let decorated = decorated.unwrap();
        let mut remaining = 1024;
        assert_eq!(
            reaches(&source, function, decorated, &mut remaining),
            safe,
            "{source}"
        );
        assert!(!reaches(&source, function, decorated, &mut 0));
    }
}
