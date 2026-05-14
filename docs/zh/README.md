# 文档目录

[中文](../zh/README.md) | [英文](../en/README.md)

本目录按文档用途归档，避免研究资料、能力规格和后续设计文档混放。

## 使用指南

- [使用指南总览](user-guide/README.md): 按章节拆分的安装、CLI、知识图谱、代码仓库、Web、MCP/Agent 和运维排障说明。
- [第 1 章 安装与运行时目录](user-guide/01-install-and-runtime.md)
- [第 2 章 CLI 基础](user-guide/02-cli-basics.md)
- [第 3 章 知识图谱工作流](user-guide/03-knowledge-graph-workflow.md)
- [第 4 章 代码仓库工作流](user-guide/04-code-repository-workflow.md)
- [第 5 章 Web 工作区](user-guide/05-web-workspace.md)
- [第 6 章 Agent 与常驻服务](user-guide/06-agent-and-service.md)
- [第 7 章 运维与排障](user-guide/07-operations-and-troubleshooting.md)
- [第 8 章 高级配置参考](user-guide/08-advanced-configuration.md)

## 功能文档

- [GraphRAG 功能文档](graphrag-capability-guide.md)：涵盖当前的证据摄取、混合检索、本地/外部语义/向量后端契约、schema 路径、时序/社区、多模态证据维护、代码图、索引恢复、Web 准备、MCP/ACP 接入及 freshness/truncation 行为说明。
- [混合检索 Context Pack 功能文档](hybrid-retrieval-context-pack.md)：涵盖当前 BM25、语义/向量、路径/时序/社区、RRF 融合、结构化图事实、多模态分组、context pack 响应字段、后端状态及 freshness/truncation 行为。
- [Semantic/Vector Provider 后端](semantic-vector-provider-backend.md)：远端 OpenAI 兼容 embedding 提供者配置、脱敏诊断、Web Providers 面板及降级行为。
- [代码仓库 Tree-sitter 检索功能文档](code-repository-tree-sitter-retrieval.md)：当前 CLI/API 实现、存储模型、`canonical_symbol_id`、边解析/置信度元数据、检索返回字段及测试覆盖。
- [文档刷新审计 2026-05-14](documentation-refresh-audit-2026-05-14.md): 本轮对 README、用户指南、功能文档、规格、研究材料、benchmark 和 verification 文档的状态盘点、已刷新项与剩余实现项。

## 研究材料

- [知识图谱技术研究总结](research/knowledge-graph-research.md): 面向 `relay-knowledge` 架构的技术研究总结。
- [arXiv 知识图谱论文深度洞察](research/arxiv-knowledge-graph-paper-insights.md): 以 arXiv 论文为主的论文归档与工程洞察。
- [代码仓库 Tree-sitter 检索研究材料](research/code-repository-tree-sitter-retrieval-research.md): tree-sitter、Git 增量、代码知识图谱和高性能检索的资料依据与工程取舍。
- [Agent 协议图检索接入研究](research/agent-protocol-graph-retrieval-research.md)：MCP 服务器与 Agent Client Protocol 适配器暴露常驻图检索能力的协议研究与权衡。
- [relay-knowledge 实现借鉴落地路线](research/relay-knowledge-implementation-reference.md): 结合 docs/PDF 研究材料和当前 Rust 实现的已落地基线、剩余差距和阶段性路线。
- [2026 行业能力快照与差距分析](research/industry-capability-snapshot-2026.md): GraphRAG、MCP、A2A、托管检索和图谱 agent 生态的当前信号，以及 relay-knowledge 的产品差距。

## 规格

- [代码知识图谱能力参考](specs/knowledge-graph-capability-reference.md): 代码知识图谱系统能力规格与参考分析。
- [GraphRAG 产品与实现路线规格](specs/graphrag-product-and-implementation-roadmap.md): relay-knowledge 的 GraphRAG 产品边界、当前实现基线、优化措施和分阶段路线。
- [存储层架构设计](specs/storage-layer-design.md): 高性能、可测试、可替换的图谱存储层设计。
- [安装部署与发布规格](specs/installation-and-release.md): GitHub Releases、crates.io、包管理器、服务安装、升级卸载和 release CI 的交付要求。
- [工程硬约束](specs/engineering-hard-constraints.md): 禁止浅函数、死代码和循环依赖，要求文档完整、文件不超过 1000 行、UT 覆盖率大于 90%，并规定 `env`、`paths`、`net`、HTTP 事件驱动、QoS、UT+集成测试分层与 Playwright Chromium 浏览器集成测试门禁。
- [基础运行时层规格](specs/foundational-runtime.md): `env`、`paths`、`net::http` 和 `net::qos` 的环境变量、路径默认值、网络预算、失败模式和测试策略。
- [先进架构与可观测性设计](specs/advanced-architecture-observability.md): 本地优先、日志、telemetry、Grafana 和模块解耦设计。
- [后台服务、静默更新与自愈设计](specs/background-service-and-self-healing.md): 安装后常驻进程、静默图谱/索引更新、资源治理、假死检测和自愈恢复设计。
- [统一 API 层与交互层架构](specs/unified-api-and-interface-architecture.md): CLI/Web 收口到统一 API、Web 同源操作执行 endpoint 和 `streaming-json` 输出协议。
- [Source Scope 与多模态摄取规格](specs/source-scope-and-multimodal-ingestion.md): Git 分支/rebase 快照隔离、检索 scope 和文档文字/图片多模态 evidence 设计。
- [代码仓库 Tree-sitter 检索规格](specs/code-repository-tree-sitter-retrieval.md): Git 代码仓库基于 tree-sitter 的结构化解析、全量/增量更新、高并发检索、代码图和影响分析设计。
- [开放 Agent Runtime 与混合检索架构](specs/open-agent-runtime-and-hybrid-retrieval-architecture.md)：支持外部 agent runtime 驱动 LLM 知识处理，同时保持核心不实现 runtime，并明确 BM25、语义、向量和图扩展的混合检索边界。
- [Semantic/Vector Provider Backend 规格](specs/semantic-vector-provider-backend.md)：语义/向量外部 embedding 提供者的配置、HTTP 边界、错误分类、Web 契约及测试要求。
- [常驻进程 Agent 图检索访问规格](specs/resident-agent-graph-retrieval-access.md)：描述常驻进程通过 MCP 服务器和 Agent Client Protocol 适配器向其他 agent 暴露图检索能力的接口、权限、QoS、审计及测试要求。
