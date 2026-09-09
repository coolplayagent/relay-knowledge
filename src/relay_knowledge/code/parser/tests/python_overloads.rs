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
def annotated(value: typing.Any):
    @typing.overload
    def annotated_choice(item: int): ...
    def annotated_choice(item): return item
def defaulted(value=ov):
    @ov
    def default_choice(item: int): ...
    def default_choice(item): return item
class Container:
    @typing.overload
    def method(self, value: int): ...
    def method(self, value): return value
def overload(f): return f
@overload
def custom(): return 1
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
    for name in ["custom", "rebound", "shadowed", "wildcard_shadowed"] {
        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.name == name && symbol.kind == "function"),
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
