# 文档目录

本目录按文档用途归档，避免研究资料、能力规格和后续设计文档混放。

## Research

- [知识图谱技术研究总结](research/knowledge-graph-research.md): 面向 `relay-knowledge` 架构的技术研究总结。
- [arXiv 知识图谱论文深度洞察](research/arxiv-knowledge-graph-paper-insights.md): 以 arXiv 论文为主的论文归档与工程洞察。
- [代码仓库 Tree-sitter 检索研究材料](research/code-repository-tree-sitter-retrieval-research.md): tree-sitter、Git 增量、代码知识图谱和高性能检索的资料依据与工程取舍。

## Specs

- [代码知识图谱能力参考](specs/knowledge-graph-capability-reference.md): 代码知识图谱系统能力规格与参考分析。
- [存储层架构设计](specs/storage-layer-design.md): 高性能、可测试、可替换的图谱存储层设计。
- [安装部署与发布规格](specs/installation-and-release.md): GitHub Releases、crates.io、包管理器、服务安装、升级卸载和 release CI 的交付要求。
- [工程硬约束](specs/engineering-hard-constraints.md): 禁止浅函数、死代码和循环依赖，要求文档完整、文件不超过 1000 行、UT 覆盖率大于 90%，并规定 `env`、`paths`、`net`、HTTP 事件驱动、QoS、UT+集成测试分层与 Playwright Chromium 浏览器集成测试门禁。
- [基础运行时层规格](specs/foundational-runtime.md): `env`、`paths`、`net::http` 和 `net::qos` 的环境变量、路径默认值、网络预算、失败模式和测试策略。
- [先进架构与可观测性设计](specs/advanced-architecture-observability.md): 本地优先、日志、telemetry、Grafana 和模块解耦设计。
- [后台服务、静默更新与自愈设计](specs/background-service-and-self-healing.md): 安装后常驻进程、静默图谱/索引更新、资源治理、假死检测和自愈恢复设计。
- [统一 API 层与交互层架构](specs/unified-api-and-interface-architecture.md): CLI/Web 收口到统一 API、React/Vite Web 交互层和 `streaming-json` 输出协议。
- [Source Scope 与多模态摄取规格](specs/source-scope-and-multimodal-ingestion.md): Git 分支/rebase 快照隔离、检索 scope 和文档文字/图片多模态 evidence 设计。
- [代码仓库 Tree-sitter 检索规格](specs/code-repository-tree-sitter-retrieval.md): Git 代码仓库基于 tree-sitter 的结构化解析、全量/增量更新、高并发检索、代码图和影响分析设计。
- [开放 Agent Runtime 与混合检索架构](specs/open-agent-runtime-and-hybrid-retrieval-architecture.md): 支持外部 agent runtime 驱动 LLM 知识处理，同时保持 core 不实现 runtime，并明确 BM25、semantic、vector 和 graph expansion 的混合检索边界。
