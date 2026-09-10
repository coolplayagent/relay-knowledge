# 软件全域建模架构

[中文](../../zh/03-architecture-specs/21-software-global-domain-modeling.md) | [English](../../en/03-architecture-specs/21-software-global-domain-modeling.md)

> 文档版本: 1.4
> 编制日期: 2026-09-02
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

软件全域建模把源码图、依赖图、构建图、配置图、测试图、发布图和运行时诊断图纳入同一版本化事实空间。代码仓库是设计态和交付态的自描述证据边界，不是所有软件事实的封闭真相边界；授权的运行事件、部署观察、组织目录和外部依赖元数据可以进入模型，但必须保留来源、观察时间、授权 scope 和冲突状态。

实现继续以 SQLite 属性图和物化读模型作为运行时，不迁移到 RDF 三元组主存储。在运行时之上增加版本化 ontology contract、shape validator，以及 SPDX、CycloneDX 和 PROV-O 的导出映射；标准映射用于互操作，不能反向成为无证据事实的来源。

设计必须满足四个约束：

- 基础事实仍按真实 source scope 分区，不能为了全域视图复制或混合单仓代码事实。
- SDK、依赖、构建 target、生成器、配置、测试和发布 artifact 是一等实体，不是代码 chunk 属性。
- 所有变化传播必须经过 durable graph mutation 和 bounded refresh task，不能由查询热路径递归扫描仓库、包缓存或 SDK 目录。
- 缺失外部源码、未授权依赖和未安装 SDK 只能形成 unresolved edge metadata，不能写成 resolved graph facts。

## 2. 核心模型

Ontology contract 分成四层：

| 层 | 当前受控类型或责任 |
| --- | --- |
| 稳定实体 | `Domain`、`SoftwareSystem`、`Component`、`Api`、`Resource`、`Configuration`、`BuildDefinition`、`DeploymentUnit`、`RuntimeService`、`TestCase`、`ReleaseArtifact`、`PackageComponent`、`Sdk`、`DocumentationUnit`、`Pipeline`、`BuildJob` |
| 快照与事件实例 | `RepositorySnapshot`、`FileRevision`、`BuildRun`、`DeploymentRevision`、`RuntimeObservation` |
| 可追溯陈述 | 一等 `SoftwareStatement`，表达 subject、predicate、object、来源、证据、提取器、时间、置信度、解析和事实状态 |
| 派生读模型 | 兼容的 dependency、SDK、file、topic、relationship、build、IaC、design 投影及新的类型化查询切片；它们可重建，不是权威事实本身 |

Issue #362 的核心模块边界位于数据模型与存储实现之间：`domain::core::ontology` 定义有界、storage-independent 的 `OntologySchema`、class identity、RDF local name、OWL object property 及可执行 domain/range relation shape；`domain::operations::software::vocabulary` 登记并直接导出 `SOFTWARE_ONTOLOGY_SCHEMA` catalog，其中包含软件 ontology 的 21 个 class、15 个 object property、`1.0.0` 版本和 `https://relay-knowledge.dev/ontology/software/1#` namespace。Entity/statement constructor、shape validator、SQLite materialization 及 PROV-O JSON-LD export 消费同一 catalog，不再各自维护 namespace 或 domain/range match。

Core schema validation 会解析 namespace IRI，要求绝对 HTTP(S) scheme 和有效 host，再于投影发布前检查 schema/version identity、class/property 容量、RDF local name、identity 唯一性、relation shape 非空和所有 class reference。可执行 relation check 即使遇到 `Any` shape，也必须拒绝 catalog 中未声明的 subject 或 object class id。软件 predicate 当前都是 OWL object property，因此 literal object 会以 `literal_object_for_object_property` shape diagnostic 保留为 rejected statement；它不能绕过 entity range validation。该 schema 只描述 ontology contract，不读取文件、不访问网络、不拥有 graph storage，也不把 LPG 运行时改成 RDF triple store。

稳定实体的 `entity_key` 由 repository、受控类型、namespace 和规范化名称生成，不包含 commit 或 source scope；同一实体跨提交保持身份。每次证据观察另有 `occurrence_id`，它绑定 `entity_key`、source scope 和 evidence id。快照与事件实例故意把 source scope 纳入身份。依赖和 SDK 的版本、requirement、ecosystem 与来源保存在 occurrence 属性和 statement 中，不能仅用显示名称推断已解析版本。

