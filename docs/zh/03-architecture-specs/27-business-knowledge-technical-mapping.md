# 业务知识与技术图谱映射

[中文](../../zh/03-architecture-specs/27-business-knowledge-technical-mapping.md) | [English](../../en/03-architecture-specs/27-business-knowledge-technical-mapping.md)

本章规定 repository authored business knowledge 如何进入与代码、配置和软件模型共用的版本化图谱。它落实 Issue #361；typed、scoped ontology identity 是先决合同，不把 label 当作实体身份。

## 1. 权威源与数据流

`knowledge/knowledge-map.yaml` 只保存 `business-knowledge` topic、`repository-business-glossary` file source 和 route order。业务定义保存在受 Knowledge Map `glossary` 目录条目治理、版本控制的 `knowledge/glossary/business-glossary.yaml`：

```yaml
schema_version: 1
domains:
  - id: revenue
    name: Revenue
terms:
  - id: monthly-recurring-revenue
    domain: revenue
    canonical_name: Monthly Recurring Revenue
    definition: Recurring subscription revenue normalized to one month.
    language: en
    aliases:
      - value: MRR
        kind: abbreviation
    mappings:
      - relation: calculated_from
        target_kind: file
        target: src/billing.rs
```

固定数据流是 `map init → glossary authoring → repo index/update → fenced business projection → repo business/context`。查询不得扫描 YAML，不得启动 watcher writer，也不得维护第二套派生快照。

`map init` 幂等补齐 route 和最小有效 glossary。已有 glossary 必须先验证再保留；保留 ID 的 source、route 或文件类型、URI、scope 漂移必须报错。仅创建缺失 glossary 不增加 map version。

## 2. Schema、身份与上限

Ontology identity 是 `(repository source scope, domain_id, term_id, entity_kind)`。domain 与 term 使用 `business_domain`、`business_term` typed identity；名称变化不改变 ID。旧 label-only entity 保持 `untyped`，升级不重写旧 ID。

同名 term 可以存在于不同 domain。无 domain 的 exact canonical/alias 查询命中多个 domain 时返回 `ambiguous`，不能猜测。Route source order 只决定首选展示；多个 source 对同一 term 的不同 definition 全部保留并返回 conflict 与各自 evidence。

Schema v1 支持 synonym/abbreviation alias、非执行 formula/aggregation/unit/grain/time basis/includes/excludes semantics，以及 `represented_by`、`calculated_from` mapping。Target kind 包含 file、symbol、config key、API、software component、build target、IaC、design element、database table/column、metric 和 external。

硬上限为：单文件 4 MiB、256 domain、10,000 term、每 term 32 alias 和 64 mapping；ID 128 bytes；名称、alias、target 1,024 bytes；definition 和 formula 32 KiB。所有列表和字符串在进入存储前验证。

## 3. 投影、解析与 publication fence

Repository indexer 只从同一 immutable Git commit 读取当前 route 授权的 active repository-scoped file。v4 topic shard 必须通过 manifest digest 和 identity/order 校验；绝对路径、父目录逃逸、反斜杠路径、缺失 blob、超限内容或错误 schema 都使该 durable attempt 失败。非 Git live filesystem snapshot 不把工作区 glossary 冒充 committed business fact。

Business projection 与 code/software projection 使用同一个 durable task、lease、attempt 和 publication fence。存储边界通过独立的 `BusinessKnowledgeStore` contract 拥有业务读写，而不继续膨胀代码存储 contract。顺序为：code facts staged、business glossary loaded and staged、software projection staged、同一事务将 business/software status 和 code scope 发布为 fresh。旧 lease 或 target fence 不能执行 DELETE/INSERT；缺失或 stale business status 时 receipt 和 fast path 不能宣称 fresh。

Mapping resolution 只查同一 authorized source scope 的 indexed tables。file、symbol、config key、API、software component、build target、IaC 和 design element 可精确解析；当前没有对应技术 owner 的 database table/column、metric、external 保留 `resolution_state=unresolved` 与 `target_hint`。Unresolved external coverage 不设置 repository/parser degraded reason。

每个 accepted definition 和 mapping 都返回 source ID/path/digest、resolved commit、confidence、lifecycle、valid-from/until graph version。Scope retirement、repository removal 与 shard cleanup 必须清理 business tables；runtime backup/restore 必须把 control database 与全部 repository shard 当作整体。

## 4. 统一查询与 Context

共享 request 包含 repository selector、固定 ref、domain、query、`terms|mappings|all`、freshness 和 limit。统一入口是：

```bash
relay-knowledge repo business <alias> --kind all --query MRR --domain revenue --ref <commit> --freshness wait-until-fresh --format json
```

- HTTP：`POST /api/v1/code/repositories/{alias}/business`
- MCP：`relay_business_query`
- Web：只读 business term 与 technical mapping；编辑仍通过 glossary 文件和代码评审。

`repo context` 先在同一 pinned scope 解析 canonical/alias，再以 mapping resolved ID 或 target hint 作为有界 code seed。`business_context` 与 code results 共享 commit/source scope，并计入原有 candidate count、limit、byte budget、truncation 和 provenance。

`repo view --kind business-domains` 先合并 declared domain，再补充 route、feature flag 和 path inference。Evidence kind `business_glossary` 与 `route`、`feature_flag`、`path` 明确区分 authored 和 inferred 事实。

## 5. 升级、回滚与验收

首次打开旧 runtime database 会增加 typed entity identity columns 和 business projection tables。旧 code/software facts 与 label-only entity 不重写；旧 scope 因缺少 fresh business status 不走 full-index fast path，必须由正常 `repo index`/`repo update` 从 Git authoritative source 重建。Binary-only rollback 可以忽略新表，但不能读取新 projection；精确回滚需要升级前对 control database 和全部 shards 的事务一致备份。

验收覆盖 map init/upgrade/idempotency、path/digest/schema bounds、homonym/acronym/conflicting definition、fenced publication/replay/stale repair、resolved/unresolved mapping、canonical exact retrieval、business-to-code context、declared domain view，以及固定 commit 的端到端闭环。公式计算、OWL/RDF 推理、外部 Wiki/数据库抓取和 Web 编辑器不属于 v1。

---

导航：上一章：[26. Git Commit + Knowledge](26-git-commit-knowledge-development-loop.md) | 返回：[架构规格](README.md)
