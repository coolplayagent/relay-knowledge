//! Full Python capture extraction distinguishes typed declarations from bodies.
use super::*;

#[test]
fn python_overload_declarations_require_proven_imports_and_preserve_implementations() {
    let registration = crate::domain::CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![],
        vec![],
    )
    .unwrap();
    let mut build = SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
    parse_indexed_file(
        &mut build,
        "sample.py",
        br#"
import typing
import typing_extensions as te
from typing import overload as ov
from typing import overload
@typing.overload
def choose(value: int): ...
@ov
def choose(value: str): ...
def choose(value): return leaf(value)
@overload
def imported(value: int): ...
def imported(value): return value
@te.overload
def extension(value: int): ...
def extension(value): return value
@ov
async def async_pick(value: int): ...
async def async_pick(value): return value
class Container:
    @typing.overload
    def method(self, value: int): ...
    def method(self, value): return value
def overload(f): return f
@overload
def custom(): return 1
def unstable_annotated(value: typing.Any):
    @typing.overload
    def unstable_annotation_choice(item: int): ...
    def unstable_annotation_choice(item): return item
    return unstable_annotation_choice
def unstable_defaulted(value=ov):
    @ov
    def unstable_default_choice(item: int): ...
    def unstable_default_choice(item): return item
    return unstable_default_choice
typing = object()
@typing.overload
def rebound(): return 1
def factory(ov):
    @ov
    def shadowed(): return 1
    return shadowed
from custom import *
@ov
def wildcard_shadowed(): return 1
"#,
    )
    .unwrap();
    // Deferred positive lookups require a namespace with no later rebinding.
    parse_indexed_file(
        &mut build,
        "stable.py",
        br#"import typing
from typing import overload as ov
def annotated(value: typing.Any):
    @typing.overload
    def annotated_choice(item: int): ...
    def annotated_choice(item): return item
def defaulted(value=ov):
    @ov
    def default_choice(item: int): ...
    def default_choice(item): return item
"#,
    )
    .unwrap();
    let snapshot = build.finish();
    for (name, declarations) in [
        ("choose", 2),
        ("imported", 1),
        ("extension", 1),
        ("method", 1),
        ("async_pick", 1),
        ("annotated_choice", 1),
        ("default_choice", 1),
    ] {
        let symbols = snapshot
            .symbols
            .iter()
            .filter(|symbol| symbol.name == name)
            .collect::<Vec<_>>();
        assert_eq!(
            symbols
                .iter()
                .filter(|symbol| symbol.kind == "function_declaration")
                .count(),
            declarations,
            "{symbols:?}"
        );
        assert_eq!(
            symbols
                .iter()
                .filter(|symbol| symbol.kind != "function_declaration")
                .count(),
            1
        );
    }
    for name in [
        "custom",
        "rebound",
        "shadowed",
        "wildcard_shadowed",
        "unstable_annotation_choice",
        "unstable_default_choice",
    ] {
        assert!(
            snapshot.symbols.iter().any(|symbol| symbol.name == name),
            "{name}"
        );
        assert!(
            snapshot
                .symbols
                .iter()
                .filter(|symbol| symbol.name == name)
                .all(|symbol| symbol.kind == "function"),
            "{name}"
        );
    }
}

#[test]
fn python_nested_scopes_skip_outer_class_imports_but_keep_immediate_class_decorators() {
    let registration = crate::domain::CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![],
        vec![],
    )
    .unwrap();
    let mut build = SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
    parse_indexed_file(
        &mut build,
        "scope.py",
        br#"
def overload(fn): return fn
class Worker:
    from typing import overload
    @overload
    def direct(self, value: int): ...
    def direct(self, value): return value
    def method(self):
        @overload
        def nested(): return 1
        def nested(): return 2
class Outer:
    from typing import overload
    class Inner:
        @overload
        def inner(self): return 1
        def inner(self): return 2
def factory():
    class Local:
        from typing import overload
        @overload
        def local(self, value: int): ...
        def local(self, value): return value
class Conditional:
    if True:
        from typing import overload
        @overload
        def conditional_direct(self, value: int): ...
        def conditional_direct(self, value): return value
        def method(self):
            @overload
            def flow(): return 1
            def flow(): return 2
"#,
    )
    .unwrap();
    let snapshot = build.finish();
    for name in ["nested", "inner", "flow"] {
        let kinds = snapshot
            .symbols
            .iter()
            .filter(|symbol| symbol.name == name)
            .map(|symbol| symbol.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["function", "function"], "{name}");
    }
    for name in ["direct", "local", "conditional_direct"] {
        let kinds = snapshot
            .symbols
            .iter()
            .filter(|symbol| symbol.name == name)
            .map(|symbol| symbol.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["function_declaration", "function"], "{name}");
    }
}