`SoftwareStatement` 至少包含 `statement_id`、`subject_id`、`predicate`、互斥的 `object_id`/`object_value`、`source_scope`、`source_kind`、`evidence_refs`、`assertion_mode`、`resolution_state`、`valid_from`、`valid_to`、`observed_at`、`extractor_id`、`extractor_version`、`confidence_basis_points` 和 `fact_state`。Assertion mode 为 `declared|extracted|observed|verified|inferred`；resolution 为 `resolved|unresolved|ambiguous|external|conflicting`；fact state 为 `active|conflicting|superseded|rejected`。

## 3. 关系模型

Ontology statement 使用固定谓词；旧投影中的 `uses_sdk` 等兼容名称只属于旧读模型，不扩展受控词表：

| 关系 | 语义 |
| --- | --- |
| `depends_on` | 直接或传递依赖 |
| `contains` | system、component、artifact 或 deployment 包含另一实体 |
| `provides_api` / `consumes_api` | component、service、SDK 或代码提供/消费 API |
| `builds` / `produces` | build definition 处理输入并生成 artifact |
| `packages` | artifact 包含组件、文件或 SBOM |
| `configures` | 配置影响服务、构建或代码路径 |
| `deploys` | 部署单元安装或启动运行时服务 |
| `runs_as` | 部署或组件对应运行服务 |
| `tests` | 测试覆盖符号、配置、服务或 artifact |
| `documents` | 文档解释实体、关系、行为或约束 |
| `derived_from` / `observed_as` | 快照、artifact、部署修订和运行观察的来源关系 |
| `supersedes` | 版本、artifact、配置或事实替代旧版本 |

每个谓词有独立 authority policy，不存在一个覆盖所有事实的全局优先级。Manifest 声明 dependency requirement，lockfile、SBOM 或 build attestation 支撑 resolved dependency；build file/CI 声明构建设计，attestation 支撑结果；IaC/service definition 声明期望部署，授权 runtime/connector 记录 observed state；机器可读 API schema 和代码支撑 API contract。来源矛盾时，statement 并存并进入 `conflicting`，不得静默覆盖。

Shape validator 在发布前检查 evidence 与 extractor 完整性、subject/object 基数、有效期、observed timestamp、confidence 范围、稳定身份、跨 scope evidence/reference 和谓词 domain/range。失败形成可查询的 `software_ontology_diagnostics`；不合规 statement 保留为 `rejected`，不能成为 accepted fact。

## 4. 变化传播

全域更新使用同一事件链路：

```text
source or manifest changed
  -> evidence extracted
  -> candidate software facts produced
  -> graph mutation committed
  -> affected scopes recorded
  -> dependency/sdk/build/test/retrieval refresh tasks enqueued
  -> read model cursors advanced or stale/degraded diagnostics recorded
```

传播规则：

- manifest、lockfile、BOM、构建脚本、SDK metadata 和 import/include 事实都可以触发 dependency refresh。
- SDK 或生成器版本变化必须影响 generation context、API surface read model 和相关测试建议。
- 构建 target 变化必须影响可达源码、条件编译、发布 artifact 和部署单元。
- 配置变化必须影响 guarded code、runtime service diagnostics 和测试选择。
- 任何 worker 失败只改变派生索引状态和 dead-letter 记录，不得回滚已提交图事实。

## 5. 检索与生成上下文

全域检索继续使用 BM25、语义、向量和图路径融合，但候选和解释必须覆盖软件生命周期要素。面向生成的上下文包应包含：

- 当前 repository snapshot、build target、目标平台和语言。
- 依赖、SDK、lockfile、SBOM、feature flag 和 generator 版本约束。
- 可用 API surface、deprecated API、unresolved external target 和证据来源。
- 相关代码符号、测试、文档、发布 artifact、运行时诊断和影响路径。
- read model freshness、冲突事实、置信度和降级原因。

如果这些约束缺失，生成入口必须把缺口作为风险暴露给调用方，而不是扩大授权范围或扫描未索引目录。

## 6. 验收标准

