//! CPython-reproduced suspension, capture and direct-invocation boundaries.
use crate::code::{SnapshotBuild, parser::parse_indexed_file};

#[test]
fn python_execution_boundaries_preserve_callable_bodies() {
    let cases = [
        (
            "await_dispatch",
            r###"import typing
def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
class Hook:
    def __await__(self):
        typing.overload=identity
        if False: yield
        return None
    def __iter__(self):
        typing.overload=identity
        return iter(())
async def outer(flag):
    import typing
    await flag
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
"###,
            false,
        ),
        (
            "yield_from_dispatch",
            r###"import typing
def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
class Hook:
    def __await__(self):
        typing.overload=identity
        if False: yield
        return None
    def __iter__(self):
        typing.overload=identity
        return iter(())
def outer(flag):
    import typing
    yield from flag
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
"###,
            false,
        ),
        (
            "yield_suspension",
            r###"import typing
def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
def outer():
    import typing
    yield None
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
"###,
            false,
        ),
        (
            "deferred_await_control",
            r###"import typing
def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
class Hook:
    def __await__(self):
        typing.overload=identity
        if False: yield
        return None
    def __iter__(self):
        typing.overload=identity
        return iter(())
def outer(flag):
    import typing
    async def later():
        await flag
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
"###,
            true,
        ),
        (
            "empty_yield_from_control",
            r###"import typing
def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
def outer():
    import typing
    yield from ()
    @typing.overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
"###,
            true,
        ),
        (
            "capture_bare",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
match identity:
    case overload:
        pass
@overload
def pick(): return leaf()
saved=pick
def pick(): return final_leaf()
pair=saved,pick
"###,
            false,
        ),
        (
            "capture_sequence",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
match [identity]:
    case [overload]:
        pass
@overload
def pick(): return leaf()
saved=pick
def pick(): return final_leaf()
pair=saved,pick
"###,
            false,
        ),
        (
            "capture_mapping",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
match {"decorator":identity}:
    case {"decorator":overload}:
        pass
@overload
def pick(): return leaf()
saved=pick
def pick(): return final_leaf()
pair=saved,pick
"###,
            false,
        ),
        (
            "capture_wildcard_control",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
match identity:
    case _:
        pass
@overload
def pick(): return leaf()
saved=pick
def pick(): return final_leaf()
pair=saved,pick
"###,
            true,
        ),
        (
            "capture_other_control",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
match identity:
    case other:
        pass
@overload
def pick(): return leaf()
saved=pick
def pick(): return final_leaf()
pair=saved,pick
"###,
            true,
        ),
        (
            "direct_async",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
async def outer():
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
pending=outer()
overload=identity
"###,
            false,
        ),
        (
            "direct_generator",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
def outer():
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    yield saved,pick
pending=outer()
overload=identity
"###,
            false,
        ),
        (
            "direct_branch",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
gate=False
def outer():
    match gate:
        case True:
            @overload
            def pick(): return leaf()
            saved=pick
            def pick(): return final_leaf()
            return saved,pick
outer()
gate=True
overload=identity
pair=outer()
"###,
            false,
        ),
        (
            "direct_early_return",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
gate=False
def outer():
    if not gate: return
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
outer()
gate=True
overload=identity
pair=outer()
"###,
            false,
        ),
        (
            "direct_sync_control",
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
overload=identity
"###,
            true,
        ),
        (
            "nested_generator_control",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
def outer():
    def later(): yield 1
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
pair=outer()
overload=identity
"###,
            true,
        ),
        (
            "repeated_direct_invocation",
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
overload=identity
pair=outer()
"###,
            false,
        ),
        (
            "repeated_alias_invocation",
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
overload=identity
again=outer
pair=again()
"###,
            false,
        ),
        (
            "single_direct_control",
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
overload=identity
"###,
            true,
        ),
        (
            "nested_default_yield",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
def outer():
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    def nested(value=(yield None)): pass
    return saved,pick
pending=outer()
overload=identity
"###,
            false,
        ),
        (
            "nested_body_yield_control",
            r###"def identity(fn): return fn
def leaf(): return 7
def final_leaf(): return 9
from typing import overload
def outer():
    def later(): yield 1
    @overload
    def pick(): return leaf()
    saved=pick
    def pick(): return final_leaf()
    return saved,pick
pair=outer()
overload=identity
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
