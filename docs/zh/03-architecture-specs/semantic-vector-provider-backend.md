# Semantic/Vector Provider Backend 规格

[中文](../../zh/03-architecture-specs/semantic-vector-provider-backend.md) | [英文](../../en/03-architecture-specs/semantic-vector-provider-backend.md)

> 版本：1.1
> 日期：2026-05-15

## 总结

Semantic 和 vector 读模型可以由本地确定性模型或远端 OpenAI 兼容 embedding provider 支撑。远端调用只允许出现在 provider 探测和刷新/维护边界中，绝不能出现在查询热路径。

## 契约

- `env` 负责 provider 环境变量解析和校验。
- `net::http` 负责出站 HTTP client 构造、proxy/TLS 策略和 timeout 配置。
- `retrieval::provider` 负责 provider 中立的 embedding 请求、响应校验、OpenAI 兼容 wire 映射和重试分类。
- `application` 负责运行时组装、探测编排，以及传递给索引刷新完成流程的游标模型元数据。
- `model_provider` 负责 Web Settings 中 chat/completion provider profile、fallback policy、公共 catalog cache、endpoint probe 和模型发现的持久化与校验。
- `storage` 只持久化 provider 中立的读模型数据和游标元数据。

Provider 配置：

- `RELAY_KNOWLEDGE_LLM_PROVIDER`：`openai_compatible` 或 `echo`。
- `RELAY_KNOWLEDGE_EMBEDDING_BASE_URL`：HTTP(S) endpoint base（端点基础地址）。
- `RELAY_KNOWLEDGE_EMBEDDING_API_KEY`：secret bearer token（密钥 token）。
- `RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE`：正整数，默认 `32`。
- `RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS`：正整数，默认 `30000`。
- `RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY`：正整数，默认 `4`。

当 semantic 或 vector 后端任一设置为 `external` 时，必须提供 base URL、API key、模型和维度。

模型 provider profile 配置通过 `paths` 边界解析文件位置：

- `model-profiles.json`：位于配置目录，保存命名 provider profile 和默认 profile。
- `model-fallback.json`：位于配置目录，保存 fallback policy。
- `model-catalog-cache.json`：位于缓存目录，保存 `models.dev` 公共 catalog cache。

Profile API key 和 secret header 只允许出现在保存请求与本地配置文件中。任何 Web/API 读取响应都必须返回脱敏视图，不得包含 secret 原文。

## Web 契约

`RuntimeStatus` 包含不含 secret 的 provider 诊断：

- semantic/vector 后端模式。
- provider 类型。
- 脱敏后的 base URL。
- API key 是否已配置。
- 文本/图像模型名称。
- embedding 维度、batch 大小、timeout 和最大并发数。

Web `Providers` 面板必须保持只读，并且不得在 DOM、暂存操作 payload、日志或浏览器请求中包含 API key 值。

Web `Settings` 的模型 provider 面板可以写入 profile/fallback 配置，并必须满足：

- 保存 profile 前校验 profile name、provider、base URL、sampling、timeout 和重复 header。
- 未配置 secret 的 MaaS、CodeAgent、Echo profile 可以保存；需要 API key 的 provider 在保存时必须提供 key 或已有 configured secret。
- `Probe` 和 `Discover` 必须通过 `net::http` 出站 client、timeout 和 QoS 预算执行，不得绕过网络边界。
- Catalog refresh 失败时保留内置 catalog fallback 或最近 cache，不得影响查询热路径。

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