- SDK 或依赖版本变化能产生 affected scope，并驱动派生 read model 刷新或 stale 诊断。
- 生成上下文能说明它使用的 SDK、依赖、构建 target、配置、测试和文档证据。
- SBOM 依赖和源码 import/include 事实可以关联，但未授权外部依赖仍保持 unresolved。
- 查询、CLI、Web 和 Agent context pack 能展示全域要素的新鲜度、解析状态和证据来源。
- 全域模型不复制单仓代码事实，不破坏 repository snapshot 作为代码事实最小分区。

## 7. 首版实现切片

首版基础能力以 repository snapshot/source scope 为边界，把现有代码索引事实投影为软件全域读模型：

- `software_components` 从 `code_repository_dependencies` 生成，区分 manifest `declared` 和 lockfile `locked`，保留 ecosystem、package name、requirement、resolved version、dependency group、证据路径和行号。Declared row 继续按证据位置独立保留，因为 manifest 目录承担 dependency-usage owner 语义；重复 locked row 只在该派生模型内按 repository/source-scope 级语义键 `(ecosystem, package_name, requirement, resolved_version, dependency_group, source_kind, language_id)` 合并，并确定性选择排序最前的 `(evidence_path, line_start, line_end)` 作为代表。合并后的投影仍严格限制为 65,536 个 component，第 65,537 个不同语义 component 必须使投影失败；权威 `code_repository_dependencies` row 与 `repo query --kind sbom` 证据不得删除或合并。
- `software_dependency_usages` 把 declared 依赖组件与匹配的代码/配置 import 证据关联；匹配依据是 module root 与 package identity，保留 import 的 `resolution_state`、`target_hint`、证据路径和 confidence，但不解析未授权包源码。生成文件的 import 仍保留为 code/SDK 事实，但不参与该派生依赖匹配；单条匹配输入保持 32 KiB 上限，完全相同的 module/target hint 只计费和扫描一次，不同文本仍累计计费并在越界时使投影事务失败回滚。
- `software_sdk_usages` 从 unresolved、ambiguous 或 external 的 `code_repository_imports` 生成，用于表达 SDK/API surface 使用候选，保留 `resolution_state` 和 `target_hint`，但不解析未授权外部源码。
- `software_files` 从 `code_repository_files` 生成，把代码、配置、文档、构建、部署、测试、模板、机器可读 API schema 和 knowledge map 文件作为整体节点。JSON/YAML 文件名中独立、大小写不敏感的 `openapi` 或 `swagger` token 会得到 `api_schema` role；包含同类 token 的生成式源码 client 仍保留普通 source/generated 分类。
- `software_files` refresh 严格保持每页最多 512 条权威事实，以唯一 `(source_scope, path)` keyset 前进，不再用 `OFFSET` 重扫已消费前缀；每条 row 继续经过同一个领域构造器校验，整个 file phase 只复用一个 prepared projection insert。带 fence 的 software projection v2 按 reset、dependencies、SDK usages、lifecycle、files、topics、relationships、ontology、publish 九个 checkpoint phase 分事务提交；每个 phase 都在事务前后校验同一 task/attempt/generation/lease fence，phase rows 与下一个 checkpoint 原子提交。中间 rows 始终属于 stale、不可查询 scope，只有最后 publish transaction 才一起发布 code scope、software status 和 checkpoint，因此释放 SQLite writer 以供续租不会削弱 freshness、rollback 或可见性语义。遗留 v1 的 `publish` checkpoint 会从新增 ontology phase 恢复，不跳过本体物化。
- `software_topics` 从 Markdown/spec heading 和 `knowledge/knowledge-map.yaml` 的 topic id 生成，用于表达仓库文档主题、架构约束和知识路由主题。普通 README heading，包括 “Getting Started” 和 “Chapter Index”，只能成为 documentation topic，不能仅凭标题或路径晋升为 `SoftwareSystem`。
- 兼容关系（`depends_on`、`uses_sdk`、`configures`、`references`、`documents`）通过同一快照的 component、SDK、configuration 和 topic fact 有界关联读取。Schema 8 不再把可重建的边 payload 复制到 `software_relationships`。SQL 在结果 limit 前施加 scope 和请求过滤；稳定 ID、端点、证据、置信度与 unresolved hint 保持可见。配置 target ID 在身份分区前复用 domain 的 Unicode 空白规范化；path 和源文件 language 条件在配置窗口前施加，被排除的 usage 不进入排序，但 SQLite 仍可能扫描 scope 内源事实来选择候选。同一规范化关系身份的配置 usage 按最高置信度、最大结束行、usage ID 顺序去重，不同关系类型仍分别保留。Durable relationships 阶段逐行复用读取时的 domain 校验，包括未选中的配置重复项，再发布选中关系的精确计数；内存只保留当前边，不写兼容行，无效字段必须在发布 fresh 前失败。类型化 ontology statement 承载归并后的 provenance 和 fact state，继续持久化。
- `software_build_targets` 从已索引 chunk 中的构建证据生成，覆盖 Dockerfile/Containerfile、Cargo、npm、Python、Go、Maven effective `pom.xml`、Gradle、CMake、Makefile 和 CI workflow 的 definition、package、script、target、feature、module、profile、plugin、goal、pipeline 和 job 等入口。Dockerfile 是 `BuildDefinition`，镜像提示是 `ReleaseArtifact`；GitHub Actions/GitLab CI job 是 `BuildJob`，不再伪装成 IaC resource。Maven effective model 只基于已索引证据解析仓库内 parent POM、properties、dependency management、plugin management、modules、profiles 和 imported BOM 声明；投影只记录证据和命令提示，不执行构建工具、不读取包缓存、不访问 registry。
- `software_iac_resources` 只从 Compose、Kubernetes YAML、Helm、Terraform、systemd 和 launchd 等明确部署证据生成，保留 provider、resource kind、name、scope hint、target hint 和解析状态。Dockerfile 与普通 CI job 不进入该投影；查询也不访问云 API或推断集群实时状态。
- `software_design_elements` 保留旧兼容响应，但 Markdown heading 默认只投影为 `DocumentationUnit`。只有显式 frontmatter（`software-system`、`system`、`component`、`api`、`resource`）、受控 manifest/schema 或其他结构化代码证据才会晋升为 `SoftwareSystem`、`Component`、`Api` 或 `Resource`。
- `software_entities`、`software_statements` 和 `software_ontology_diagnostics` 与旧表并行物化。API trait/interface/protocol、OpenAPI/Swagger schema 文件、测试符号、配置 flag、构建定义、release artifact、deployment unit/resource、service definition 和文档单元进入类型化实体；所有 accepted active statement 都必须具有同 scope evidence、source kind 和 extractor version。Projection schema version 8 会把旧 scope 标为 stale，并通过既有 durable task、lease、checkpoint 和单仓单写者路径重建，不做破坏性原地转换。
- 无查询文本的类型化读模型使用确定性的 evidence-priority，而不是名称字母序。`topics` 先展示有明确目录上下文的文档 heading，再展示 knowledge-map topic 和根级概览；`design` 依次优先 architecture、capability、module，再展示 API/system metadata；`apis` 优先 API schema 与代码声明，`resources` 沿用 Kubernetes、Terraform、Compose、systemd、launchd、Helm 的 IaC provider 优先级，`deployments` 优先平台 service definition，再展示 IaC 与 runtime observation。所有规则只排序已按 source scope、kind、path 和 language 过滤的物化行，并保留 name/path/identity 的稳定 tie-break；不得为排序读取 live source、扩大 limit 或枚举仓库、case、路径和符号。
- `software_global_status` 除旧计数和最后错误外，还记录 `ontology_version`、`projection_schema_version`、`source_coverage`、`completeness_basis_points`、`freshness`、`entity_count`、`statement_count`、`conflict_count` 和 `diagnostic_count`。`completeness_basis_points=10000` 表示当前投影中的 statement provenance 完整，不表示授权范围之外的世界知识完整。
- CLI、Web 和 MCP 共享同一个 application service。兼容 kind `dependencies|sdks|files|topics|relationships|build|iac|design|all` 保持不变，并新增 `systems|apis|resources|tests|deployments|releases|statements|conflicts`。Web Software 页面从有界 repository list 选择固定 commit，并行读取 `statements` 与 `conflicts`，展示稳定实体关系、provenance/freshness 和冲突/shape diagnostics；它不建立独立前端事实。`relay-knowledge repo software export <alias> --profile spdx-3|cyclonedx-1.7|prov-o` 从同一 snapshot-bound statement 视图导出互操作文档；查询和导出都只读取已提交投影，不在热路径扫描包缓存、SDK 目录、云 API、未索引外部源码或全仓文档。
- 对 `repo software --kind all`，`--limit` 是十二个响应数组共享的严格总上限，固定优先顺序为 `components`、`dependency_usages`、`sdk_usages`、`files`、`topics`、`relationships`、`build_targets`、`iac_resources`、`design_elements`、`entities`、`statements`、`diagnostics`。查询从每个切片读取不超过请求上限的候选，再逐轮为每个非空且未耗尽的切片分配一条；未使用额度在下一轮继续分配。若保留的 statement 需要额外的 entity endpoint，只可为有序且可表示的 statement 集合的累计 endpoint 回收后续轮次的剩余额度，既不挤占任一切片的首个公平行，也不会返回悬空 statement 引用。累计候选被拒绝后，endpoint 预留和 entity 选择必须针对缩减后的 statement 集合重新计算，直至最终预算收敛。statement endpoint 的多批查询会在公平分配前恢复全局 canonical entity 顺序；其 entity 或 statement evidence 未通过请求 path/language filter 的 diagnostics 会通过 SQL join 完整过滤后的 evidence 集合排除，再读取有界 projection 候选。因此，上限不小于非空切片数时，每个非空切片至少返回一条；上限更小时，按上述顺序确定性地优先较早数组。该分配不改变各切片自身的证据排序或请求最大值 500，所有数组合计返回行数绝不超过 `--limit`。

