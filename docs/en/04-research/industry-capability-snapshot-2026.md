# Industry Capability Snapshot 2026

[English](../../en/04-research/industry-capability-snapshot-2026.md) | [中文](../../zh/04-research/industry-capability-snapshot-2026.md)

This is the English documentation page for `research/industry-capability-snapshot-2026.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> 文档版本: 1.0
> 编制日期: 2026-05-13
> 范围: GraphRAG、agent protocol、托管检索、本地图谱检索和 relay-knowledge 易用性差距。

## Summary

2026 年的主流方向已经从单纯 vector RAG 转向可解释、可治理、可互操作的知识系统:

- GraphRAG 从 local graph search 扩展到 global/community search、DRIFT 和 query routing。
- MCP 成为 agent 访问本地工具、资源和上下文的主流协议，Streamable HTTP 替代早期 HTTP+SSE。
- A2A 从早期规范进入生产化生态，用于跨框架 agent 协作和能力发现。
- 托管检索产品把复杂索引和执行细节隐藏在默认行为后，只向用户暴露少量结果数、过滤和引用相关开关。
- 图数据库生态把 full-text、vector、graph traversal、agent framework integration 和可解释路径组合成默认 GraphRAG 能力。

`relay-knowledge` 的当前架构方向基本正确: 本地优先、统一 API、三层检索、freshness、QoS、审计、MCP/ACP adapter 和 code graph 都已经存在。主要差距不是“是否有高级能力”，而是高级能力过早暴露给新手、真实 provider 和服务安装产品化不足、query routing/DRIFT 类查询策略还没有成为明确接口。

## Industry Signals

Microsoft GraphRAG 的查询引擎明确区分 local search、global search、DRIFT search、basic search 和 question generation。Local search 适合围绕具体实体的问题，global search 通过 community reports 做 map-reduce，DRIFT 把社区信息引入局部查询以扩大起点和事实多样性。Microsoft Research 的 DRIFT 实验还显示，它在 5K+ 新闻数据和 50 个局部问题上，相比 local search 在 comprehensiveness 和 diversity 上更常胜出。

MCP 2025-11-25 规范把 stdio 和 Streamable HTTP 定为标准 transport。Streamable HTTP 要求单一 MCP endpoint 支持 POST/GET，强调 Origin 校验、本地默认 localhost、认证、session id、protocol-version header、cancellation、resumability 和 back-compat。`relay-knowledge` 已经实现 Streamable HTTP、session header、协议版本校验、resources/prompts 和 DELETE session termination；GET/SSE resumability 仍是后续增强点。

A2A 官方规范把 Agent2Agent 定位为不同框架、语言和厂商 agent 之间的互操作标准。Linux Foundation 在 2026-04-09 宣布 A2A 已有 150+ 组织支持、主要云平台集成和生产使用案例；A2A v1.0 也已作为首个稳定生产版本发布。对 `relay-knowledge` 来说，A2A 更适合作为后续 specialist knowledge agent gateway，而不是把 core 改成 agent runtime。

OpenAI File Search 代表托管检索的易用性方向: 用户创建 vector store、上传文件，然后把 `file_search` 作为 Responses API tool 使用；检索执行由平台管理，并结合 semantic 与 keyword search。它也只把结果数这类少量控制项暴露给用户，降低 token/latency 与质量之间的取舍成本。

Neo4j GraphRAG 生态把 GraphRAG 描述为结合知识图谱、vector search、full-text search、graph traversal、社区/聚类、agent frameworks 和 MCP/A2A 集成的系统能力。它强调事实来源、路径、关系和可解释性，这与 `relay-knowledge` 的 context pack、graph paths 和 provenance 方向一致。

## relay-knowledge Current Fit

- 已匹配: local-first storage、SQLite read models、BM25、semantic/vector、graph path、temporal/community context、structured facts、freshness/versioning、QoS、MCP Streamable HTTP、ACP adapter、code repository graph。
- 部分匹配: global/community search 已有 community summary context item，但还没有 query router、lite-global 或 DRIFT-like 策略接口。
- 部分匹配: MCP tool surface 已可用，但 resources/prompts、resumability 和更完整 session lifecycle 尚未产品化。
- 部分匹配: A2A 已在架构文档中保留 gateway 方向，但没有 agent card、task lifecycle、artifact mapping 或 signed identity 计划细节。
- 缺口: 外部 embedding/OCR/vision provider、proposal lifecycle、service manager 安装、silent update operator、Web executable endpoints、release diagnostics 仍未落地。
- 易用性缺口: README 和用户指南过去把大量环境变量放在主路径，给新手造成“必须先理解所有配置”的错觉。

## Product Direction

- 默认零配置: 本地 deterministic read models 是默认路径，用户不需要先选择 embedding provider、HTTP 预算、QoS 或 MCP policy。
- 高级配置分层: Basic 只保留 CLI 参数；Advanced 承载 embedding、QoS、HTTP、MCP；Deployment 承载 service manager 和远程访问；Diagnostic 承载 CI/复现变量。
- 单一上手循环: `status -> ingest -> query -> health` 是最小闭环；code repo、Web、MCP 和 external backends 是后续章节。
- 接口预留: 后续新增 `setup doctor` 和 `setup profile`，先作为文档规格记录，不在本轮实现代码。
- 检索策略演进: 保持现有 context pack，不新增 core final-answer API；在其上规划 query router、lite-global 和 DRIFT-like expansion。

## Sources

- Microsoft GraphRAG Query Engine: https://microsoft.github.io/graphrag/query/overview/
- Microsoft Research DRIFT Search: https://www.microsoft.com/en-us/research/blog/introducing-drift-search-combining-global-and-local-search-methods-to-improve-quality-and-efficiency/
- MCP Streamable HTTP transport, 2025-11-25: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- A2A Protocol specification: https://a2a-protocol.org/dev/specification/
- A2A Protocol v1.0 announcement: https://a2a-protocol.org/latest/announcing-1.0/
- Linux Foundation A2A adoption update, 2026-04-09: https://www.linuxfoundation.org/press/a2a-protocol-surpasses-150-organizations-lands-in-major-cloud-platforms-and-sees-enterprise-production-use-in-first-year
- OpenAI File Search guide: https://developers.openai.com/api/docs/guides/tools-file-search
- Neo4j GraphRAG Labs: https://neo4j.com/labs/genai-ecosystem/graphrag/
