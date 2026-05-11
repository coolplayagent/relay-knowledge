# 文档目录

本目录按文档用途归档，避免研究资料、能力规格和后续设计文档混放。

## Research

- [知识图谱技术研究总结](research/knowledge-graph-research.md): 面向 `relay-knowledge` 架构的技术研究总结。
- [arXiv 知识图谱论文深度洞察](research/arxiv-knowledge-graph-paper-insights.md): 以 arXiv 论文为主的论文归档与工程洞察。

## Specs

- [Code-Review-Graph 能力规格文档](specs/code-review-graph-capabilities.md): 代码知识图谱系统能力规格与参考分析。
- [存储层架构设计](specs/storage-layer-design.md): 高性能、可测试、可替换的图谱存储层设计。
- [先进架构与可观测性设计](specs/advanced-architecture-observability.md): 本地优先、日志、telemetry、Grafana 和模块解耦设计。
- [统一 API 层与交互层架构](specs/unified-api-and-interface-architecture.md): CLI/Web 收口到统一 API、React/Vite Web 交互层和 `streaming-json` 输出协议。
- [Source Scope 与多模态摄取规格](specs/source-scope-and-multimodal-ingestion.md): Git 分支/rebase 快照隔离、检索 scope 和文档文字/图片多模态 evidence 设计。
- [开放 Agent Runtime 与混合检索架构](specs/open-agent-runtime-and-hybrid-retrieval-architecture.md): 支持外部 agent runtime 驱动 LLM 知识处理，同时保持 core 不实现 runtime，并明确 BM25、semantic、vector 和 graph expansion 的混合检索边界。