## 8. Knowledge 开发闭环边界

仓库 knowledge map 只保存指向 repository root 的稳定 `software-model`
路由，不复制 projection row 或生成式 narrative。Repository bootstrap、固定
ref 的 spec context 和 commit 后的一致性恢复由
[代码地图驱动的 Knowledge 开发闭环](24-code-map-backed-knowledge-development-loop.md)
定义。Code index publication 仍是按 source scope 刷新这些软件投影的唯一写入路径。


### Maven 模块图与下游影响（#390）

`repo software <alias> --kind modules --ref HEAD --limit 500 --format json` 返回独立的 Maven reactor 视图：`build_targets` 每个已索引 POM 只有一个 `kind=reactor_module`、`language_id=jvm` 节点（包括聚合项目），`relationships` 包含 `aggregates`、`depends_on` 与 `inherits_from`。模块 ID 由 repository identity 与大小写敏感的 POM 路径确定，跨 ref、版本和行号变化保持稳定；SQL 主键同时包含 snapshot scope。`output_hint` 是模块目录，源码文件按最长目录前缀归属模块。

依赖沿用已有 effective POM 的 parent、properties、dependencyManagement 和本地 imported BOM 解析结果，不执行 Maven、不读取包缓存、不访问 registry。只有 group/artifact、显式有效版本、无 classifier 且 type 与本地 packaging 匹配的唯一候选才解析为模块目标。system scope 的文件系统制品、外部制品、版本不匹配、缺失版本和 classifier 目标保留 `unresolved`；重复坐标保留 `ambiguous`，均不制造 parser degradation。依赖 `target_hint` 保留版本、scope、type、classifier、optional 与 profile；聚合关系不等价于构建依赖。

