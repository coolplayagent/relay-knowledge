# 第 5 章 Web 工作区

## 5.1 构建静态资源

Web 工作区位于 `web/`，用于诊断面板和操作预览:

```bash
./build.sh
```

构建产物位于 `web/dist`。浏览器集成测试会先构建静态资源，再启动测试用静态目录服务。

## 5.2 当前可读接口

当前 Web client 从同源服务读取:

```text
/api/project/status
/api/health
/api/service/status
/api/web/operations/execute
```

页面展示 project health、GraphRAG readiness、provider backend diagnostics、graph counts、scoped index freshness、refresh queue diagnostics、stale reasons、runtime budgets 和操作 composer。GraphRAG readiness 的 Stale reasons 项会显示第一条失败或滞后原因；完整列表仍以 `/api/health` 的 `index_refresh.stale_reasons` JSON 为准。
Providers 面板只展示脱敏后的 semantic/vector backend mode、模型、维度、endpoint host、key configured 状态和 cursor metadata；Web UI 不保存或提交 provider API key。

`relay-knowledge service run --web` 会在配置的 `RELAY_KNOWLEDGE_HTTP_BIND` 上挂载静态 Web workspace 和这些 Web endpoints；同时启用 `--mcp streamable-http` 时，MCP endpoint 与 Web endpoints 共用同一事件驱动 HTTP listener 和 QoS budget。非 loopback bind 必须显式启用 remote-client access policy；`/api/web/operations/execute` 的请求体受 `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` 限制。该 endpoint 接收当前 composer snapshot，返回执行后的 metadata、operation、command 和 result JSON。Rust Web adapter 只负责 HTTP JSON 解析和错误映射，实际 retrieve、ingest、graph inspect、index refresh、code repository workflow 和 service status 都复用 application service。

## 5.3 操作执行

Web Operations 面板覆盖这些工作流的 typed command/request preview 和同源执行:

- retrieve context
- ingest evidence
- graph inspect
- code repository register/index/query/update/impact/status
- index refresh
- provider probe
- worker status/run-once
- proposal list/show/accept/reject/supersede
- audit query
- service status 和 service run snapshot

`Run` 会发送当前 snapshot 并在页面内显示 pending、success 或 error 状态；执行请求不使用诊断请求的 10 秒客户端超时，避免长时间索引或维护操作被前端提前 abort。成功后页面会重新读取 `/api/project/status`、`/api/health` 和 `/api/service/status`，使 graph version、index freshness、queue diagnostics 和 stale reasons 跟随最新状态刷新；如果旧操作的刷新晚于新操作完成，旧刷新不会覆盖当前运行结果。`Stage` 仍保留最近 6 个操作 snapshot，适合保留待执行命令或对比 payload。`service run` 在 Web 中只返回 service runtime snapshot，不从浏览器启动或托管常驻进程。

## 5.4 同端口本地服务

本地启动 Web/API/MCP 服务:

```bash
./build.sh
./run.sh start --port 8791 --daemon
```

访问:

```text
http://127.0.0.1:8791/
http://127.0.0.1:8791/api/health
```

`run.sh` 不会自动构建。缺少 `target/release/relay-knowledge` 或 `web/dist/index.html` 时，先运行 `./build.sh`。

## 5.5 浏览器集成测试

本地验证:

```bash
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

测试覆盖 diagnostics、GraphRAG readiness、operation composer、index table、runtime panel 和移动端布局。
