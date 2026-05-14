# relay-teams 基线 2026-05-14

[中文](../../zh/05-benchmarks/relay-teams-baseline-2026-05-14.md) | [英文](../../en/05-benchmarks/relay-teams-baseline-2026-05-14.md)

日期：2026-05-14

测试仓库：`/opt/workspace/relay-teams`

- 分支：`improve-memory-skill-draft-status-ui`
- HEAD: `fa3c0ddc9d81400b8d5e58ab7600dd557a056816`
- 增量测试的基础引用：`0a4e709c86f25d4fd475113f20d78f9a99498c37`
- 运行时主目录：`/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/home`
- 更新运行时主目录：`/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/update-home`
- 原始基准日志：`/tmp/relay-knowledge-relay-teams-refresh-20260514-224214`
- 二进制：`target/release/relay-knowledge`
- Web 绑定地址：`127.0.0.1:8791`

相关记录：

- [优化研究](relay-teams-optimization-study-2026-05-14.md)
- [优化问题清单](relay-teams-optimization-issues-2026-05-14.md)

`relay-teams` 工作树在此次运行中保持干净。基于 Git 的索引
将引用解析为已提交的树对象。

## 主机和工具链

- OS: Linux 6.17.0-23-generic x86_64
- CPU：第12代英特尔酷睿 i7-1260P，16个逻辑CPU
- Rust: `rustc 1.95.0`, `cargo 1.95.0`
- Node: `v24.14.0`
- uv: `0.10.10`

基准测试前使用的构建门控：

```bash
cargo build --release
npm --prefix web install
npm --prefix web run build
```

Web 验证期间使用的浏览器门控：

```bash
uv run --extra dev python -m playwright install chromium
uv run --extra dev pytest tests/browser
```

结果：`1 passed in 2.04s`。

## 仓库范围

`repo scope preview relay-teams --ref HEAD` 选中：

- 文件数：1,653
- 字节数：22,063,153
- 不支持的文件：218
- 生成的或大型文件：0
- 预期降级的文件：218
- 语言：Python 1,430 个文件 / 19,910,475 字节；未知 218 个文件 /
  2,145,214 字节；JavaScript 3 个文件 / 4,737 字节；Bash 2 个文件 / 2,727 字节

最大选中文件：

| 路径 | 字节数 |
| --- | ---: |
| `tests/unit_tests/frontend/test_project_view_ui.py` | 288,044 |
| `tests/unit_tests/frontend/test_model_profiles_ui.py` | 224,391 |
| `tests/unit_tests/boards/test_todo_service.py` | 204,354 |
| `docs/core/api-design.md` | 200,851 |
| `tests/unit_tests/sessions/runs/test_run_service_recovery.py` | 199,826 |

## 索引基线

冷全索引：

- 命令：`repo index relay-teams --ref HEAD --format json`
- 墙钟时间：47.45秒
- 峰值 RSS：360,504 KiB
- SQLite 数据文件：438,771,712 字节
- 索引文件：1,653 个
- 符号数：28,125
- 引用数：187,993
- 代码块数：28,436
- SQLite 写入次数：445,951
- 降级文件：218 个

无操作 HEAD 重新索引：

- 命令：`repo index relay-teams --ref HEAD --format json`
- 墙钟时间：0.38 秒
- 峰值 RSS：14,512 KiB
- `changed_path_count=0`
- `skipped_unchanged_count=1653`
- Blob 读取次数：0
- 解析文件数：0
- SQLite 写入次数：0

增量更新，通过首先索引基础数据，在单独的运行时中进行测量
提交然后更新到 HEAD：

- 基础完整索引：60.77秒
- `repo update relay-teams --base 0a4e709... --head fa3c0dd...`: 7.56s
- 更改的路径：4
- Blob 读取次数：1
- 解析的文件：1
- SQLite 写入次数：104

## CLI 基线

所有时间均为单进程的墙钟时间采样，单位为毫秒，测量于冷启动后
除非另有说明，否则索引可用。

| 命令案例 | 退出 | 毫秒 |
| --- | ---: | ---: |
| `version` | 0 | 0 |
| `help --format json` | 0 | 0 |
| `status` | 0 | 129 |
| `health` | 0 | 134 |
| `graph inspect` | 0 | 134 |
| `ingest` | 0 | 145 |
| `query relay-teams --freshness wait-until-fresh` | 0 | 129 |
| `query relay-teams --freshness graph-only` | 0 | 131 |
| `query relay-teams benchmark` | 0 | 129 |
| `index refresh --kind bm25 --kind semantic --kind vector` | 0 | 131 |
| `index refresh --kind bm25` | 0 | 143 |
| `repo status` | 0 | 130 |
| `repo report --format json` | 0 | 400 |
| `repo report --format markdown` | 0 | 384 |
| `repo scope preview --ref HEAD` | 0 | 167 |
| `repo query --kind hybrid` | 0 | 156 |
| `repo query --kind symbol` | 0 | 141 |
| `repo query --kind definition` | 0 | 143 |
| `repo query --kind references` | 0 | 130 |
| `repo query --kind callers` | 0 | 138 |
| `repo query --kind callees` | 0 | 141 |
| `repo query --kind imports` | 0 | 144 |
| `repo impact base..HEAD` | 0 | 521 |
| `repo update main..HEAD after indexing HEAD` | 1 | 134 |
| `provider probe` | 0 | 6 |
| `worker status` | 0 | 134 |
| `worker run-once --kind extractor` | 0 | 132 |
| `worker run-once --kind ocr` | 0 | 127 |
| `worker run-once --kind vision` | 0 | 134 |
| `proposal list` | 0 | 89 |
| `proposal show` | 0 | 90 |
| `proposal reject` | 0 | 87 |
| `proposal accept` | 0 | 104 |
| `proposal supersede` | 0 | 84 |
| `audit query` | 0 | 131 |
| `service status` | 0 | 132 |
| `service doctor` | 0 | 131 |
| `service plan install` | 0 | 132 |
| `service plan uninstall` | 0 | 126 |
| `service definition write` | 0 | 126 |
| `service operator status` | 0 | 134 |
| `service operator pause` | 0 | 135 |
| `service operator resume` | 0 | 132 |