本地 parent 声明通过独立 `inherits_from` 边指向 effective model 实际使用的父 POM；外部或坐标不匹配的父 POM 保留未解析声明证据。activeByDefault profile 的 modules 与顶层模块声明一起形成聚合边，未启用的 profile 不默认参与。effective model 与 workspace 的 XML 解析都支持命名空间前缀和 CDATA；普通文本解码 XML 预定义实体和数字字符引用，CDATA 保留字面值。未知实体明确拒绝，不读取外部实体。

`repo impact` 从变更路径所属模块反向遍历已解析、默认 profile 的直接依赖（包括 BOM import）和父 POM 继承边，返回依赖声明所在 POM 行和 `edge_kind=module_depends_on`，`edge_target_hint` 为直接或多跳 POM 路径链。这是保守的下游构建/验证影响候选，不是 Maven 传递 classpath 求解；test/provided/optional 声明也可使直接消费者需要重新验证。显式 profile 变体在图中可见，但未请求 profile 激活时不假定其启用；聚合边不参与传播。POM impact 使用 XML 证据，同时遵守索引 scope 和请求的路径/语言过滤；仅请求 Java 时不返回 XML POM 命中。任一语言过滤排除 XML 时直接跳过 reactor 遍历，因此不相关的不完整 POM 图不会中断该请求。当前传播依据 head snapshot，不借用未授权仓库或已删除模块的历史图。

