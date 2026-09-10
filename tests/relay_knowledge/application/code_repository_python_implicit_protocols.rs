//! Real Git query selection respects implicit Python execution boundaries.
use super::*;
#[tokio::test]
async fn implicit_protocols_preserve_executable_canonical_ambiguity() {
    let repo = FixtureRepo::create("python-implicit-protocols");
    let cases = [
        ("module_hook", "typing.__getattr__ = flag", false),
        (
            "module_dict_hook",
            "typing.__dict__['__getattr__'] = flag",
            false,
        ),
        (
            "module_setattr_hook",
            "setattr(typing, '__getattr__', flag)",
            false,
        ),
        ("truth", "if flag:\n        pass", false),
        ("iteration", "for item in flag:\n        pass", false),
        ("context", "with flag:\n        pass", false),
        ("operator", "value = flag + 1", false),
        ("descriptor", "value = flag.value", false),
        ("subscription", "value = flag[0]", false),
        ("comparison", "value = flag == 1", false),
        ("formatting", "value = f\"{flag}\"", false),
        ("unpacking", "left, right = flag", false),
        ("boolean_operator", "value = flag and 1", false),
        (
            "generator_iteration",
            "value = (item for item in flag)",
            false,
        ),
        ("augmented_operator", "flag += 1", false),
        ("dictionary_hash", "value = {flag: 1}", false),
        ("set_hash", "value = {flag}", false),
        (
            "alias_other",
            "alias = typing\n    alias.other = flag",
            true,
        ),
        (
            "chain_other",
            "a = alias = typing\n    alias.other = flag",
            true,
        ),
        (
            "conditional_alias",
            "if True:\n        alias = typing\n    alias.other = flag",
            true,
        ),
        ("dict_other", "typing.__dict__['other'] = flag", true),
        (
            "parenthesized_dict",
            "((typing)).__dict__['other'] = flag",
            true,
        ),
        (
            "function_truth",
            "from typing import overload\n    overload and True",
            true,
        ),
        (
            "container_iteration",
            "for item in [flag]:\n        pass",
            true,
        ),
        (
            "literal_mapping",
            "mapping = {}\n    mapping[typing] = flag",
            true,
        ),
        (
            "registry_attribute",
            "class Registry: pass\n    registry = Registry()\n    import typing\n    registry.other = flag",
            true,
        ),
        (
            "registry_for",
            "class Registry: pass\n    registry = Registry()\n    import typing\n    for registry.other in [flag]:\n        pass",
            true,
        ),
        (
            "external_receiver",
            "from types import SimpleNamespace\n    registry = SimpleNamespace()\n    import typing\n    registry.other = flag",
            false,
        ),
        (
            "custom_setter",
            "class Registry:\n        def __setattr__(self, name, value): pass\n    registry = Registry()\n    import typing\n    registry.other = flag",
            false,
        ),
        ("literal_truth", "if True:\n        pass", true),
        (
            "literal_iteration",
            "for item in (1, 2):\n        pass",
            true,
        ),
        ("literal_operator", "value = 1 + 2", true),
        ("typing_attribute", "value = typing.Any", true),
        ("literal_dictionary_key", "value = {'plain': flag}", true),
        ("literal_set", "value = {1, 2}", true),
        (
            "deferred_body",
            "def later():\n        if flag:\n            pass",
            true,
        ),
    ];
    for (name, statement, _) in cases {
        let source = format!(
            "def leaf(): return 17\ndef final_leaf(): return 19\ndef outer(flag):\n    import typing\n    {statement}\n    @typing.overload\n    def {name}(): return leaf()\n    def {name}(): return final_leaf()\n"
        );
        repo.write(&format!("src/{name}.py"), &source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Python binding target and syntax cases"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-full-origin-inventory"),
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
            context("index-chains"),
        )
        .await
        .unwrap();
    for (name, _, typed) in cases {
        let definitions = query(&service, name, CodeQueryKind::Definition).await;
        let canonical = definitions
            .results
            .iter()
            .find_map(|h| {
                h.canonical_symbol_id
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
                context("query-chains"),
            )
            .await;
        if typed {
            assert_eq!(result.unwrap().results.len(), 1, "{name}");
        } else {
            assert_eq!(
                result.unwrap_err().error_kind,
                ErrorKind::InvalidArgument,
                "{name}"
            );
        }
    }
}
