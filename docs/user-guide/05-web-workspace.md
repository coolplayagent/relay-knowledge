# 第 5 章 Web 工作区

## 5.1 构建静态资源

Web 工作区位于 `web/`，用于诊断面板和操作预览:

```bash
npm install --prefix web
npm run build --prefix web
```

构建产物位于 `web/dist`。浏览器集成测试会先构建静态资源，再启动测试用静态目录服务。

## 5.2 当前可读接口

当前 Web client 从同源服务读取:

```text
/api/project/status
/api/health
```

页面展示 project health、GraphRAG readiness、provider backend diagnostics、graph counts、scoped index freshness、refresh queue diagnostics、stale reasons、runtime budgets 和操作 composer。GraphRAG readiness 的 Stale reasons 项会显示第一条失败或滞后原因；完整列表仍以 `/api/health` 的 `index_refresh.stale_reasons` JSON 为准。
Providers 面板只展示脱敏后的 semantic/vector backend mode、模型、维度、endpoint host、key configured 状态和 cursor metadata；Web UI 不保存或提交 provider API key。

## 5.3 操作预览

Web Operations 面板覆盖这些工作流的 typed command/request preview:

- retrieve context
- ingest evidence
- graph inspect
- code repository register/index/query/update/impact/status
- index refresh
- provider probe 预览
- service status 和 service run

当前 composer 只生成和暂存命令或 payload 预览。执行型 Web endpoint 仍需要 Rust HTTP adapter 暴露后才能从页面直接发起写入、查询或索引操作。

## 5.4 浏览器集成测试

本地验证:

```bash
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

测试覆盖 diagnostics、GraphRAG readiness、operation composer、index table、runtime panel 和移动端布局。
