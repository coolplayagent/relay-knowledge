# C/C++ 语法型自迭代测评集 2026-05-20

[中文](../../zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md) | [English](../../en/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md)

本文记录 `tools/self_iteration` 中 C/C++ 语法型测评集的设计、固定数据版本和复现方式。该测评集是正式回归与自迭代目标，不是临时 fixture；失败应通过 parser、代码图事实、引用/调用解析、排序、候选预算或索引性能改进解决，不能通过枚举 query、path、symbol 或 case id 修复。
其他语言的生成式语法 fixture 见 [多语言语法型自迭代测评集](07-multilingual-syntax-self-iteration-evaluation.md)。

## 设计来源

- [`tree-sitter-c`](https://github.com/tree-sitter/tree-sitter-c) 与
  [`grammar.js`](https://github.com/tree-sitter/tree-sitter-c/blob/master/grammar.js)，当前 crate
  `tree-sitter-c 0.24.2`，用于 C 预处理、声明器、函数定义、调用、类型定义和 include 节点。
- [`tree-sitter-cpp`](https://github.com/tree-sitter/tree-sitter-cpp) 与
  [`grammar.js`](https://github.com/tree-sitter/tree-sitter-cpp/blob/master/grammar.js)，当前 crate
  `tree-sitter-cpp 0.23.4`，用于 C++ namespace、class、method、template、operator、lambda、new
  expression 和 include 节点。
- cppreference C function declaration / function pointer 语法说明，以及 C++ class/template/lambda/overloaded operator 语法说明。

当前依赖已经使用 docs.rs latest 版本，因此本次变更不升级 tree-sitter 依赖，先把测评压力放到 case 设计和 fixture 覆盖上。

## 新增语法 fixture

自迭代 evaluator 会在 evaluation home 下生成并提交两个小型 git 仓库，然后按普通 repository target 注册、索引和查询：

| Repository key | Fixture version | 重点语法 | Fast profile |
| --- | --- | --- | --- |
| `c_syntax_fixture` | `c_syntax_v1` | function pointer typedef、operation table、designated initializer、compound initializer、function-like macro、token paste、macro-generated handler、Nginx/Kong 风格外部头文件 typedef 与 module table、GCC `attribute((always_inline))` / `__attribute__((always_inline))` / `__always_inline` recovery、local 和 unresolved external include、callback dispatch、negative symbol | 是 |
| `cpp_syntax_fixture` | `cpp_syntax_v1` | namespace、template class、out-of-line template method、virtual override、operator overload、lambda capture、namespace alias、using alias、export-macro-decorated class、unresolved external include、header/source split、test fake demotion | 是 |

这些 fixture 的源文件由 `tools/self_iteration/src/evaluator_tail.rs` 中的常量生成；生成仓库使用固定 git author/committer date，保证内容相同情况下 commit 可重复。

C 没有原生 lambda 语法，因此 fixture 明确使用 function pointer typedef、operation table 和 callback dispatch 作为语言正确的等价覆盖。C++ 在 `RunPipeline` 中使用 captured lambda，并用 case 同时约束 `cache.Insert` 与 `pipeline(event)` 证据。

## Case 分层

新增 C case 位于 `tools/self_iteration/cases/repository_c_syntax_fixture_targets.json`，覆盖：

- `symbol`/`definition`: `struct rk_driver_ops`、`rk_driver_read`、`rk_read_fn`。
- 可恢复 C macro definition：`RK_HTTP_HANDLER(rk_http_access_handler)` 必须抽取为 definition，且文件不标为 partial。
- 外部头文件 macro recovery：`KONG_ACCESS_PHASE(ngx_http_demo_access)`、`ngx_module_t ngx_http_demo_module` 与 unresolved `#include <ngx_http.h>` 必须保留结构化事实且不降级，即使 Nginx 头文件不在 indexed scope 内。parser regression 同时覆盖 `# define`、续行 define/condition、`#undef`、inactive preprocessor branch 与有界 numeric/comparison `#if` 条件求值下的本地 macro recovery。
- GCC/EulerOS 风格编译器扩展 recovery：`attribute((always_inline))`、`__attribute__((always_inline))`、`__always_inline` 函数、PascalCase SDK 类型和 unresolved `#include "securec.h"` 必须保留结构化 parser/import evidence，且不写入 `degraded_reason`。
- `references`: `.read = rk_driver_read`、`rk_pipeline[index](dev)`、`RK_TRACE_VALUE(dev->fd)`。
- `callers`/`callees`: function pointer dispatch、operation table read callback、dispatch 调用序列。
- `imports`: 本地 `#include "driver_ops.h"` 和 unresolved external `#include <openssl/ssl.h>`，且不设置 `degraded_reason`。
- `hybrid`: operation table + callback dispatch + compound designator 组合召回。
- `negative`/`forbidden`: missing handler 空结果、macro definition 或 fake test stub 不应压过真实代码。

新增 C++ case 位于 `tools/self_iteration/cases/repository_cpp_syntax_fixture_targets.json`，覆盖：

- `symbol`/`definition`: `Cache` template class、`Cache<Key>::Insert`、`RecordingWriter::Append`、`Pipeline::operator()`。
- 可恢复 C++ decorated definition：`RK_STORE_API class HttpModule final` 必须抽取 `HttpModule`，且文件不标为 partial。
- `references`: nested `using KeyList`、namespace alias `cache_alias`。
- `callers`/`callees`: virtual `Append` dispatch、lambda capture 中的 `cache.Insert` 和 `pipeline(event)` 调用序列。
- `imports`: 本地 `#include "store/cache.hpp"` 和 unresolved external `#include <boost/asio.hpp>`，且不设置 `degraded_reason`。
- `hybrid`: template cache、out-of-line method、lambda pipeline 的多证据召回。
- `negative`/`forbidden`: missing policy 空结果、`tests/fake_cache.cpp` 不能压过 production template method。

## 固定外部仓库版本

真实大仓继续提供规模、噪声和性能压力，所有外部 repository target 均固定到下列 commit：

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

## 环境恢复

换环境时先恢复真实仓库，再运行 self-iteration。单仓示例：

```bash
git clone https://github.com/anomalyco/opencode.git /opt/workspace/opencode
git -C /opt/workspace/opencode checkout 6e4db5666ae33ebadf3b8ca077d6b1b149d0b0c3
```

SSH URL 目标需要对应 GitHub 权限；公开仓库可按同一 commit 改用 HTTPS URL。`c_syntax_fixture` 和 `cpp_syntax_fixture` 不需要下载，evaluator 每轮会在 evaluation home 中生成。

## 验证命令

```bash
jq empty tools/self_iteration/cases.json tools/self_iteration/cases/*.json
cargo test --manifest-path tools/self_iteration/Cargo.toml
./self-iterate.sh once --profile smoke --dry-run-codex
./self-iterate.sh once --profile fast --categories competitive --dry-run-codex
```
