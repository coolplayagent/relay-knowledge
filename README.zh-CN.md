[English](README.md) | [中文](README.zh-CN.md)

# relay-knowledge

`relay-knowledge` 是一个本地优先、基于图能力的知识检索底座。它存储证据、
图事实、代码仓库结构、派生索引、新鲜度状态、诊断、审计记录和面向 agent 的
上下文包。它不是通用 agent 运行时，也不负责生成最终答案。

## 快速开始

默认本地配置不依赖外部服务：运行时目录使用平台默认位置，SQLite 保存本地状态，
并启用确定性的本地 semantic/vector 读模型。

```bash
cargo build
target/debug/relay-knowledge status
target/debug/relay-knowledge ingest --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust
target/debug/relay-knowledge query SQLite --source docs \
  --freshness wait-until-fresh
```

脚本和 agent 集成应使用 JSON 输出：

```bash
target/debug/relay-knowledge status --format json
target/debug/relay-knowledge health --format json
target/debug/relay-knowledge help --format json
```

## 安装发布版

[GitHub Releases](https://github.com/coolplayagent/relay-knowledge/releases)
提供 Linux x64/ARM64、macOS Intel/Apple Silicon 和 Windows x64/ARM64
预构建压缩包。将二进制放入 `PATH` 前，应使用 `checksums.txt` 校验所选压缩包；
GitHub artifact attestation 覆盖同一组压缩包摘要。Linux GNU 压缩包以
glibc 2.28 为 baseline。

Rust 用户也可以从 crates.io 安装：

```bash
cargo install relay-knowledge
relay-knowledge --version
relay-knowledge service doctor
```

每个 release 还会发布 `relay-knowledge-cli-skill-<tag>.tar.gz`，供 agent
通过 CLI 而不是 MCP/ACP 使用本地图谱。平台细节、校验、服务安装、升级、回滚和
卸载合同见 [CLI skill 包](skills/relay-knowledge-cli/README.md)与
[安装、发布与升级](docs/zh/03-architecture-specs/19-installation-release-and-upgrade.md)。

## 能力概览

- 混合 GraphRAG 上下文包组合 BM25、本地或外部 semantic/vector 检索、
  图证据、新鲜度、有界上下文和排序解释。
- 结构化 evidence、entity、relation、claim、event、source span、
  confidence、graph version 和 accepted/proposed grounding 全程可追踪。
- 仓库工作流覆盖注册、tree-sitter 索引、全量与增量刷新、worktree overlay、
  symbol、reference、call、import、context、impact、feature flag、SBOM
  证据和多仓库 set。
- 持久有界队列、lease、checkpoint、backpressure、恢复和可观测维护保护长时间
  索引与后台工作。
- 软件全域投影和授权的本地文件索引暴露 dependency、SDK、file、topic、
  build/IaC/design 证据和 relationship，无需在查询时扫描仓库。
- CLI、Web、MCP Streamable HTTP 和本地 ACP 共享相同应用行为、scope policy、
  QoS、取消、审计和诊断。

行为细节、限制和实现职责属于下列按职责组织的文档，不在这个导航页重复。

## 文档

| 范围 | 入口 |
| --- | --- |
| 完整书架 | [中文文档](docs/zh/README.md) |
| 用户工作流 | [使用指南](docs/zh/01-user-guide/README.md) |
| 已实现行为 | [能力说明](docs/zh/02-capabilities/README.md) |
| 架构合同 | [架构规格](docs/zh/03-architecture-specs/README.md) |
| 强制工程规则 | [工程硬约束](docs/zh/03-architecture-specs/02-engineering-hard-constraints.md) |
| 研究与外部证据 | [研究资料](docs/zh/04-research/README.md) |
| 性能与自迭代合同 | [基准记录](docs/zh/05-benchmarks/README.md) |
| 可审计验收记录 | [验证记录](docs/zh/06-verification/README.md) |

开发闭环的两章职责不同：

- [第 24 章：Code Map 驱动的知识开发闭环](docs/zh/03-architecture-specs/24-code-map-backed-knowledge-development-loop.md)
- [第 27 章：业务知识与技术图谱映射](docs/zh/03-architecture-specs/27-business-knowledge-technical-mapping.md)
  是可执行的操作合同。
- [第 26 章：Git Commit + Knowledge 开发迭代理念与 Loop](docs/zh/03-architecture-specs/26-git-commit-knowledge-development-loop.md)
  独立说明 commit 事实边界、派生 knowledge、决策上下文、恢复模型和人机交接理念。

## 核心 CLI 工作流

机器可读 help 是命令合同：

```bash
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

创建和查询知识：

```bash
relay-knowledge ingest --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge graph inspect --format json
```

注册、索引和查询代码仓库：

```bash
relay-knowledge repo register /path/to/repository --path src --format json
relay-knowledge repo index repository --ref HEAD --format json
relay-knowledge repo status repository --format json
relay-knowledge repo query repository --query retry_policy \
  --kind definition --ref HEAD --path src --freshness wait-until-fresh \
  --limit 10 --format json
relay-knowledge repo software repository --kind relationships \
  --ref HEAD --format json
```

索引会返回持久 task，并通过 `repo status` 暴露进度。如果一次性 CLI 在调用方超时前
无法完成大仓冷索引，应先检查 status，再使用
[代码仓库图谱工作流](docs/zh/01-user-guide/05-code-repository-graph-workflow.md)
记录的有界 task worker 或托管服务恢复路径；不要启动无人管理的 loop 或竞争 writer。

查询常驻服务时不打开无关本地状态：

```bash
relay-knowledge --remote http://127.0.0.1:8791 \
  repo query repository --query retry_policy --kind definition \
  --freshness wait-until-fresh --format json
```

完整 grammar、各命令自己的 `--kind` 取值、JSON schema、读写影响和环境变量优先级见
[CLI 命令参考](docs/zh/01-user-guide/03-cli-command-reference.md)。

## 常驻服务与 Agent 接入

启动共享 Web/API 服务并显式启用 MCP Streamable HTTP：

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --web --mcp streamable-http
```

默认 Web 地址为 `http://127.0.0.1:8791/`，MCP 地址为
`http://127.0.0.1:8791/mcp`。MCP 默认关闭；图工具要求允许的 scope
或已显式注册的仓库 alias。

Session、授权、取消、审计、平台服务管理器和诊断说明见
[Web 工作区](docs/zh/01-user-guide/06-web-workspace.md)、
[MCP 与 Agent 接入](docs/zh/01-user-guide/07-mcp-agent-access.md)和
[常驻服务](docs/zh/01-user-guide/09-resident-service.md)。

## 开发

按职责使用仓库脚本：

```bash
./setup.sh
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
./check.sh
./check.sh --deep
```

主要本地质量门禁：

```bash
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
python3 tools/docs/check_docs.py --self-test-and-check
```

默认 `./check.sh` 使用 stable 工具链，适合日常改动。deep profile 还会执行
确定性 benchmark、针对无 FFI 核心领域不变量的 Miri，以及覆盖 library 和 binary
的 AddressSanitizer。先安装 nightly 前置组件：

```bash
rustup toolchain install nightly --profile minimal --component miri,rust-src
./check.sh --deep
```

Miri 和 sanitizer 也作为 Linux pull request job 自动运行。它们依赖 nightly、
需要重建插桩产物且明显慢于 stable check/Clippy/test，因此不进入普通 commit hook。

架构边界、async 与资源预算、UT 覆盖率、文档完整性和手写文件少于 1,000 行的要求都属于
[工程硬约束](docs/zh/03-architecture-specs/02-engineering-hard-constraints.md)，
不是可选建议。

### 自迭代 Harness

检索与索引优化使用独立 Rust harness，详见
[tools/self_iteration](tools/self_iteration/README.zh-CN.md)：

```bash
./self-iterate.sh
./self-iterate.sh once
./self-iterate.sh loop --strategy unattended-layered
./self-iterate.sh chart
```

默认 `fast` profile 构建并评估 release 产品 binary，运行聚焦门禁和 workload
护栏。完整门禁和 workload 使用
`./self-iterate.sh once --profile full`。运行历史、报告、patch 和 resume state
保存在 `.git/relay-knowledge-self-iteration/`。Harness 文档同时记录外部仓库的
精确固定 commit，以及可复现的 detached-checkout 准备方法。

### 浏览器检查

```bash
./build.sh
./run.sh start --port 8791 --daemon
curl http://127.0.0.1:8791/api/health
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

运行数据、配置、索引、日志和缓存应位于文档规定的平台目录，而不是仓库内。
不得提交 secret、本地数据库、私有数据集或生成的构建产物。详见
[安装与运行时目录](docs/zh/01-user-guide/01-install-and-runtime.md)。

可选本地 hook：`pre-commit install` 和
`pre-commit run --all-files`。Rust 改动在 commit 前执行 `cargo check`、Clippy
和 tests；test 命令通过 `--all-targets` 同时覆盖确定性 benchmark target。
