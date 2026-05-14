# relay-teams E2E 验证 2026-05-14

[中文](../../zh/06-verification/relay-teams-e2e-2026-05-14.md) | [英文](../../en/06-verification/relay-teams-e2e-2026-05-14.md)

## 范围

使用 `/opt/workspace/relay-teams` 作为外部测试仓库，对当前 `relay-knowledge` CLI、Web 工作区、同源 Web API 和 MCP HTTP 表面进行端到端验证。

测试仓库状态：

- 分支：`improve-memory-skill-draft-status-ui`
- 提交：`fa3c0ddc9d81400b8d5e58ab7600dd557a056816`
- 影响分析使用的基线分支：`main`

运行时隔离：

- `RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/home`
- Web 绑定地址：`127.0.0.1:8791`
- MCP scope：`docs,src,frontend,relay-teams-benchmark`
- 原始命令日志：`/tmp/relay-knowledge-relay-teams-refresh-20260514-224214`

## 构建与浏览器门禁

已通过：

- `./build.sh`
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`：400 个库单元测试、1 个 benchmark 测试、41 个集成测试通过
- `cargo test --test benchmarks --all-features -- --nocapture`
- `uv run --extra dev python -m playwright install chromium`
- `uv run --extra dev pytest tests/browser`
- 针对 `http://127.0.0.1:8791` 的实时 Playwright 冒烟测试

实时浏览器检查打开了由 Rust 服务提供的真实 Web 工作区，并覆盖检索、代码状态、worker 状态和移动端布局检查。
5 次 `wait_until="networkidle"` 页面加载样本平均为 528.36ms，页面文本包含
`1653 files / 28125 symbols`，未出现 `Code graph empty`。

## CLI 覆盖

已通过：

- `--version`
- `--help`
- `status --format json`
- `health --format json`
- `service status --format json`
- `service plan install --format json`
- `service plan uninstall --format json`
- `service definition write --format json`
- `service operator status --format json`
- `service operator pause --format json`
- `service operator resume --format json`
- `ingest --source docs ... --format json`
- `query ... --freshness wait-until-fresh --format json`
- `graph inspect --format json`
- `index refresh --kind bm25 --kind semantic --kind vector --format json`
- `provider probe --format json`
- `worker status --format json`
- `worker run-once --kind extractor --format json`
- `proposal list --state proposed --format json`
- `proposal show <proposal-id> --format json`
- `proposal reject <proposal-id> --by e2e --reason ... --format json`
- `proposal accept <proposal-id> --by benchmark --reason ... --format json`
- `proposal supersede <proposal-id> --by benchmark --reason ... --format json`
- `audit query --limit 20 --format json`
- `repo register /opt/workspace/relay-teams --alias relay-teams --format json`
- `repo scope preview relay-teams --ref HEAD --format json`
- `repo index relay-teams --ref HEAD --dry-run --format json`
- `repo index relay-teams --ref HEAD --format json`
- `repo status relay-teams --format json`
- `repo report relay-teams --format json`
- `repo report relay-teams --format markdown`
- `repo query relay-teams --kind hybrid --format json`
- `repo query relay-teams --kind definition --format json`
- `repo query relay-teams --kind references --format json`
- `repo query relay-teams --kind callers --format json`
- `repo query relay-teams --kind callees --format json`
- `repo query relay-teams --kind imports --format json`
- `repo update relay-teams --base HEAD --head HEAD --format json`
- `repo impact relay-teams --base main --head HEAD --format json`

`relay-teams` 的代码索引结果：

- 已索引文件：1,653
- 符号数：28,125
- 引用数：187,993
- 代码块数：28,436
- 降级文件：218

预期的降级/默认行为：

- 由于未配置外部 embedding provider，`provider probe` 返回 `ok=false` 和
  `remote_embedding_not_configured`。本地 semantic/vector 读模型仍可用且处于 fresh 状态。

## Web 与 HTTP 覆盖

已通过：

