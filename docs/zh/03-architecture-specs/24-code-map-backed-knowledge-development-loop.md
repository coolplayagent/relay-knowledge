# 代码地图驱动的 Knowledge 开发闭环

[中文](../../zh/03-architecture-specs/24-code-map-backed-knowledge-development-loop.md) | [English](../../en/03-architecture-specs/24-code-map-backed-knowledge-development-loop.md)

> 文档版本: 1.0
> 编制日期: 2026-08-12
> 需求来源: [issue #351](https://github.com/coolplayagent/relay-knowledge/issues/351)、[issue #352](https://github.com/coolplayagent/relay-knowledge/issues/352)

## 1. 结论与范围

本规格把两个 issue 收敛为一个可执行的 Knowledge 开发闭环：

1. repository bootstrap 必须同时建立 `codespec/codespec-map.yaml`、`knowledge/knowledge-map.yaml` 和版本化代码地图；只完成其中一个 surface 不能报告初始化成功。
2. Git commit 是已跟踪源码事实的权威源；代码地图是针对精确 commit/source scope 发布的符号、调用、依赖和检索证据视图。软件全域模型是与该代码地图同 scope、同发布边界的派生读模型。
3. YAML 保存稳定的知识路由和模型入口，不复制会随 commit 变化的架构摘要、构建 target 或部署事实。实际 `design`、`build`、`iac`、`relationships` 事实由 `repo software` 返回，并携带 ref、source scope、新鲜度和证据。
4. spec 编写和 agent 编码前必须消费同一个固定 ref 的 `business-knowledge` 路由、业务术语/映射、软件模型、architecture/business-domain 视图和代码上下文；commit 后必须刷新同一 fenced projection 并再次验证 YAML。

本规格不引入第二份代码事实、不把 LLM 叙述持久化为真源、不在查询热路径扫描仓库，也不以 shell polling loop 取代 durable task、lease 或平台服务。

本章回答“如何落地”：定义 CLI 协调、任务、lease、freshness 与验收契约。关于“为什么以 commit 为事实锚、团队和 agent 如何组织决策上下文”，见[第 26 章：Git Commit + Knowledge 开发迭代理念与 Loop](26-git-commit-knowledge-development-loop.md)。

## 2. 权威状态与所有权

| 状态面 | 权威内容 | 所有者 | 一致性身份 |
| --- | --- | --- | --- |
| Git repository | 源码、文档、manifest、CI、部署配置 | Git | immutable commit 或显式 worktree overlay |
| Repository maps | 强类型 CodeSpec/Knowledge 目录及 topic、source、route、有限 history 和软件模型入口 | `codespec/codespec-map.yaml`、`knowledge/knowledge-map.yaml`、`knowledge/topics/`、`knowledge/history/` | `schema_version`、`map_type`、`map_version`、SHA-256 digest |
| Code map | file、symbol、reference、call、import、chunk 和变更事实 | code repository index | repository id、resolved commit、tree hash、source scope |
| Software model | 兼容投影、稳定 ontology entity、snapshot occurrence、provenance statement、shape/conflict diagnostic | software global projection v6 | 与 code map 相同的 source scope 和 graph version；Knowledge Map 只保存稳定路由，不复制这些派生行 |
| Business model | domain、canonical term、alias、semantics、definition conflict 与技术映射 | Git-authored glossary 的 fenced projection | 与 code map 相同的 resolved commit、source scope 和 graph version |
| Agent context | map route、software/view/context/impact 的有界组合 | skill workflow | 固定 base/head、freshness、evidence id |

`knowledge/knowledge-map.yaml` 的默认稳定入口为：

- topic id: `software-model`
- source id: `repository-software-model`
- source kind: `repo`
- URI: `.`
- source scope: `repo`

该 source 表示“当前仓库的 code-map-backed 软件模型入口”，而不是一份生成结果缓存。`map init` 对新旧 map 都必须幂等确保该入口存在；如果保留 id 已被用于不兼容的 topic、kind、URI 或 scope，命令必须报告冲突，不能静默覆盖用户契约。

`map init` 同时确保 `business-knowledge` topic、`repository-business-glossary` file source、`knowledge/glossary/business-glossary.yaml` URI 和 `repo` scope。该 route 只负责授权；glossary 保存 authored 业务事实，索引后才成为绑定 commit、scope、freshness 和 evidence 的图事实。已有 glossary 必须保留，保留 route/source 冲突必须失败。

`map validate` 必须要求两个保留 source 具有上述精确字段，并要求每个 source 继续属于对应的保留 topic route。两个 topic 下仍可包含其他符合普通 source 契约的 source；这些 source 不得被误当成 business glossary 文档解析，visible v3 glossary source 必须使用 canonical URI，legacy glossary URI 只允许在读取 legacy contract 时归一化。source removal 也不得删除任一保留入口。`map source add` 首次创建 Knowledge Map 时，必须先在内存中完成整个 mutation 的领域校验，不能先创建 writer lock 或合同文件；校验成功后，在发布 root 前创建受治理的 baseline README 和缺失的最小 glossary。被拒绝的首次 add 不得留下可见、legacy 或 CodeSpec map artifact。每个受控 source add、update 或 remove 都必须通过 current-path lock 串行化 legacy-state 探测；若 legacy state 活跃，则按 legacy-then-current 顺序取得两把 repository lock，并先针对 live legacy snapshot 在内存中校验请求 mutation。每次 forward cycle 都必须用精确的 active legacy root 刷新已验证的 rollback backup；若存在已验证的 active legacy glossary，还必须同步刷新 canonical glossary；之后才可在新的可见 v3 publication 中补齐缺失的内置 route 并安装 legacy redirect。被拒绝的 legacy mutation 因而不得发布 migration state。因此 rollback 可以恢复一个结构有效但缺少当前必需 route 的旧 contract；此时当前 `map validate` 会报告缺口，直到显式 `map init` 再次升级。

Repository Map v3 为两个可见根文件增加强类型 `directories`。`codespec` 固定要求 `requirements`、`design`、`api`、`test`、`decisions`；`knowledge` 固定要求 `domain`、`guides`、`ops`、`glossary`、`best-practices`，同时允许扩展受限的自定义目录。条目包含 purpose、content scope、key files、load policy、强类型限定关系和 update policy。Knowledge Map 继续保存 topic 摘要、有序 source id、内容寻址 shard ref、map version 与最多 16 条 recent history；topic 位于 `knowledge/topics/`，更早历史位于 `knowledge/history/`。`map route <topic> --type knowledge` 只加载目标 shard；聚合读取默认处理两张 map，定向写入必须指定具体 type。跨文件 digest、目录存在性、关系目标与环、history 连续性、路径边界和保留 route 均由 `map validate` 校验，不能只依赖 JSON Schema。

所有生成 ref 必须限制在所选 map 根下的指定真实目录，拒绝绝对路径、`..`、symlink/reparse 逃逸和 map type/path 不匹配。多文件 mutation 先发布不可变 artifact，最后发布根文件。V2 迁移先复制保留资产，再发布 `knowledge/knowledge-map.yaml`；只有可见 v3 root 及其 artifact 已完成校验后，才把 `.knowledge/knowledge-map.yaml` 替换为旧 reader 明确拒绝的 v3 redirect。legacy topic 和 history 复制必须在加载内容前执行单文件、总字节数和文件数量预算。rollback 已准备 legacy root 但尚未发布时，reader 必须使用 retained v3 root 及其 contract directory。rollback 在移动普通 fallback-only root 到保留恢复状态前必须先将其提升；重复 rollback 会先协调已提交的转场，且 clean rollback 已活跃时会校验并保留 live legacy root，不会重新暂存较旧 backup。CLI 保留经过验证的 v2 根供 `map migrate --type knowledge --rollback` 恢复；卸载不得删除仓库拥有的 map 内容。

`map show`、`map route`、`map validate` 和 `map history` 是严格只读边界。legacy v1 reader 只会在校验前归一化历史 glossary URI，visible v3 校验仍坚持 canonical URI。早期 v2 root 已归档 history 但缺少 range index 时，`map validate` 必须给出运行显式 `map init` 的诊断，不得创建 index node 或改写 root；`map route` 必须在读取目标 shard 前校验 root 的 `map_type` 与可见路径一致。只有持有 writer lock 的显式初始化/迁移路径可以补齐 history index。

## 3. 为什么 YAML 不复制派生模型

把 resolved commit、架构摘要、build target 和部署资源直接回写进同一个被索引的 YAML，会形成自引用循环：YAML 变化会改变 Git tree，新的 tree 又会生成新的 snapshot identity，继而要求再次改写 YAML。它还会制造一份脱离 durable publication fence 的陈旧事实副本。

因此本规格采用以下边界：

- YAML 固化“去哪里读”和“哪个 repository 是模型根”。
- code map 固化“从这个 ref 派生并服务哪些源码事实视图”。
- software projection 固化“从同一个 source scope 能确定性派生什么架构、构建和部署事实”。
- 短 narrative 只能作为带 evidence id 的响应内容，不能成为持久化权威事实。

这使 YAML 可审查、可回滚，派生模型可刷新、可诊断，并避免双写漂移。

## 4. Bootstrap 协议

Skill 在初始化仓库知识时必须按以下顺序协调现有 CLI：

1. 解析已发布的 `relay-knowledge` 可执行文件并读取命令 metadata。
2. 执行 `map validate --format json`。仅当 map 缺失时创建；已存在但无效的 map 必须报告诊断，不能覆盖。
3. 执行 `map init --format json`，创建或幂等补齐默认 `software-model` 路由，再次执行 `map validate`。
4. 执行 `repo list --format json`，按规范化 root 和注册 scope 复用已完成 alias；没有匹配项时执行 `repo register`，并从响应捕获 alias。
5. 对 Git repository 先建立 clean `HEAD` 基线。若 map 是新建/升级或需要包含其他已授权的未提交文件，再建立 `worktree` overlay；非 Git source directory 继续使用 `HEAD` filesystem snapshot。
6. 把 `repo index` 当作 durable、bounded、single-writer task。命令超时后通过 `repo status` 恢复；已有 managed service 时不得启动竞争 worker；没有 service 且任务 queued/retrying 时只运行有界 single-shot `repo index-worker`。
7. 只有在 status 指向精确 resolved target、checkpoint 完成且 scope 不 stale 后，才读取同一 ref 的 `repo business --kind all`、`repo software --kind all`、`repo view --kind architecture-layers|business-domains` 与业务驱动 `repo context`。
8. 最后再次执行 `map validate`，并把 map version、resolved ref、source scope、freshness 和 degraded diagnostics 纳入初始化结果。

Bootstrap 不是跨 YAML 与 SQLite 的假原子事务。中途失败时保留可恢复的 map 文件、durable task、checkpoint 和诊断；下一次运行从状态恢复，不删除有效成果或启动无界重试。

## 5. 增量开发协议

### 5.1 Commit 事件

对已注册 Git repository，正常 commit 事件执行一次 `repo update <alias>`。服务必须在排队前解析并固定 base/head；agent 从完成响应的 `summary.base_resolved_commit_sha`、`summary.resolved_commit_sha`，或 queued task 的 immutable base/head 捕获同一对 commit。

随后必须：

1. 等待 `repo status` 报告精确 head 已发布且不 stale。
2. 在固定 base/head 上执行 `repo impact`。
3. 在固定 head 上执行 `repo business --kind all`、`repo context`、`repo software --kind all` 和 architecture/business-domain `repo view`。
4. 如果 Markdown、spec 或 knowledge map 发生变化，再读取 `repo software --kind topics|relationships` 和受影响 OKF neighborhood。
5. 执行 `map validate`。新增、移动或删除权威文档/config/CI/runtime source 时，只通过 `map source add/update/remove` 修改路由并保留 history。

Code index publication 已在同一个 task lease、attempt 与 publication fence 内刷新 business/software projection；business 未完成时 staged scope 不得发布，所以不允许增加第二个 writer、查询时 YAML/全仓扫描或未管理后台 loop。

### 5.2 Worktree 迭代

需要让 agent 在 commit 前消费未提交修改时，先确保 clean `HEAD` 基线存在，再执行 `repo index <alias> --ref worktree`。所有后续 query、software、view 和 context 命令也必须显式使用 `worktree`，不能把 clean commit 的结果描述成包含未提交内容。

Map mutation 会改变 worktree。若本轮 spec/编码决策必须立即看到新 route，则刷新 worktree overlay；否则把 map 与相关源码/文档一起提交，并由下一次 commit update 发布。两种路径都必须在交付前说明所服务的 ref。

## 6. Spec 与编码上下文契约

Agent 在写 spec 前至少读取：

- `map route business-knowledge` 与固定 ref 的 `repo business --kind all`；
- 相关 `map route`，包含 architecture、build、deployment 或 repository-specific topic；
- 固定 ref 的 `repo software --kind all`，重点检查 `design`、`build`、`iac`、`relationships`；
- `repo view --kind architecture-layers`；
- `repo view --kind business-domains`，并区分 authored 与 inferred evidence kind；
- 与需求相关的 `repo context` 或具体 definition/references/callers/callees 查询；
- freshness、unresolved edge、direct-source-read 和 degraded diagnostics。

Agent 在编码前必须把 spec requirement 映射到代码 symbol、调用/依赖边、配置、构建/部署证据和测试入口。缺少证据时应暴露 gap 或 unresolved target，不能用猜测、fixture 特判或任意 grep 结果填补。

Agent 在验收时必须给出“requirement → authoritative evidence → test/gate”的矩阵。窄 UT 只能证明其覆盖的局部行为；全仓质量、skill package 和 release surface 分别需要对应 gate。

## 7. 新鲜度、失败与降级语义

| 状态 | 允许行为 | 禁止声明 |
| --- | --- | --- |
| map missing | 创建并验证 map | repository knowledge 已初始化 |
| map invalid/conflicting | 报告诊断并停止 map mutation | 已自动修复或已同步 |
| task queued/running/retrying | 报告 task/checkpoint，按 managed service 或 bounded worker 恢复 | 精确 target 已可查询 |
| scope stale | 继续恢复或明确使用 allow-stale 诊断 | spec/code 基于最新图谱 |
| scope fresh, projection degraded | 读取未受影响证据并披露缺口；受影响路径需直接核对 | 无条件完整模型 |
| exact scope fresh, map valid | 生成带 provenance 的 spec/code context | 无证据的架构事实 |

缺失外部依赖源码仍是 unresolved metadata，不是 repository degradation。只有授权 scope 内的解析、持久化或投影失败才能形成 degraded reason。

## 8. 安全、资源与可恢复性

- 所有 index/update 都保留 bounded queue、lease、checkpoint、backoff、dead-letter 和单 repository active writer 约束。
- Skill 不杀死竞争进程、不提高无界 busy timeout、不删除 runtime state 来制造成功。
- `repo business`、`repo software`、`repo view`、`repo context` 只读取已提交投影/图事实，不在查询热路径读取 glossary 或递归扫描仓库。
- Map mutation 使用文件锁、原子 rename、连续 history version 和 CLI validation；不手工改写 YAML，除非 CLI 不可用且用户明确要求修复。
- 静默后台更新仍由平台 service manager 承载，必须可暂停、可观察、可恢复。

## 9. 验收矩阵

| ID | 要求 | 权威证据 |
| --- | --- | --- |
| KDL-01 | 新 map 初始化包含默认 software-model route | domain/application UT 读取 YAML 并验证 topic/source/route |
| KDL-02 | 旧 map 幂等补齐 route，重复 init 不增加版本 | application UT 比较首次升级和第二次 init 的 map version |
| KDL-03 | 保留 source id 冲突不被覆盖 | domain UT 断言明确冲突错误 |
| KDL-04 | Skill bootstrap 同时覆盖 map 与 code map | skill contract gate 校验有序的 validate/init/list/register/index/status/model/view/validate 工作流 |
| KDL-05 | 增量 loop 固定 base/head 并刷新模型 | skill contract gate 与现有 update/index integration tests |
| KDL-06 | 架构、构建、部署模型来自同一 code scope | 隔离 runtime 的 CLI 端到端测试串联 `map init`、register/index、`repo software all`、architecture view 与最终 validate，并断言 resolved commit、source scope、freshness 和 evidence；software projection tests 补充局部边界覆盖 |
| KDL-07 | Spec/编码入口消费 map、model、impact/context | skill 默认 prompt、reference workflow 和 package validation |
| KDL-08 | 文档与发布包不会回退到旧提示词 | shared skill metadata/policy self-test、PR gate、release bundle gate |
| KDL-09 | 仓库交付通过全量质量门禁 | fmt、clippy、all-target tests、coverage、package、publish dry-run 和相关 self-iteration cases |
| KDL-10 | 业务模型与代码/软件模型使用同一发布身份 | 端到端测试串联 business route、glossary authoring、index、business/view/context，并断言 commit、scope、freshness、evidence 与 publication fence 一致 |
| KDL-11 | validate 要求两个精确保留 route，同时允许普通同 topic source | application tests 分别移除两个保留 shard，并增加一个非 glossary 的 business source |
| KDL-12 | 首次 source add 发布完整 baseline，拒绝的请求零副作用 | application 与 CLI tests 校验 baseline README/glossary，并覆盖空 id 与保留 id 冲突 preflight |
| KDL-13 | 所有 source mutation 都保留内置 route 与 legacy rollback 边界 | application tests 覆盖保留 source 删除、legacy-only add/update/remove、validate 与原字节 rollback |

---

导航：上一章：[23. HTTP API 参考](23-api-reference.md) | 下一章：[25. 代码索引保留策略](25-code-index-retention.md) | 返回：[架构规格](README.md)