reset 不解析 POM，暂存旧 Maven 构建事实；lifecycle 阶段只进行一次有界 effective model 加载，供 reactor 与 build 提取共同使用。证据不完整时保留两者，完整时在同一 fenced 事务中替换。self-iteration fast 性能套件通过 128 个 POM 的 SQL trace 要求 reset 加 lifecycle 合计只有一次模型物化。

模块图在现有 fenced software Lifecycle transaction 内替换，与 checkpoint 同提交；全量、增量和 overlay 都走该边界。解析证据截断或损坏、POM 为空或不是 project 文档、以及索引 POM 缺少源码 chunk 时，保留已有记录并设置 reactor incomplete marker；modules 查询与模块 impact 明确报错，不能用旧图冒充完整图。每 scope 最多 8192 个模块、131072 条边，每条持久化事实最多 32 KiB；BFS 最多 64 层，使用 visited 集合终止循环。dependencies 与 modules 使用基于稳定 ID 的分页，每页所有数组合计不超过 limit（1–500）。非末页返回 `next_cursor`，CLI 通过 `--cursor`、Web/MCP 通过 `cursor` 继续读取，并保持 ref、kind 和过滤条件一致。读取顺序为模块、声明关系，再到 dependencies 的组件和源码使用记录；每类按稳定 ID 排序，末页省略游标。非 JVM 语言过滤跳过 reactor 完整性检查；即使不相关的 POM 证据不完整，仍可读取符合过滤条件的组件与使用记录。每次读取使用 SQLite 快照及至多一条前瞻记录。游标绑定索引 scope、过滤条件和投影版本，变化或损坏时明确拒绝；Web 仓库直连路由在请求规范化时同样保留并校验传入游标，与 operation 路由及 CLI/MCP 一致；其他 kind 和 graph-only 不支持游标。关系可能引用前页节点，客户端合并各页后组装完整图。路径 SQL 谓词由 SQLite scope_filters 统一提供，避免 reactor 与 software 投影之间的循环依赖。

Workspace detector 可显式启用 `maven`，从授权 snapshot 的根 POM 递归读取 modules，识别声明的 group/artifact（包括 parent groupId）和 activeByDefault profile 的 modules，不依赖 activation/modules 的元素顺序。它保持独立于始终执行的 effective module graph projection；`workspace_detection.enabled=false` 不表示 Maven 事实禁用。原命令 `--kind dependencies` 现在同时返回 `build_targets` 中的 reactor 模块和 `relationships` 中的 POM 声明依赖，保留组件及源码使用证据。`dependency_usages` 仍表示源码使用次数，不表示声明数量；`--kind build` 与其全局状态计数仍是构建事实，不能用作模块总数。

回归覆盖缺少 reactor 表的旧数据库导入、父 POM 与 BOM 影响、system 制品、默认 profile 聚合、XML 前缀与 CDATA、两模块以上的多跳传播、循环与预算、坐标歧义、外部依赖、profile 变体、增量删除依赖、稳定身份，原 dependencies 命令在没有 Java import 时的声明依赖输出、游标校验，以及包含 153 个节点和 607 条关系（含 152 条父 POM 继承边）的 152 子模块跨页读取。Maven 语义参考 [reactor 文档](https://maven.apache.org/guides/mini/guide-multiple-modules.html) 和 [依赖机制](https://maven.apache.org/guides/introduction/introduction-to-dependency-mechanism.html)。

---

导航: 上一章: [20. 多仓库代码图谱薄覆盖层](20-multi-repository-code-graph-overlay.md) | 下一章: [22. 服务化部署、控制面与数据面分离](22-service-deployment-control-data-plane.md)