#[test]
fn python_binding_directives_resolve_the_declared_namespace() {
    let registration = crate::domain::CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![],
        vec![],
    )
    .unwrap();
    let mut build = SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
    parse_indexed_file(
        &mut build,
        "sample.py",
        br#"from typing import overload, get_overloads
global overload
def leaf(): return 1
def outer():
    overload = lambda fn: fn
    def inner():
        global overload
        @overload
        def global_choice(x: int): ...
        def global_choice(x): return leaf()
        return global_choice
    return inner()
def enclosing():
    from typing import overload
    def inner():
        nonlocal overload
        @overload
        def nonlocal_choice(x: int): ...
        def nonlocal_choice(x): return leaf()
        return nonlocal_choice
    return inner()
"#,
    )
    .unwrap();
    let snapshot = build.finish();
    for name in ["global_choice", "nonlocal_choice"] {
        let kinds = snapshot
            .symbols
            .iter()
            .filter(|symbol| symbol.name == name)
            .map(|symbol| symbol.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["function_declaration", "function"], "{name}");
    }
}

#[test]
fn python_redirected_later_writes_and_attribute_targets_preserve_runtime_bodies() {
    let registration = crate::domain::CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![],
        vec![],
    )
    .unwrap();
    let mut build = SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
    parse_indexed_file(
        &mut build,
        "sample.py",
        br#"from typing import overload, get_overloads
from typing import overload as global_ov
from types import SimpleNamespace
events=[]
def custom(fn): events.append(fn.__qualname__); return fn
def leaf(): return 1
registry=SimpleNamespace()
registry.overload=custom
@overload
def attribute_choice(x:int): ...
def attribute_choice(x): return leaf()
def global_outer():
    global global_ov
    @global_ov
    def global_choice(): return leaf()
    def global_choice(): return leaf()
    return global_choice
def nonlocal_outer():
    from typing import overload
    def inner():
        nonlocal overload
        @overload
        def nonlocal_choice(): return leaf()
        def nonlocal_choice(): return leaf()
        return nonlocal_choice
    overload=custom
    return inner()
def implicit_global_outer():
    @global_ov
    def implicit_global_choice(): return leaf()
    def implicit_global_choice(): return leaf()
    return implicit_global_choice
def implicit_nonlocal_outer():
    from typing import overload
    def inner():
        @overload
        def implicit_nonlocal_choice(): return leaf()
        def implicit_nonlocal_choice(): return leaf()
        return implicit_nonlocal_choice
    overload=custom
    return inner()
global_ov=custom
global_outer()
nonlocal_outer()
import typing as property_module
import typing as subscript_module
import typing as mutation_module
from typing import get_overloads
from types import SimpleNamespace
events=[]
registry=SimpleNamespace()
registry.property_module=custom
@property_module.overload
def module_property_choice(x:int): ...
def module_property_choice(x): return leaf()
mapping={}
mapping[subscript_module]=custom
@subscript_module.overload
def module_subscript_choice(x:int): ...
def module_subscript_choice(x): return leaf()
mutation_module.overload=custom
@mutation_module.overload
def module_mutation_choice(): return leaf()
def module_mutation_choice(): return leaf()
"#,
    )
    .unwrap();
    let snapshot = build.finish();
    for name in [
        "attribute_choice",
        "global_choice",
        "nonlocal_choice",
        "implicit_global_choice",
        "implicit_nonlocal_choice",
        "module_property_choice",
        "module_subscript_choice",
        "module_mutation_choice",
    ] {
        let kinds = snapshot
            .symbols
            .iter()
            .filter(|s| s.name == name)
            .map(|s| s.kind.as_str())
            .collect::<Vec<_>>();
        let expected = if matches!(
            name,
            "attribute_choice" | "module_property_choice" | "module_subscript_choice"
        ) {
            vec!["function_declaration", "function"]
        } else {
            vec!["function", "function"]
        };
        assert_eq!(kinds, expected, "{name}");
    }
}
