# 2026 行业能力快照与差距分析

[中文](../../zh/04-research/industry-capability-snapshot-2026.md) | [英文](../../en/04-research/industry-capability-snapshot-2026.md)

> 文档版本: 1.0
> 编制日期: 2026-05-13
> 范围: GraphRAG、agent protocol、托管检索、本地图谱检索和 relay-knowledge 易用性差距。

## 总结

2026 年的主流方向已经从单纯 vector RAG 转向可解释、可治理、可互操作的知识系统:

- GraphRAG 从本地图搜索扩展到全局/社区搜索、DRIFT 和查询路由。
- MCP 成为 agent 访问本地工具、资源和上下文的主流协议，Streamable HTTP 替代早期 HTTP+SSE。
- A2A 从早期规范进入生产化生态，用于跨框架 agent 协作和能力发现。
- 托管检索产品把复杂索引和执行细节隐藏在默认行为后，只向用户暴露少量结果数、过滤和引用相关开关。
- 图数据库生态将全文检索、向量检索、图遍历、代理框架集成和可解释路径组合成默认的 GraphRAG 能力。

`relay-knowledge` 当前的架构方向基本正确：本地优先、统一 API、三层检索、时效性、服务质量、审计、MCP/ACP 适配器和代码图均已存在。主要差距不在于“是否具备高级能力”，而是高级能力过早暴露给新手、真实提供者和服务安装的产品化不足、查询路由/DRIFT 类查询策略尚未形成明确接口。

## 行业信号

Microsoft GraphRAG 的查询引擎明确区分本地搜索、全局搜索、DRIFT 搜索、基础搜索和问题生成。本地搜索适合围绕具体实体的问题，全局搜索通过社区报告做 map-reduce，DRIFT 将社区信息引入局部查询以扩大起点和事实多样性。微软研究院的 DRIFT 实验还显示，在 5K+ 新闻数据和 50 个局部问题上，相较于本地搜索，DRIFT 在全面性和多样性方面更常胜出。

MCP 2025-11-25 规范将 stdio 和可流式 HTTP 定为标准传输方式。可流式 HTTP 要求单一 MCP 端点支持 POST/GET，强调 Origin 校验、本地默认 localhost、认证、会话 ID、协议版本头、取消、可恢复性和向后兼容。`relay-knowledge` 已实现可流式 HTTP、会话头和协议版本校验，但 resources/prompts、GET/SSE 可恢复性、DELETE 会话终止和旧 HTTP+SSE 兼容仍是后续增强点。

A2A 官方规范将 Agent2Agent 定位为不同框架、语言和厂商代理间的互操作标准。Linux Foundation 于 2026-04-09 宣布 A2A 已获得 150+ 组织支持、主要云平台集成和生产使用案例；A2A v1.0 也已作为首个稳定生产版本发布。对 `relay-knowledge` 来说，A2A 更适合作为后续专业知识代理网关，而非将核心改为代理运行时。

OpenAI 文件搜索代表托管检索的易用性方向：用户创建向量存储、上传文件，然后将 `file_search` 作为 Responses API 工具使用；检索执行由平台管理，结合语义和关键词搜索。它也仅向用户暴露结果数等少量控制项，降低了 token/延迟与质量之间的权衡成本。

Neo4j GraphRAG 生态将 GraphRAG 描述为结合知识图谱、向量搜索、全文搜索、图遍历、社区/聚类、代理框架和 MCP/A2A 集成的系统能力。它强调事实来源、路径、关系和可解释性，这与 `relay-knowledge` 的上下文包、图路径和来源方向一致。

## relay-knowledge 当前匹配情况

- 已匹配：local-first 存储、SQLite 读取模型、BM25、语义/向量、图路径、时间/社区上下文、结构化事实、新鲜度/版本控制、QoS、MCP 可流式 HTTP、ACP 适配器、代码仓库图。
- 部分匹配：global/community 搜索已有社区摘要上下文项，但尚无查询路由器、轻量级全局或类似 DRIFT 的策略接口。
- 部分匹配：MCP 工具界面已可用，但资源/提示、可恢复性和更完整的会话生命周期尚未产品化。
- 部分匹配：A2A 在架构文档中保留了网关方向，但无代理卡、任务生命周期、工件映射或签名身份的计划细节。
- 缺口：外部嵌入/OCR/视觉提供者、提案生命周期、服务管理器安装、静默更新操作员、Web 可执行端点、发布诊断仍未落地。
- 易用性缺口: README 和用户指南过去把大量环境变量放在主路径，给新手造成“必须先理解所有配置”的错觉。

## 产品方向

- 默认零配置：本地确定性读取模型是默认路径，用户无需预先选择嵌入提供者、HTTP 预算、QoS 或 MCP 策略。
- 高级配置分层：Basic 仅保留 CLI 参数；Advanced 承载嵌入、QoS、HTTP、MCP；Deployment 承载服务管理器和远程访问；Diagnostic 承载 CI/复现变量。
- 单一上手循环：`status -> ingest -> query -> health` 是最小闭环；代码仓库、Web、MCP 和外部后端将在后续章节介绍。
- 接口预留: 后续新增 `setup doctor` 和 `setup profile`，先作为文档规格记录，不在本轮实现代码。
- 检索策略演进：保持现有上下文包，不新增核心最终答案 API；在此基础上规划查询路由器、轻量全球和类似 DRIFT 的扩展。

## 来源

- Microsoft GraphRAG 查询引擎：https://microsoft.github.io/graphrag/query/overview/
- Microsoft Research DRIFT 搜索：https://www.microsoft.com/en-us/research/blog/introducing-drift-search-combining-global-and-local-search-methods-to-improve-quality-and-efficiency/
- MCP 可流式 HTTP 传输，2025-11-25：https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- A2A 协议规范：https://a2a-protocol.org/dev/specification/
- A2A Protocol v1.0 announcement: https://a2a-protocol.org/latest/announcing-1.0/
- Linux Foundation A2A adoption update, 2026-04-09: https://www.linuxfoundation.org/press/a2a-protocol-surpasses-150-organizations-lands-in-major-cloud-platforms-and-sees-enterprise-production-use-in-first-year
- OpenAI File Search guide: https://developers.openai.com/api/docs/guides/tools-file-search
- Neo4j GraphRAG Labs: https://neo4j.com/labs/genai-ecosystem/graphrag/
