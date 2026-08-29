# 验证与审计记录

[中文](README.md) | [English](../../en/06-verification/README.md)

本卷保存带日期的验证和审计记录。每篇只证明对应 revision 与环境中实际执行的范围，不自动认证后续变更。判断当前状态时，必须重新运行有效门禁，并记录精确命令、revision、环境、结果及所有跳过项。

当前优先入口为[文档与自迭代准备度验证记录 2026-08-18](13-documentation-self-iteration-readiness-2026-08-18.md)。
2026-06-05 文档审计继续作为历史快照保留。

## 记录目录

1. [relay-teams E2E 验证 2026-05-14](01-relay-teams-e2e-2026-05-14.md)
2. [文档内容审计 2026-05-14](02-documentation-content-audit-2026-05-14.md)
3. [relay-teams 代码图检索准确性测试 2026-05-15](03-code-graph-retrieval-accuracy-relay-teams-2026-05-15.md)
4. [Linux 代码图检索准确性测试 2026-05-15](04-code-graph-retrieval-accuracy-linux-2026-05-15.md)
5. [文档书架结构审计 2026-05-17](05-documentation-book-structure-audit-2026-05-17.md)
6. [文档内容刷新审计 2026-05-17](06-documentation-content-refresh-audit-2026-05-17.md)
7. [Grep 兜底文档刷新审计 2026-05-22](07-grep-fallback-documentation-refresh-2026-05-22.md)
8. [软件全域建模文档刷新审计 2026-05-28](08-software-global-modeling-documentation-refresh-2026-05-28.md)
9. [软件全域、CodeGraph 与 Search Everything 研究文档刷新审计 2026-05-31](09-software-global-codegraph-search-everything-research-2026-05-31.md)
10. [服务化部署、控制面与数据面分离文档刷新审计 2026-06-04](10-service-deployment-control-data-plane-2026-06-04.md)
11. [文档发版准备审计 2026-06-05](11-documentation-release-readiness-2026-06-05.md)
12. [图数据库、知识图谱与 CodeGraph 深度研究归档 2026-06-05](12-graph-database-codegraph-deep-research-archive-2026-06-05.md)
13. [文档与自迭代准备度验证记录 2026-08-18](13-documentation-self-iteration-readiness-2026-08-18.md)

## 证据规则

- 命令输出只有在覆盖相应要求时才能作为证据。
- UT、integration、browser、coverage、package 和 benchmark 是不同验证层，一层通过不能替代其他层。
- missing、skipped、timed out、stale 或 environment-dependent 必须显式报告。
- 历史记录保持原样；当前证据变化时新增带日期的记录，而不是改写旧结论。

---

导航：[中文文档总目录](../README.md) | 下一篇：[1. relay-teams E2E 验证](01-relay-teams-e2e-2026-05-14.md)
