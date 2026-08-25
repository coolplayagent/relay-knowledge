# HTTP API 端点速查表

[中文](api-http-endpoints.md) | 英文版尚未提供

本页汇总[HTTP API 参考](../23-api-reference.md)中的方法、路径和用途。它仅用于快速导航；共享错误、响应 envelope 和运行时约定以主章为准，详细请求、响应与端点边界见以下专题：

- [控制面与 Web 操作 API](api-control-web.md)
- [代码仓库 API](api-code-repositories.md)与[代码库视图 API](api-codebase-views.md)
- [MCP Streamable HTTP API](api-mcp-streamable-http.md)
- [模型配置 API](api-model-configuration.md)

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/project/status` | 项目状态和运行时 |
| GET | `/api/health` | 全面健康检查 |
| GET | `/api/service/status` | 完整服务状态 |
| GET | `/api/v1/control/status` | 运行时诊断（只读） |
| GET | `/api/v1/control/health` | 健康检查（只读） |
| GET | `/api/v1/control/service/status` | 服务状态（只读） |
| GET | `/api/v1/control/storage/topology` | 存储拓扑 |
| GET | `/api/web/graph/canvas` | 图可视化画布 |
| POST | `/api/web/operations/execute` | 统一操作执行 |
| GET | `/api/v1/code/repositories` | 列出已有完成索引 scope 的仓库 |
| POST | `/api/v1/code/repositories/{alias}/index` | 仓库全量索引 |
| POST | `/api/v1/code/repositories/{alias}/update` | 持久化 Git commit 增量索引 |
| POST | `/api/v1/code/repositories/{alias}/scope/preview` | 索引范围预览 |
| POST | `/api/v1/code/repositories/{alias}/query` | 仓库代码查询 |
| POST | `/api/v1/code/repositories/{alias}/graph` | 有界 OKF repository graph neighborhood |
| POST | `/api/v1/code/repositories/{alias}/context` | codegraph context pack |
| POST | `/api/v1/code/repositories/{alias}/feature-flags` | 特性标志查询 |
| POST | `/api/v1/code/repositories/{alias}/impact` | 变更影响分析 |
| GET | `/api/v1/code/repositories/{alias}/report` | 仓库索引报告 |
| POST | `/api/v1/code/repositories/{alias}/software` | 软件全局投影 |
| POST | `/api/v1/code/repositories/{alias}/views` | 代码库理解派生视图 |
| GET | `/api/v1/code/repositories/{alias}/status` | 仓库索引状态 |
| GET | `/api/configs/model/profiles` | 模型 profile 列表 |
| PUT / DELETE | `/api/configs/model/profiles/{name}` | 保存或删除模型 profile |
| GET / PUT | `/api/configs/model-fallback` | 读取或保存 fallback 配置 |
| GET / POST | `/api/configs/model/catalog` / `/api/configs/model/catalog:refresh` | 读取或刷新模型 catalog |
| POST | `/api/configs/model:probe` / `/api/configs/model:discover` | 探测 provider 或发现模型 |
| POST | `/mcp` | MCP JSON-RPC |
| DELETE | `/mcp` | MCP 会话终止 |
| GET | `/mcp/metrics` | MCP 指标 |

---

导航：上一页：[API 专题索引](README.md) | 下一专题：[控制面与 Web 操作 API](api-control-web.md) | 返回：[23. HTTP API 参考](../23-api-reference.md)
