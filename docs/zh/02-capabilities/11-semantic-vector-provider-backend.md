# Semantic/Vector Provider 后端

[中文](./11-semantic-vector-provider-backend.md) | [English](../../en/02-capabilities/11-semantic-vector-provider-backend.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

Semantic/vector provider 后端提供本地和外部模型之间的可控切换。默认 local 模式保证离线可用；external 模式记录模型元数据；disabled 模式明确退出对应 read model。

## 用户可见行为

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external RELAY_KNOWLEDGE_VECTOR_BACKEND=external RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embed-3-small RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32 RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536 relay-knowledge index refresh --kind semantic --kind vector --format json
```

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` 和 `RELAY_KNOWLEDGE_VECTOR_BACKEND` 接受 `local`、`external` 或 `disabled`。

## 竞争力特性

Provider 状态进入 `backend_statuses`，包含 configured backend、model、dimension、scope post-filter 和 indexed graph version。外部模型不是事实真源，只是派生读模型后端。

## Web 用户界面

Web Settings 页面展示模型 provider profile、fallback policy、endpoint probe 和模型发现状态。Secret 只在保存时接收，回传给浏览器时只显示 configured boolean 或脱敏 header。

## 降级与诊断

Disabled 模式不会运行 semantic/vector retriever，也不会调度对应 refresh。模型名只接受 trim 后非空值；维度变化应触发明确 rebuild/freshness 要求。

## 关联架构章节

- [Semantic/Vector Provider 架构](../03-architecture-specs/10-semantic-vector-provider-architecture.md)

---

导航: 上一章: [10. 代码影响分析与报告](10-code-impact-and-reporting.md) | 下一章: [12. Web 工作区能力](12-web-workspace-capabilities.md)
