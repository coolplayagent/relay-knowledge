# Multilingual Syntax Self-Iteration Evaluation Set 2026-05-20

[English](../../en/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md) | [中文](../../zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md)

This page records the multilingual syntax-focused expansion of `tools/self_iteration`. Together with the [C/C++ syntax evaluation set](06-c-cpp-syntax-self-iteration-evaluation.md), these cases form the product code-retrieval evaluation set. New cases may fail at first, but they must represent real language syntax and retrieval goals. Fixes must improve parser extraction, code graph facts, reference/call resolution, ranking, candidate budgets, indexing performance, or general workflows; they must not enumerate query strings, paths, symbols, repository keys, or case ids.

## Generated Syntax Repositories

Beyond C/C++, the evaluator now generates these fixture repositories under the evaluation home, commits deterministic source content, and evaluates them through the normal `repo register/index/query` path. They do not need to be cloned when moving to a new environment.

| Repository key | Fixture version | Language focus |
| --- | --- | --- |
| `python_syntax_fixture` | `python_syntax_v2` | decorators, async functions, async context managers, relative imports, exception subclasses, lambda payload filters |
| `javascript_syntax_fixture` | `javascript_syntax_v2` | ESM export/import, class methods, async callbacks, arrow handlers, registry dispatch, test fake demotion |
| `typescript_syntax_fixture` | `typescript_syntax_v2` | interfaces/type aliases, typed arrow projectors, generic functions, type-only imports, dynamic imports, barrel exports, TSX components |
| `go_syntax_fixture` | `go_syntax_v2` | receiver methods, interfaces, grouped import alias/dot/blank forms, function literals, goroutines, defer, constructor flow |
| `java_syntax_fixture` | `java_syntax_v2` | generic interfaces, annotations, functional-interface lambdas, nested builders, constructor/object creation, method overrides |
| `rust_syntax_fixture` | `rust_syntax_v2` | traits/impls, associated functions, module imports, closure dispatch, enum match flow, macro-call noise |
| `bash_syntax_fixture` | `bash_syntax_v1` | sourced scripts, shell functions, case branches, command substitution, installer dispatch |
| `csharp_syntax_fixture` | `csharp_syntax_v2` | namespaces, generic interfaces, using directives, target-typed new, `Func<>` lambdas, ArrayPool flow |
| `kotlin_syntax_fixture` | `kotlin_syntax_v2` | objects, typealiases, companion objects, constructor/call flow, lambda handlers |
| `php_syntax_fixture` | `php_syntax_v2` | namespaces/use imports, interfaces, traits, constructor property promotion, arrow-function provider flow |
| `ruby_syntax_fixture` | `ruby_syntax_v2` | modules/classes, singleton methods, require_relative, mixins, lambda runtime flow |
| `scala_syntax_fixture` | `scala_syntax_v2` | packages, traits, objects, inline methods, imports, function literals, stage/runtime flow |
| `swift_syntax_fixture` | `swift_syntax_v2` | protocols, final classes, structs, imports, async throws, closure request flow |

## Lambda And Callback Coverage

The generated fixtures now distinguish native lambda support from language-specific callback equivalents:

| Language | Coverage target |
| --- | --- |
| Python, JavaScript, TypeScript, Java, Rust, C#, Kotlin, PHP, Ruby, Scala, Swift, C++ | Native lambda, closure, arrow function, function literal, block, or closure-expression syntax with a scored `*_lambda` case |
| Go | Function literal callback syntax with a scored `go_tree_sitter_lambda` case |
| C | Function pointer typedefs, operation tables, and callback dispatch in the C/C++ fixture set |
| Bash | No lambda syntax; shell functions and `case` dispatch remain the intended control-flow coverage |

## Case Design

- Most generated fixtures now provide 7 core syntax cases: `symbol`, `definition`, `imports`, `callees` or relationship flow, `hybrid`, an explicit lambda/closure case where the language supports one, and `negative`.
- Hybrid and relationship cases use `expected_all`, `expected_sequence`, `forbidden`, or `forbidden_rank_penalty` to preserve continuous scoring pressure after basic pass/fail is achieved.
- Generated fixtures are not added to the normal fast repository list by default. Run targeted checks with `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS`, for example:

```bash
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=python_syntax_fixture,typescript_syntax_fixture \
  ./self-iterate.sh evaluate --profile fast --dry-run-codex
```

## External Repository Versions

Real external repositories remain the main scale, noise, and performance corpora. The complete clone URL, suggested path, and pinned commit table is recorded in the [C/C++ syntax evaluation set](06-c-cpp-syntax-self-iteration-evaluation.md#pinned-external-repositories). All external `ref: HEAD` values have been replaced by commit SHAs. To move environments, follow that table with `git clone` and `git checkout <sha>`.

## Verification Commands

```bash
jq empty tools/self_iteration/cases.json tools/self_iteration/cases/*.json
cargo test --manifest-path tools/self_iteration/Cargo.toml
cargo clippy --manifest-path tools/self_iteration/Cargo.toml --all-targets -- -D warnings
./self-iterate.sh evaluate --profile smoke --dry-run-codex
```