`repo update main..HEAD after indexing HEAD` 失败是有文档记录的
当前已索引的范围必须与增量基准匹配的前提条件
参考。上述单独的更新运行时测量有效的基底到头部路径。

## Web HTTP 基线

Web 服务已启动，命令如下：

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/home \
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs,src,frontend,relay-teams-benchmark \
target/release/relay-knowledge service run --web --mcp streamable-http
```

使用 `curl` 测量针对同源 HTTP 端点的情况：

| Web 场景 | HTTP | 毫秒 |
| --- | ---: | ---: |
| `GET /` | 200 | 0 |
| `GET /api/health` | 200 | 6 |
| `GET /api/project/status` | 200 | 0 |
| `GET /api/service/status` | 200 | 0 |
| `GET /mcp/metrics` | 200 | 7 |
| `retrieve.context` | 200 | 1 |
| `graph.ingest` | 200 | 10 |
| `graph.inspect` | 200 | 8 |
| `index.refresh` | 200 | 2 |
| `provider.embedding.probe` | 200 | 0 |
| `worker.status` | 200 | 1 |
| `worker.run-once` | 200 | 4 |
| `proposal.list` | 200 | 1 |
| `proposal.show` | 200 | 0 |
| `proposal.reject` | 200 | 2 |
| `proposal.accept` | 200 | 12 |
| `proposal.supersede` | 200 | 2 |
| `audit.query` | 200 | 0 |
| `code.repo.register` | 200 | 10 |
| `code.repo.status` | 200 | 0 |
| `code.repo.query` 混合查询 | 200 | 17 |
| `code.repo.query` 符号查询 | 200 | 5 |
| `code.repo.query` 定义查询 | 200 | 5 |
| `code.repo.query` 引用查询 | 200 | 5 |
| `code.repo.query` 调用方查询 | 200 | 4 |
| `code.repo.query` 被调用方查询 | 200 | 5 |
| `code.repo.query` 导入查询 | 200 | 9 |
| `code.repo.impact` | 200 | 269 |
| `code.repo.index` no-op | 200 | 162 |
| `code.repo.update` HEAD..HEAD | 200 | 37 |
| `code.repo.update` main..HEAD 在索引 HEAD 之后 | 400 | 4 |
| `service.doctor` | 200 | 2 |
| `service.run.streamable_http` | 200 | 2 |

浏览器集成：

```bash
uv run --extra dev pytest tests/browser
```

结果：`1 passed in 2.04s`。

Headless Chromium 对 `http://127.0.0.1:8791/` 的实时页面加载基准测试，
5 个使用 `wait_until="networkidle"` 的示例：

- 平均值：528.36ms
- 中位数：527.74ms
- 最小值：521.41ms
- 最大值：536.21ms
- 浏览器导航 `loadEventEnd`：9.6 毫秒到 16.2 毫秒
- 实时仪表盘显示了仓库代码总量，但未显示之前的数据
  代码图为空状态。

## 历史基线问题和本次复测状态

1. 使用不同 alias 重新注册同一个仓库根目录会使先前 alias 失效。

   后续修复状态：已解决。重复根目录注册现在会为同一仓库 ID 添加持久 alias，并保留先前 alias。

   Web 测试期间，使用 alias `relay-teams-web` 对 `/opt/workspace/relay-teams`
   执行 `code.repo.register` 会更新既有仓库行，随后使用 alias `relay-teams`
   调用 `code.repo.status` 返回 `code repository 'relay-teams' is not registered`。
   仓库 ID 和已索引总量会保留在新 alias 下。这会让期望 alias 稳定或可追加的用户感到意外。

2. 在索引 HEAD 后，`repo update --base main --head HEAD` 仍较脆弱。

   后续修复状态：当基础快照此前已索引时已解决。即使活动仓库状态已指向 HEAD，增量更新现在也会克隆持久化的匹配基础 scope。

   当当前已索引 scope 已经是 HEAD 时，CLI 返回退出码 1，Web 返回 HTTP 400。有效顺序是先在单独或当前 scope 中索引基础引用，再更新到 HEAD。该行为符合当前校验，但 Web 编排器和文档应明确此前置条件。

3. 健康检查仍将图代码计数与仓库代码总量分开。

   后续修复状态：已解决。服务级 `health` 和 `graph inspect` 现在会将仓库代码总量纳入图代码计数，同时仍通过 `repository_code_totals` 暴露仓库专用明细。

   `/api/health` 报告 `graph.code_file_count=0`，同时
   `repository_code_totals.indexed_file_count=1653`。实时 Web 仪表盘现在能正确显示仓库代码总量，因此这不再是仪表盘误报为空的问题，但 API consumer 仍必须使用 `repository_code_totals` 读取代码仓库数据。

## 相比上一基线已解决

- 重复全量索引现在使用无操作快速路径：0.38 秒，blob 读取、解析和 SQLite 写入均为 0。
- Web 无操作 `code.repo.index` 现在会在 162ms 内返回 HTTP 200，而不是 30 秒后超时。
- 顶层 CLI GraphRAG 查询现在接受多词位置参数输入。
- 默认 scope 不再包含大型 JSONL 数据集 dump 或 `uv.lock`；选中字节数从 32,888,900 降至 22,063,153。
- 实时 Web 仪表盘在仓库索引后不再显示代码图为空。