- `GET /`
- `GET /api/project/status`
- `GET /api/health`
- `GET /api/service/status`
- `POST /api/web/operations/execute` for:
  - `retrieve.context`
  - `graph.ingest`
  - `graph.inspect`
  - `index.refresh`
  - `provider.embedding.probe`
  - `worker.status`
  - `worker.run-once`
  - `proposal.list`
  - `proposal.show`
  - `proposal.reject`
  - `proposal.accept`
  - `proposal.supersede`
  - `audit.query`
  - `code.repo.register`
  - `code.repo.index`
  - `code.repo.update`
  - `code.repo.status`
  - `code.repo.query`
  - `code.repo.impact`
  - `service.doctor`
  - `service.run.streamable_http`

Web 代码工作流也使用单独 alias `relay-teams-web` 完成验证；该 alias 指向
`/opt/workspace/relay-teams`。重复注册同一个 Git root 会保留既有
`relay-teams` alias，并把新 alias 解析到同一个 repository id。

## MCP 覆盖

针对同一个 `127.0.0.1:8791` 服务已通过：

- `initialize`
- `notifications/initialized`
- `tools/list`
- `resources/list`
- `prompts/list`
- `ping`
- `GET /mcp/metrics`

## 发现

后续性能验证见
[`docs/zh/05-benchmarks/relay-teams-baseline-2026-05-14.md`](../05-benchmarks/relay-teams-baseline-2026-05-14.md)
；该验证在全仓库索引后重新测试了由 Rust 服务提供的实时 Web 页面。仪表盘已显示仓库代码总量，未再出现先前的 `Code graph empty` 状态。

### RK-E2E-2026-05-14-1：仓库索引成功后 Web 仪表盘显示代码图为空

严重性：中

状态：已解决并重新验证。该发现保留为早期过滤 scope 运行的历史证据；当前复测中
`/api/health` 的 graph code counters 与 `repository_code_totals` 一致，实时页面显示
`1653 files / 28125 symbols`。

早期过滤 scope 运行索引 `/opt/workspace/relay-teams` 后，`/api/health` 报告
`repository_code_totals.indexed_file_count=738`,
`symbol_count=14286`、`reference_count=88082` 和 `chunk_count=14296`。
但 Web 页面仍显示：

- `Code files 0`
- `Symbols 0`
- `References 0`
- `Code graph empty`
- `0 files / 0 symbols`

影响：用户可以成功注册、索引、查询和报告代码仓库，但仪表盘摘要会让代码图看起来为空。操作编排器仍可工作，因此这更像是 Web 展示或 API 字段选择问题，而不是索引失败。

证据：

- 历史 API 输出：`/tmp/relay-knowledge-e2e-20260514092854/api_health.out`
- 历史实时页面文本 dump：
  `/tmp/relay-knowledge-e2e-20260514092854/live_page_text.out`
- 复测 API 输出：`/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/web_health.out`
- 复测实时页面文本 dump：
  `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/live_page_text.out`

### RK-E2E-2026-05-14-2：文档中的 `repo update --base main --head HEAD` 路径在非 main 分支上较脆弱

严重性：低

状态：按文档前置条件重新验证。在只索引 `HEAD` 的运行时中，
`repo update relay-teams --base main --head HEAD --format json` 仍会失败：

```text
incremental base ref 'main' resolves to 0a4e709c86f25d4fd475113f20d78f9a99498c37,
but code repository 'relay-teams' is indexed at fa3c0ddc9d81400b8d5e58ab7600dd557a056816
```

`repo update relay-teams --base HEAD --head HEAD --format json` 已通过，
`repo impact relay-teams --base main --head HEAD --format json` 也已通过。单独运行时先索引
`0a4e709c86f25d4fd475113f20d78f9a99498c37`，再更新到
`fa3c0ddc9d81400b8d5e58ab7600dd557a056816` 已通过，耗时 7.56s。

影响：用户验证 feature 分支时，如果未先索引基础引用，或未使用与已索引 scope 匹配的 base/head 组合，README 风格工作流可能失败。这很可能是预期校验行为，但文档或 CLI 错误可以更清楚地解释所需顺序。

证据：

- 失败命令：`/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/out/repo_update_main_head_after_head_indexed.err`
- 通过命令：
  `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/out/update_base_to_head.out`
