# 第 6 章 Web 工作区

[中文](../../zh/01-user-guide/06-web-workspace.md) | [English](../../en/01-user-guide/06-web-workspace.md)

Web 工作区面向本地诊断和操作执行。它不是独立业务层；页面上的命令预览和执行结果都来自同一个后端 application service。

## 6.1 构建静态资源

Web 工作区位于 `web/`:

```bash
./build.sh
```

构建产物位于 `web/dist`。浏览器集成测试会先构建静态资源，再启动测试用静态目录服务。

## 6.2 同源接口

当前 Web client 从同源服务读取和执行:

```text
/api/project/status
/api/health
/api/service/status
/api/web/graph/canvas
/api/web/operations/execute
/api/configs/model/profiles
/api/configs/model-fallback
/api/configs/model/catalog
/api/configs/model:probe
/api/configs/model:discover
```

页面展示 project health、GraphRAG readiness、provider backend diagnostics、graph counts、Status graph overview、Graph canvas、scoped index freshness、refresh queue diagnostics、stale reasons、runtime budgets、agent/model settings 和操作 composer。完整诊断仍以 `/api/health`、`/api/service/status` 和操作返回 JSON 为准。

Providers 面板只展示脱敏后的 semantic/vector backend mode、模型、维度、endpoint host、key configured 状态和 cursor metadata；Web UI 不保存或回显 provider API key 原文。

## 6.3 页面结构

Web 工作区使用左侧导航和右侧详情区。桌面端左侧导航固定在视口内，右侧详情区独立滚动；窄屏端导航固定为顶部横向菜单。点击 Status、Readiness、Graph、Providers、Operations、Indexes、Runtime 或 Settings 时，详情区只显示对应页面，不把所有面板堆在同一个长页面内。

工具栏提供白天/夜间主题切换。首次打开时跟随浏览器系统主题；用户切换后，选择会写入浏览器本地存储并在后续刷新中保留。

Graph 页面提供三个只读画布:

- Knowledge: 展示 entity、evidence、relation、claim 和 event 的事实关系。
- Code: 展示 source scope、code file、code symbol 和 reference/call/import/define 关系。
- Mixed: 合并知识图和代码图，并显示 source scope 或 source path 可推导出的跨图关联。

画布请求使用 `/api/web/graph/canvas?kind=knowledge|code|mixed&scope=<scope>&query=<text>&limit=<n>`。默认 limit 是 250，最大 1000；后端始终按当前 graph version 返回 bounded snapshot，超过限制时在 `summary.truncated` 中标记截断。

## 6.4 操作执行

Web Operations 面板覆盖这些 typed command/request preview 和同源执行:

- 检索上下文和摄取 evidence。
- 图检查和索引刷新。
- 代码仓库注册、索引、查询、更新、影响分析和状态。
- provider 探测。
- worker 状态和 run-once。
- proposal 列表、展示、接受、拒绝和取代。
- 审计查询。
- service status 和 service run snapshot。

`Run` 会发送当前 snapshot 并在页面内显示 pending、success 或 error 状态。执行请求不使用诊断请求的 10 秒客户端超时，避免长时间索引或维护操作被前端提前 abort。

操作面板展示的 command 是 CLI 等价预览，不是前端模拟结果。真正执行结果来自 `/api/web/operations/execute` 返回的统一 API 响应。发生错误时，先复制 result JSON 中的 operation、command、error kind 和 metadata，再回到 CLI 用同样参数复现。

操作 payload 校验遵循共享 API 契约。例如 `code.repo_set.add` 在缺少 `priority` 时默认使用 `0`，但只要传入了格式错误或越界的 `priority`，就会返回 bad request，不会静默改成默认值并影响成员排序。

## 6.5 同端口本地服务

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

## 6.6 浏览器集成测试

本地验证:

```bash
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

测试覆盖 diagnostics、Status 首页查询入口、Status graph overview、单详情页导航、主题切换、GraphRAG readiness、graph canvas controls、operation composer、index table、runtime panel、Settings 配置生成、模型 provider profile、provider probe/discovery，以及移动端布局。

## 6.7 安全边界

Web 工作区面向本地诊断和操作，不承担安装器或后台 daemon 管理职责。浏览器中的 service run 只返回 runtime snapshot；真正常驻服务由 CLI、`run.sh` 或平台 service manager 启动。

远程访问默认关闭，只有在 MCP remote-client policy 和 HTTP bind 明确允许后才接受非本机客户端。`/api/web/operations/execute` 的请求体受 `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` 限制，实际 retrieve、ingest、index、repo、worker、proposal、audit 和 service 操作都复用后端 application service。
