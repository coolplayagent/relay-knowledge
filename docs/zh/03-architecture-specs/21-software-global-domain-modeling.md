# 软件全域建模架构

[中文](../../zh/03-architecture-specs/21-software-global-domain-modeling.md) | [English](../../en/03-architecture-specs/21-software-global-domain-modeling.md)

> 文档版本: 1.0
> 编制日期: 2026-05-28
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

软件全域建模把源码图、依赖图、构建图、配置图、测试图、发布图和运行时诊断图纳入同一版本化事实空间。它不是替代现有代码知识图谱，而是在 `repository_snapshot`、source scope、mutation log、派生索引新鲜度和后台恢复机制之上增加软件生命周期要素。

设计必须满足四个约束：

- 基础事实仍按真实 source scope 分区，不能为了全域视图复制或混合单仓代码事实。
- SDK、依赖、构建 target、生成器、配置、测试和发布 artifact 是一等实体，不是代码 chunk 属性。
- 所有变化传播必须经过 durable graph mutation 和 bounded refresh task，不能由查询热路径递归扫描仓库、包缓存或 SDK 目录。
- 缺失外部源码、未授权依赖和未安装 SDK 只能形成 unresolved edge metadata，不能写成 resolved graph facts。

## 2. 核心模型

全域模型在现有 `CodeRepository`、`CodeFile`、`CodeSymbol`、`CodeChunk` 和 `CodeChangeSet` 之上增加以下实体族：

| 实体 | 责任 |
| --- | --- |
| `SoftwareSystem` | 产品、服务或工具的稳定业务身份 |
| `BuildTarget` | 构建入口、profile、平台、feature 和输出 artifact |
| `PackageComponent` | 包、模块、库、容器镜像或第三方组件 |
| `Sdk` | 平台 SDK、编译器、系统 header、语言运行时和生成 SDK |
| `Generator` | 代码生成器、schema compiler、IDL compiler 或模板引擎 |
| `Configuration` | 环境变量、配置 key、feature flag 和部署参数 |
| `RuntimeService` | 安装后的后台服务、HTTP endpoint、worker 或外部服务依赖 |
| `TestCase` | 单元、集成、浏览器、性能或验收测试入口 |
| `DeploymentUnit` | service definition、容器、包管理器安装单元或平台部署单元 |
| `ReleaseArtifact` | 二进制、压缩包、安装器、SBOM、checksum 或 release note |
| `Vulnerability` | 漏洞、弱点、受影响版本区间和修复建议 |
| `License` | 许可证、例外、归属和合规约束 |
| `DocumentationUnit` | 需求、设计、接口、运维和发布文档 |

实体主键必须绑定稳定身份和 scope。依赖包和 SDK 版本不得只用名称做身份；至少要包含 ecosystem、name、version/range、source authority 和 scope。

## 3. 关系模型

全域关系至少包括：

| 关系 | 语义 |
| --- | --- |
| `depends_on` | 直接或传递依赖 |
| `uses_sdk` | 源码、构建 target 或生成器依赖 SDK/API surface |
| `generates` / `generated_from` | 生成器、schema、模板和生成文件之间的关系 |
| `builds` | build target 生成 artifact |
| `packages` | artifact 包含组件、文件或 SBOM |
| `configures` | 配置影响服务、构建或代码路径 |
| `deploys` | 部署单元安装或启动运行时服务 |
| `tests` | 测试覆盖符号、配置、服务或 artifact |
| `documents` | 文档解释实体、关系、行为或约束 |
| `exposes_api` | SDK、包、服务或符号暴露 API surface |
| `affects` | 变更、漏洞、配置或依赖影响其他要素 |
| `constrains_generation` | SDK、依赖、配置、平台或文档约束代码生成方向 |
| `supersedes` | 版本、artifact、配置或事实替代旧版本 |

每条关系必须携带 `source_scope`、`graph_version`、`resolution_state`、`confidence`、`evidence_refs`、`valid_from` 和 `valid_to`。跨仓、跨包或跨 SDK 关系还必须暴露 target hint 和解析依据。

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

- `software_components` 从 `code_repository_dependencies` 生成，区分 manifest `declared` 和 lockfile `locked`，保留 ecosystem、package name、requirement、resolved version、dependency group、证据路径和行号。
- `software_sdk_usages` 从 unresolved、ambiguous 或 external 的 `code_repository_imports` 生成，用于表达 SDK/API surface 使用候选，保留 `resolution_state` 和 `target_hint`，但不解析未授权外部源码。
- `software_global_status` 记录每个 source scope 的 projected graph version、stale 状态、组件数、SDK usage 数和最后错误。
- CLI 通过 `relay-knowledge repo software <alias> --kind dependencies|sdks|all` 暴露投影结果；查询只读取已提交投影，不在热路径扫描包缓存、SDK 目录或全仓源码。

---

导航: 上一章: [20. 多仓库代码图谱薄覆盖层](20-multi-repository-code-graph-overlay.md)
