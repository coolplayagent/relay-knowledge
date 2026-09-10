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
        ("def", "import provider\n ", "return target", false),
        (
            "def",
            "from provider import value\n ",
            "return target",
            false,
        ),
        (
            "def",
            "def later(): import provider\n ",
            "return target",
            true,
        ),
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

#[test]
fn direct_invocation_import_and_store_effects_preserve_executable_definitions() {
    use crate::code::{SnapshotBuild, parser::parse_indexed_file};
    let cases = [
        (
            "preceding_import",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
import typing
def outer():
    import mutator
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
pair=outer()
"###,
            false,
        ),
        (
            "preceding_from_import",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
import typing
def outer():
    from mutator import exported
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
pair=outer()
"###,
            false,
        ),
        (
            "deferred_import_control",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
import typing
def outer():
    def later():
        import mutator
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
pair=outer()
"###,
            true,
        ),
        (
            "local_standard_import_control",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
def outer():
    import typing
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
pair=outer()
"###,
            true,
        ),
        (
            "attribute_target",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
class Holder:
    def __setattr__(self,name,value):
        global overload,pair
        overload=identity
        pair=outer()
    def __setitem__(self,name,value):
        global overload,pair
        overload=identity
        pair=outer()
sink=Holder()
from typing import overload
def outer():
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
sink.value=outer()
"###,
            false,
        ),
        (
            "subscript_target",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
class Holder:
    def __setattr__(self,name,value):
        global overload,pair
        overload=identity
        pair=outer()
    def __setitem__(self,name,value):
        global overload,pair
        overload=identity
        pair=outer()
sink=Holder()
from typing import overload
def outer():
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
sink[0]=outer()
"###,
            false,
        ),
        (
            "simple_name_control",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
def outer():
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
pair=outer()
"###,
            true,
        ),
    ];
    for (name, source, declaration) in cases {
        let registration = crate::domain::CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            vec![],
            vec![],
        )
        .unwrap();
        let mut build =
            SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
        parse_indexed_file(&mut build, "sample.py", source.as_bytes()).unwrap();
        let snapshot = build.finish();
        let kinds = snapshot
            .symbols
            .iter()
            .filter(|s| s.name == "pick")
            .map(|s| s.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                if declaration {
                    "function_declaration"
                } else {
                    "function"
                },
                "function"
            ],
            "{name}"
        );
    }
}

#[test]
fn direct_call_shortcut_rejects_effectful_assignment_targets() {
    for (statement, safe) in [
        ("pair = outer()", true),
        ("outer()", true),
        ("sink.value = outer()", false),
        ("sink[0] = outer()", false),
        ("first, second = outer()", false),
        ("pair: registry.Type = outer()", false),
    ] {
        let source = format!("{statement}\n");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let node = tree.root_node().named_child(0).unwrap();
        assert_eq!(
            super::super::direct_function_call(&source, node, "outer", &mut 1024),
            safe,
            "{source}"
        );
        assert!(!super::super::direct_function_call(
            &source, node, "outer", &mut 0
        ));
    }
}
