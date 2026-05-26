# C/C++ Syntax Self-Iteration Evaluation Set 2026-05-20

[English](../../en/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md) | [中文](../../zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md)

This page records the C/C++ syntax-focused evaluation set used by `tools/self_iteration`. These cases are product evaluation targets, not temporary fixtures. Failures should be addressed through parser extraction, code graph facts, reference/call resolution, ranking, candidate budgets, or indexing performance, not by enumerating query strings, paths, symbols, or case ids.
Generated syntax fixtures for other languages are documented in the [multilingual syntax evaluation set](07-multilingual-syntax-self-iteration-evaluation.md).

## Design Sources

- [`tree-sitter-c`](https://github.com/tree-sitter/tree-sitter-c) and
  [`grammar.js`](https://github.com/tree-sitter/tree-sitter-c/blob/master/grammar.js), with the current
  `tree-sitter-c 0.24.2` crate for C preprocessor, declarator, function definition, call, type definition, and
  include nodes.
- [`tree-sitter-cpp`](https://github.com/tree-sitter/tree-sitter-cpp) and
  [`grammar.js`](https://github.com/tree-sitter/tree-sitter-cpp/blob/master/grammar.js), with the current
  `tree-sitter-cpp 0.23.4` crate for C++ namespaces, classes, methods, templates, operators, lambdas, new
  expressions, and includes.
- cppreference C function declaration/function pointer syntax and C++ class/template/lambda/overloaded operator syntax.

The current dependency versions already match docs.rs latest, so this change keeps dependencies stable and increases evaluation pressure through case design.

## Generated Syntax Repositories

The evaluator creates two small git repositories under the evaluation home, commits deterministic source content, then registers and indexes them like normal repository targets:

| Repository key | Fixture version | Syntax focus | Fast profile |
| --- | --- | --- | --- |
| `c_syntax_fixture` | `c_syntax_v1` | function pointer typedefs, operation tables, designated initializers, compound initializers, function-like macros, token paste, macro-generated handlers, Nginx/Kong-style external-header typedefs and module tables, local and unresolved external includes, callback dispatch, negative symbols | Yes |
| `cpp_syntax_fixture` | `cpp_syntax_v1` | namespaces, template classes, out-of-line template methods, virtual overrides, overloaded operators, lambda captures, namespace aliases, using aliases, export-macro-decorated classes, unresolved external includes, header/source split, test fake demotion | Yes |

The fixture sources are generated from constants in `tools/self_iteration/src/evaluator_tail.rs`; the git author and committer dates are fixed so the generated commits are repeatable for unchanged content.

C has no native lambda syntax, so its fixture intentionally uses function pointer typedefs, operation tables, and callback dispatch as the language-correct equivalent. C++ uses a captured lambda in `RunPipeline`, with cases that require both `cache.Insert` and `pipeline(event)` evidence.

## Case Families

C cases live in `tools/self_iteration/cases/repository_c_syntax_fixture_targets.json` and cover:

- `symbol`/`definition`: `struct rk_driver_ops`, `rk_driver_read`, and `rk_read_fn`.
- Recoverable C macro definitions: `RK_HTTP_HANDLER(rk_http_access_handler)` must be extracted as a definition without marking the file partial.
- External-header macro recovery: `KONG_ACCESS_PHASE(ngx_http_demo_access)`, `ngx_module_t ngx_http_demo_module`, and unresolved `#include <ngx_http.h>` must stay structured and non-degraded even though the external Nginx headers are not part of the indexed scope. Parser regression tests also cover spaced `# define`, continued definitions, `#undef`, inactive preprocessor branches, and bounded numeric `#if` condition evaluation for this local macro recovery path.
- `references`: `.read = rk_driver_read`, `rk_pipeline[index](dev)`, and `RK_TRACE_VALUE(dev->fd)`.
- `callers`/`callees`: function pointer dispatch, operation-table callbacks, and dispatch call sequences.
- `imports`: local `#include "driver_ops.h"` and unresolved external `#include <openssl/ssl.h>` with no `degraded_reason`.
- `hybrid`: operation table, callback dispatch, and compound designator evidence.
- `negative`/`forbidden`: missing handler empty results and macro/test fake demotion.

C++ cases live in `tools/self_iteration/cases/repository_cpp_syntax_fixture_targets.json` and cover:

- `symbol`/`definition`: `Cache` template class, `Cache<Key>::Insert`, `RecordingWriter::Append`, and `Pipeline::operator()`.
- Recoverable C++ decorated definitions: `RK_STORE_API class HttpModule final` must extract `HttpModule` without marking the file partial.
- `references`: nested `using KeyList` and namespace alias `cache_alias`.
- `callers`/`callees`: virtual `Append` dispatch and lambda capture sequences with `cache.Insert` plus `pipeline(event)`.
- `imports`: local `#include "store/cache.hpp"` and unresolved external `#include <boost/asio.hpp>` with no `degraded_reason`.
- `hybrid`: template cache, out-of-line method, and lambda pipeline evidence.
- `negative`/`forbidden`: missing policy empty results and `tests/fake_cache.cpp` demotion.

## Pinned External Repositories

Real repositories still provide scale, noise, and performance pressure. Every external repository target is pinned:

| Repository key | Clone URL | Suggested path | Commit |
| --- | --- | --- | --- |
| `relay_teams` | `git@github.com:coolplayagent/relay-teams.git` | `/opt/workspace/relay-teams` | `39dda9f8905d01951dae6ed6fe9c09c4c92896e2` |
| `opencode_typescript` | `https://github.com/anomalyco/opencode.git` | `/opt/workspace/opencode` | `6e4db5666ae33ebadf3b8ca077d6b1b149d0b0c3` |
| `linux_sample`, `linux_full` | `git@github.com:torvalds/linux.git` | `/opt/workspace/linux` | `70eda68668d1476b459b64e69b8f36659fa9dfa8` |
| `leveldb_cpp` | `git@github.com:google/leveldb.git` | `/opt/workspace/leveldb` | `7ee830d02b623e8ffe0b95d59a74db1e58da04c5` |
| `temporal_samples_go` | `https://github.com/temporalio/samples-go.git` | `/opt/workspace/temporal-samples-go` | `231564bebe0be78e78233ef14992158c623d1e86` |
| `temporal_sdk_go` | `https://github.com/temporalio/sdk-go.git` | `/opt/workspace/temporal-sdk-go` | `ff47f19909ac85aacff89645360de0dba6f6f898` |
| `otel_collector_contrib` | `https://github.com/open-telemetry/opentelemetry-collector-contrib.git` | `/opt/workspace/opentelemetry-collector-contrib` | `84fe8df16c34efbb7e929310c955df8f4861d2f4` |
| `otel_collector` | `https://github.com/open-telemetry/opentelemetry-collector.git` | `/opt/workspace/opentelemetry-collector` | `31e51520f30fc5c4362949e41307ea57b7b45a9d` |
| `kubernetes_go_sample` | `git@github.com:kubernetes/kubernetes.git` | `/opt/workspace/kubernetes` | `016a2bcfa48d4a56059ee5e878eb208ffccdb773` |
| `spring_framework_java` | `git@github.com:spring-projects/spring-framework.git` | `/opt/workspace/spring-framework` | `2f458f909391b04eb138aba8980598dc4b0cf4a3` |
| `rustfs_rust` | `https://github.com/rustfs/rustfs.git` | `/opt/workspace/rustfs` | `a66337bd289f41968d454bdfb93892abd022a42f` |
| `codex_python` | `https://github.com/openai/codex.git` | `/opt/workspace/codex` | `24c598e8a9efdd7b9de2dd8c935f7204c1c7c414` |
| `nvm_bash` | `https://github.com/nvm-sh/nvm.git` | `/opt/workspace/nvm` | `53855417eb66b9c35b732ac39358f1aae3ee1977` |
| `dotnet_runtime_csharp` | `https://github.com/dotnet/runtime.git` | `/opt/workspace/dotnet-runtime` | `86db03a9c145cefc46fbe9e0f0dc646f739c606c` |
| `okhttp_kotlin` | `https://github.com/square/okhttp.git` | `/opt/workspace/okhttp` | `1d9a8ba6c335355da9c71586abf82c9516e1bac5` |
| `laravel_php` | `https://github.com/laravel/framework.git` | `/opt/workspace/laravel-framework` | `f05ef246c22eac49c7c7e9b2815449873ccd8a22` |
| `rails_ruby` | `https://github.com/rails/rails.git` | `/opt/workspace/rails` | `a78f8bcaac1d6f10a515aeccfb6553b895f126c3` |
| `scala3_scala` | `https://github.com/scala/scala3.git` | `/opt/workspace/scala3` | `c101b01b41f8780122caffcc03e0f395edc8016e` |
| `alamofire_swift` | `https://github.com/Alamofire/Alamofire.git` | `/opt/workspace/alamofire` | `7595cbcf59809f9977c5f6378500de2ad73b7ddb` |

## Restoring An Environment

Clone and checkout the pinned commit before running self-iteration. Example:

```bash
git clone https://github.com/anomalyco/opencode.git /opt/workspace/opencode
git -C /opt/workspace/opencode checkout 6e4db5666ae33ebadf3b8ca077d6b1b149d0b0c3
```

SSH targets require matching GitHub permissions; public repositories can use HTTPS URLs for the same commit. `c_syntax_fixture` and `cpp_syntax_fixture` are generated by the evaluator and do not need to be cloned.

## Verification Commands

```bash
jq empty tools/self_iteration/cases.json tools/self_iteration/cases/*.json
cargo test --manifest-path tools/self_iteration/Cargo.toml
./self-iterate.sh once --profile smoke --dry-run-codex
./self-iterate.sh once --profile fast --categories competitive --dry-run-codex
```
