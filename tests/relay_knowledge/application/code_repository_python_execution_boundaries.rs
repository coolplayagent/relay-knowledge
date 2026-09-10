//! Real Git preserves executable overload bodies across Python execution boundaries.
use super::*;

#[tokio::test]
async fn python_execution_boundaries_preserve_canonical_ambiguity() {
    let repo = FixtureRepo::create("python-execution-boundaries");
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
    def await_dispatch(): return leaf()
    saved=await_dispatch
    def await_dispatch(): return final_leaf()
    return saved,await_dispatch
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
    def yield_from_dispatch(): return leaf()
    saved=yield_from_dispatch
    def yield_from_dispatch(): return final_leaf()
    return saved,yield_from_dispatch
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
    def yield_suspension(): return leaf()
    saved=yield_suspension
    def yield_suspension(): return final_leaf()
    return saved,yield_suspension
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
    def deferred_await_control(): return leaf()
    saved=deferred_await_control
    def deferred_await_control(): return final_leaf()
    return saved,deferred_await_control
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
    def empty_yield_from_control(): return leaf()
    saved=empty_yield_from_control
    def empty_yield_from_control(): return final_leaf()
    return saved,empty_yield_from_control
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
def capture_bare(): return leaf()
saved=capture_bare
def capture_bare(): return final_leaf()
pair=saved,capture_bare
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
def capture_sequence(): return leaf()
saved=capture_sequence
def capture_sequence(): return final_leaf()
pair=saved,capture_sequence
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
def capture_mapping(): return leaf()
saved=capture_mapping
def capture_mapping(): return final_leaf()
pair=saved,capture_mapping
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
def capture_wildcard_control(): return leaf()
saved=capture_wildcard_control
def capture_wildcard_control(): return final_leaf()
pair=saved,capture_wildcard_control
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
def capture_other_control(): return leaf()
saved=capture_other_control
def capture_other_control(): return final_leaf()
pair=saved,capture_other_control
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
    def direct_async(): return leaf()
    saved=direct_async
    def direct_async(): return final_leaf()
    return saved,direct_async
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
    def direct_generator(): return leaf()
    saved=direct_generator
    def direct_generator(): return final_leaf()
    yield saved,direct_generator
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
            def direct_branch(): return leaf()
            saved=direct_branch
            def direct_branch(): return final_leaf()
            return saved,direct_branch
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
    def direct_early_return(): return leaf()
    saved=direct_early_return
    def direct_early_return(): return final_leaf()
    return saved,direct_early_return
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
    def direct_sync_control(): return leaf()
    saved=direct_sync_control
    def direct_sync_control(): return final_leaf()
    return saved,direct_sync_control
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
    def nested_generator_control(): return leaf()
    saved=nested_generator_control
    def nested_generator_control(): return final_leaf()
    return saved,nested_generator_control
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
    def repeated_direct_invocation(): return leaf()
    saved=repeated_direct_invocation
    def repeated_direct_invocation(): return final_leaf()
    return saved,repeated_direct_invocation
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
    def repeated_alias_invocation(): return leaf()
    saved=repeated_alias_invocation
    def repeated_alias_invocation(): return final_leaf()
    return saved,repeated_alias_invocation
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
    def single_direct_control(): return leaf()
    saved=single_direct_control
    def single_direct_control(): return final_leaf()
    return saved,single_direct_control
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
    def nested_default_yield(): return leaf()
    saved=nested_default_yield
    def nested_default_yield(): return final_leaf()
    def nested(value=(yield None)): pass
    return saved,nested_default_yield
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
    def nested_body_yield_control(): return leaf()
    saved=nested_body_yield_control
    def nested_body_yield_control(): return final_leaf()
    return saved,nested_body_yield_control
pair=outer()
overload=identity
"###,
            true,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Python execution boundary proof"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-execution-boundaries"),
        )
        .await
        .unwrap();
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-execution-boundaries"),
        )
        .await
        .unwrap();
    for (name, _, declaration) in cases {
        let definitions = query(&service, name, CodeQueryKind::Definition).await;
        let canonical = definitions
            .results
            .iter()
            .find_map(|hit| {
                hit.canonical_symbol_id
                    .as_deref()
                    .filter(|id| id.ends_with(name))
            })
            .unwrap();
        let result = service
            .query_code_repository(
                CodeRetrievalRequest::new(
                    canonical,
                    selector("fixture", "HEAD"),
                    CodeQueryKind::Callees,
                    10,
                    FreshnessPolicy::AllowStale,
                )
                .unwrap(),
                context("query-execution-boundaries"),
            )
            .await;
        if declaration {
            let result = result.unwrap();
            assert_eq!(result.results.len(), 1, "{name}");
            assert!(
                result.results[0]
                    .canonical_symbol_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with("::final_leaf")),
                "{name}"
            );
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.error_kind, ErrorKind::InvalidArgument, "{name}");
            assert!(
                error.message.contains("multiple definitions"),
                "{name}: {}",
                error.message
            );
        }
    }
}
