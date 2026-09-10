//! Real Git preserves uncertainty across imports and post-call stores.
use super::*;

#[tokio::test]
async fn python_direct_call_effects_keep_canonical_definitions_ambiguous() {
    let repo = FixtureRepo::create("python-direct-effects");
    repo.write(
        "src/mutator.py",
        r###"import typing
def custom(fn): return fn
typing.overload=custom
exported=None
"###,
    );
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
    def preceding_import(): return leaf()
    saved=preceding_import
    def preceding_import(): return final_leaf()
    return saved,preceding_import
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
    def preceding_from_import(): return leaf()
    saved=preceding_from_import
    def preceding_from_import(): return final_leaf()
    return saved,preceding_from_import
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
    def deferred_import_control(): return leaf()
    saved=deferred_import_control
    def deferred_import_control(): return final_leaf()
    return saved,deferred_import_control
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
    def local_standard_import_control(): return leaf()
    saved=local_standard_import_control
    def local_standard_import_control(): return final_leaf()
    return saved,local_standard_import_control
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
    def attribute_target(): return leaf()
    saved=attribute_target
    def attribute_target(): return final_leaf()
    return saved,attribute_target
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
    def subscript_target(): return leaf()
    saved=subscript_target
    def subscript_target(): return final_leaf()
    return saved,subscript_target
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
    def simple_name_control(): return leaf()
    saved=simple_name_control
    def simple_name_control(): return final_leaf()
    return saved,simple_name_control
pair=outer()
"###,
            true,
        ),
    ];
    for (name, source, _) in cases {
        repo.write(&format!("src/{name}.py"), source);
    }
    repo.git(["add", "."]);
    repo.git([
        "commit",
        "-m",
        "Python direct call import and store effects",
    ]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-direct-effects"),
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
            context("index-direct-effects"),
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
                context("query-direct-effects"),
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
