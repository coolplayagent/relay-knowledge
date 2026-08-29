# relay-knowledge 模型配置 API

[中文](06-model-configuration-api.md) | 英文版尚未提供

> 文档版本: 1.0
> 编制日期: 2026-06-06
> 文档定位：第 23 章 [HTTP API 参考](../23-api-reference.md)的模型配置专题；本页沿用主章迁移前的小节编号，不单独占用章节号。

## 8. 模型配置 API

模型配置端点位于 `/api/configs/`，用于管理 model profile、fallback、catalog 与 provider connectivity：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/configs/model/profiles` | 列出 model profiles |
| PUT / DELETE | `/api/configs/model/profiles/{name}` | 保存或删除指定 profile |
| GET / PUT | `/api/configs/model-fallback` | 读取或保存 fallback 配置 |
| GET | `/api/configs/model/catalog` | 读取 catalog；查询参数 `refresh=true` 可要求刷新 |
| POST | `/api/configs/model/catalog:refresh` | 显式刷新 catalog |
| POST | `/api/configs/model:probe` | 探测 provider/model connectivity |
| POST | `/api/configs/model:discover` | 从 provider 发现可用 model |

---

导航：上一专题：[MCP Streamable HTTP API](05-mcp-streamable-http-api.md) | 返回：[23. HTTP API 参考](../23-api-reference.md) | 上级：[API 专题索引](README.md)
