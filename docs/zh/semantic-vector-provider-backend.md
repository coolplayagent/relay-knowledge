# Semantic/Vector Provider 后端

[中文](../zh/semantic-vector-provider-backend.md) | [英文](../en/semantic-vector-provider-backend.md)

`relay-knowledge` 支持三种 semantic/vector 读模型模式：

- `local`：进程内确定性 token 读模型和哈希向量读模型。
- `external`：远端 embedding provider 元数据，以及面向 OpenAI 兼容 embedding API 的 provider 探测支持。
- `disabled`：跳过该读模型族，并报告回退状态。

查询路径从不调用远端模型。远端 provider 工作只属于索引刷新、启动恢复、维护 worker 和显式探测。semantic/vector 后端被禁用或降级时，BM25、图证据、图路径、时序和社区检索仍可使用。

## 配置

所有 provider 设置都通过 `env` 边界读取：

```bash
relay-knowledge setup profile external-embedding --format json
```

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external
RELAY_KNOWLEDGE_VECTOR_BACKEND=external
RELAY_KNOWLEDGE_LLM_PROVIDER=openai_compatible
RELAY_KNOWLEDGE_EMBEDDING_BASE_URL=https://api.example.com/v1
RELAY_KNOWLEDGE_EMBEDDING_API_KEY=...
RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embedding-3-small
RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32
RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536
RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE=32
RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS=30000
RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY=4
```

`external` 模式要求提供 base URL、API key、模型名称和维度。状态与健康检查响应只暴露脱敏后的 endpoint、provider 名称、模型元数据、预算，以及是否已配置 key。

## 运行时行为

OpenAI 兼容 provider 使用 `POST /v1/embeddings`：

```json
{
  "model": "text-embedding-3-small",
  "input": ["..."]
}
```

响应必须为每个输入返回且只返回一个 embedding，维度必须与配置一致，数值必须是有限浮点数。HTTP 408、429、5xx、超时和传输错误可重试。HTTP 400、401、403 和 404 视为永久配置或授权失败。

索引游标会持久化模型名称、维度、来源哈希和后端游标，使健康诊断能够按索引族、scope 和模态解释过期、降级或失败状态。

## Web UI 用户界面

Web 诊断工作区包含 `Providers` 区域，展示：

- semantic 和 vector 后端模式。
- 模型和维度元数据。
- 脱敏后的远端 endpoint 和 key 配置状态。
- batch、timeout 和 concurrency 预算。
- 带有模型、维度、scope 和后端游标的 semantic/vector 游标行。

Web UI 不保存 provider 设置，也不提交 API key。在引入 secret store 前，provider 配置仍属于安装和运行时关注点。
