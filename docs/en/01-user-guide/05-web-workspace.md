# Chapter 5: Web Workspace

[English](../../en/01-user-guide/05-web-workspace.md) | [中文](../../zh/01-user-guide/05-web-workspace.md)

This is the English documentation page for `user-guide/05-web-workspace.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

## 5.1 构建静态资源

Web 工作区位于 `web/`，用于诊断面板和操作预览:

```bash
./build.sh
```

构建产物位于 `web/dist`。浏览器集成测试会先构建静态资源，再启动测试用静态目录服务。

## 5.2 同源接口

当前 Web client 从同源服务读取和执行:

```text
/api/project/status
/api/health
/api/service/status
/api/web/operations/execute
```

页面展示 project health、GraphRAG readiness、provider backend diagnostics、graph counts、scoped index freshness、refresh queue diagnostics、stale reasons、runtime budgets 和操作 composer。GraphRAG readiness 的 Stale reasons 项会显示第一条失败或滞后原因；完整列表仍以 `/api/health` 的 `index_refresh.stale_reasons` JSON 为准。
Providers 面板只展示脱敏后的 semantic/vector backend mode、模型、维度、endpoint host、key configured 状态和 cursor metadata；Web UI 不保存或提交 provider API key。

`relay-knowledge service run --web` 会在配置的 `RELAY_KNOWLEDGE_HTTP_BIND` 上挂载静态 Web workspace 和这些 Web endpoints；同时启用 `--mcp streamable-http` 时，MCP endpoint 与 Web endpoints 共用同一事件驱动 HTTP listener 和 QoS budget。非 loopback bind 必须显式启用 remote-client access policy；`/api/web/operations/execute` 的请求体受 `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` 限制。该 endpoint 接收当前 composer snapshot，返回执行后的 metadata、operation、command 和 result JSON。Rust Web adapter 只负责 HTTP JSON 解析和错误映射，实际 retrieve、ingest、graph inspect、index refresh、provider probe、code repository workflow、worker/proposal/audit operations 和 service status 都复用 application service。

## 5.3 布局与主题

Web 工作区使用左侧导航和右侧详情区。桌面端左侧导航固定在视口内，右侧详情区独立滚动；窄屏端导航固定为顶部横向菜单，详情区占满剩余空间。点击 Status、Readiness、Providers、Operations、Indexes 或 Runtime 时，详情区只显示对应页面，不把所有面板堆在同一个长页面内。

工具栏提供白天/夜间主题切换。首次打开时跟随浏览器系统主题；用户切换后，选择会写入浏览器本地存储并在后续刷新中保留。

## 5.4 操作执行

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

操作面板展示的 command 是 CLI 等价预览，不是前端模拟结果。真正执行结果来自 `/api/web/operations/execute` 返回的统一 API 响应。发生错误时，先复制 result JSON 中的 operation、command、error kind 和 metadata，再回到 CLI 用同样参数复现。

## 5.5 同端口本地服务

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

## 5.6 浏览器集成测试

本地验证:

```bash
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

测试覆盖 diagnostics、单详情页导航、主题切换、GraphRAG readiness、operation composer、index table、runtime panel 和移动端布局。

## 5.7 安全边界

Web 工作区面向本地诊断和操作，不承担安装器或后台 daemon 管理职责。浏览器中的 service run 只返回 runtime snapshot；真正常驻服务由 CLI、`run.sh` 或平台 service manager 启动。Provider 面板只展示脱敏配置，不接收或保存 API key。远程访问默认关闭，只有在 MCP remote-client policy 和 HTTP bind 明确允许后才接受非本机客户端。
