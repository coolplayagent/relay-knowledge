# 多语言语法型自迭代测评集 2026-05-20

[中文](../../zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md) | [English](../../en/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md)

本文记录 `tools/self_iteration` 的多语言语法型测评集扩展。它与 [C/C++ 语法型自迭代测评集](06-c-cpp-syntax-self-iteration-evaluation.md) 共同构成本软件的代码检索测评集；新增 case 可以暂时失败，但必须代表真实语言语法和检索能力目标。修复失败时必须改进 parser、代码图事实、引用/调用解析、排序、候选预算、索引性能或通用工作流，不能枚举 query、path、symbol、repository key 或 case id。

## 生成式语法仓库

除 C/C++ 外，本次新增以下生成式 fixture。它们会由 evaluator 在 evaluation home 下创建临时 git 仓库并提交固定内容，再通过普通 `repo register/index/query` 路径测评；换环境时无需下载这些仓库。

| Repository key | Fixture version | 语言重点 |
| --- | --- | --- |
| `python_syntax_fixture` | `python_syntax_v2` | decorator、async function、async context manager、relative import、exception subclass、lambda payload filter |
| `javascript_syntax_fixture` | `javascript_syntax_v2` | ESM export/import、class method、async callback、arrow handler、registry dispatch、test fake demotion |
| `typescript_syntax_fixture` | `typescript_syntax_v2` | interface/type alias、typed arrow projector、generic function、type-only import、dynamic import、barrel export、TSX component |
| `go_syntax_fixture` | `go_syntax_v2` | receiver method、interface、grouped import alias/dot/blank、function literal、goroutine、defer、constructor flow |
| `java_syntax_fixture` | `java_syntax_v2` | generic interface、annotation、functional-interface lambda、nested builder、constructor/object creation、method override |
| `rust_syntax_fixture` | `rust_syntax_v2` | trait/impl、associated function、module import、closure dispatch、enum match flow、macro call noise |
| `bash_syntax_fixture` | `bash_syntax_v1` | sourced script、shell function、case branch、command substitution、installer dispatch |
| `csharp_syntax_fixture` | `csharp_syntax_v2` | namespace、interface generic、using directive、target-typed new、`Func<>` lambda、ArrayPool flow |
| `kotlin_syntax_fixture` | `kotlin_syntax_v2` | object、typealias、companion object、constructor/call flow、lambda handler |
| `php_syntax_fixture` | `php_syntax_v2` | namespace/use、interface、trait、constructor property promotion、arrow-function provider flow |
| `ruby_syntax_fixture` | `ruby_syntax_v2` | module/class、singleton method、require_relative、mixin、lambda runtime flow |
| `scala_syntax_fixture` | `scala_syntax_v2` | package、trait、object、inline method、import、function literal、stage/runtime flow |
| `swift_syntax_fixture` | `swift_syntax_v2` | protocol、final class、struct、import、async throws、closure request flow |

## Lambda 与 callback 覆盖

生成式 fixture 会区分语言原生 lambda 能力和语言特定 callback 等价能力：

| 语言 | 覆盖目标 |
| --- | --- |
| Python、JavaScript、TypeScript、Java、Rust、C#、Kotlin、PHP、Ruby、Scala、Swift、C++ | 原生 lambda、closure、arrow function、function literal、block 或 closure expression，并配套 `*_lambda` 评分 case |
| Go | function literal callback，并配套 `go_tree_sitter_lambda` case |
| C | C/C++ fixture 中的 function pointer typedef、operation table 和 callback dispatch |
| Bash | 无 lambda 语法；继续以 shell function 与 `case` dispatch 覆盖控制流 |

## Case 设计

- 大多数生成式 fixture 现在提供 7 条基础语法 case：`symbol`、`definition`、`imports`、`callees` 或关系流、`hybrid`、语言支持时的显式 lambda/closure case，以及 `negative`。
- `hybrid` 与关系类 case 使用 `expected_all`、`expected_sequence`、`forbidden` 或 `forbidden_rank_penalty` 保留连续评分空间，让通过后的排序、覆盖率和性能仍能继续优化。
- 生成式 fixture 默认不加入普通 fast repository 列表；需要定向验证时设置 `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS`，例如：

```bash
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=python_syntax_fixture,typescript_syntax_fixture \
  ./self-iterate.sh evaluate --profile fast --dry-run-codex
```

## 外部仓库版本

真实外部仓库仍是规模、噪声和性能测评主体。完整 clone URL、建议路径和固定 commit 记录在 [C/C++ 语法型自迭代测评集](06-c-cpp-syntax-self-iteration-evaluation.md#固定外部仓库版本) 的固定仓库表中；所有 `ref: HEAD` 已改为具体 commit SHA。换环境时按该表执行 `git clone` 和 `git checkout <sha>` 即可恢复测评数据。

## 验证命令

```bash
jq empty tools/self_iteration/cases.json tools/self_iteration/cases/*.json
cargo test --manifest-path tools/self_iteration/Cargo.toml
cargo clippy --manifest-path tools/self_iteration/Cargo.toml --all-targets -- -D warnings
./self-iterate.sh evaluate --profile smoke --dry-run-codex
```
