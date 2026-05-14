# Semantic/Vector Provider Backend 规格

[中文](../../zh/03-architecture-specs/semantic-vector-provider-backend.md) | [英文](../../en/03-architecture-specs/semantic-vector-provider-backend.md)

> 版本：1.0
> 日期：2026-05-13

## 总结

Semantic 和 vector 读模型可以由本地确定性模型或远端 OpenAI 兼容 embedding provider 支撑。远端调用只允许出现在 provider 探测和刷新/维护边界中，绝不能出现在查询热路径。

## 契约

- `env` 负责 provider 环境变量解析和校验。
- `net::http` 负责出站 HTTP client 构造、proxy/TLS 策略和 timeout 配置。
- `retrieval::provider` 负责 provider 中立的 embedding 请求、响应校验、OpenAI 兼容 wire 映射和重试分类。
- `application` 负责运行时组装、探测编排，以及传递给索引刷新完成流程的游标模型元数据。
- `storage` 只持久化 provider 中立的读模型数据和游标元数据。

Provider 配置：

- `RELAY_KNOWLEDGE_LLM_PROVIDER`：`openai_compatible` 或 `echo`。
- `RELAY_KNOWLEDGE_EMBEDDING_BASE_URL`：HTTP(S) endpoint base（端点基础地址）。
- `RELAY_KNOWLEDGE_EMBEDDING_API_KEY`：secret bearer token（密钥 token）。
- `RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE`：正整数，默认 `32`。
- `RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS`：正整数，默认 `30000`。
- `RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY`：正整数，默认 `4`。

当 semantic 或 vector 后端任一设置为 `external` 时，必须提供 base URL、API key、模型和维度。

## Web 契约

`RuntimeStatus` 包含不含 secret 的 provider 诊断：

- semantic/vector 后端模式。
- provider 类型。
- 脱敏后的 base URL。
- API key 是否已配置。
- 文本/图像模型名称。
- embedding 维度、batch 大小、timeout 和最大并发数。

Web `Providers` 面板必须保持只读，并且不得在 DOM、暂存操作 payload、日志或浏览器请求中包含 API key 值。

## 失败模式

- 缺少必需远端配置会导致运行时配置失败。
- provider 响应无效是永久错误，并记录诊断错误。
- 408、429、5xx、timeout 和传输失败可重试。
- 400、401、403 和 404 是永久错误。
- semantic/vector 后端过期或失败时，不得阻止 BM25 或图检索返回上下文。

## 测试

- 单元测试覆盖 env 解析、运行时组装、URL 归一化、响应校验、重试分类和游标元数据。
- Web build 和浏览器测试覆盖 Providers 面板、readiness 展示，以及不含 secret 的操作预览。
- 必要门禁为 `cargo fmt`、`cargo clippy`、`cargo test`、`npm run build --prefix web` 和 Playwright 浏览器测试。
