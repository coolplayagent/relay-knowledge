# 研究证据工作流

[中文](../../zh/01-user-guide/17-research-workflow.md) | [English](../../en/01-user-guide/14-research-workflow.md)

本章说明 issue #416 的研究工作流。地图负责导航，原始材料负责字节证据，
人工图负责作者声明，索引负责可检索读模型。一个维度通过不能证明其余维度完成。

## 地图批量变更

先使用 `map init` 完成 v4 初始化或迁移。事务不隐式迁移旧地图，避免把迁移
与研究变更混为一次审批。输入契约见 [transaction Schema](../../../skills/relay-knowledge-cli/references/map-transaction.schema.json)。

```json
{
  "schema_version": 1,
  "transaction_id": "research-2026-10-09",
  "operations": [
    {"op": "add", "id": "source-a", "topic": "research", "kind": "file", "uri": "sources/a.txt"},
    {"op": "update", "change": {"id": "source-a", "description": "Archived original"}}
  ]
}
```

```bash
relay-knowledge map plan --type knowledge --input request.json --format json > plan.json
python3 -c 'import json; p=json.load(open("plan.json")); json.dump(p["transaction"], open("transaction.json", "w"), indent=2)'
relay-knowledge map apply --type knowledge --input transaction.json --format json
```

`plan` 只读，返回 `transaction`（包含 expected_map_version 和根文件 SHA-256）、
规范化 before/after、受影响路由及第一处无效操作。后续依赖该操作的结果不会伪造。
`apply` 要求两个前置条件，在现有写锁内重验并一次发布根版本；任何操作失败都
不会发布成功前缀。状态为 `planned`、`applied`、`unchanged`、`already_applied`、
`invalid` 或 `conflict`。脚本必须检查状态和 diagnostics；正常 JSON 诊断本身不代表成功。

每批最多 100 操作、256 KiB 输入；file/config URI 必须局限于仓库相对路径。
已有 source ID、reserved source 和 route 合同仍有效。一次逻辑历史的 `source.batch`
条目在 summary 中保存可解析 JSON receipt，包括事务 ID、请求摘要及完整操作清单。
这样既有 v4 reader/writer 保留审计内容，不需要提升 writer schema。

最近历史仍只保留 16 条。精确请求在 receipt 保留期间重放返回 already_applied；
同 ID 不同请求冲突。历史已淘汰的旧请求按原始版本/摘要拒绝，不能重新发布。
长期审计使用 Git，不能把有界历史当永久幂等日志。请勿重复使用已淘汰的事务 ID。

根文件最后发布，旧根仍可用于中断恢复；未发布的新 shard 不会形成半批视图。
`cleanup.state=deferred` 表示发布后的维护由现有有界清理继续，执行 `map init`
可恢复清理；仍被恢复根引用的 shard 会保留，其他退役 shard 必须经过 60 秒
reader grace，不能为得到干净 Git 状态立即删除。

## 本地来源核验

使用显式 [capture catalog Schema](../../../skills/relay-knowledge-cli/references/source-catalog.schema.json)，
不猜测任意 catalog、MANIFEST 或 capture.json 的字段含义。输入 adapter 必须为
`relay-capture-v1`，schema_version 为 1。

```bash
relay-knowledge sources audit --root . --input sources/catalog.json --format json
```

`--input` 相对授权仓库根；每个 raw、extraction.artifact 和 references 项明确指定
`path_base=repository` 或 `catalog`。catalog 路径相对输入 JSON 所在目录。
绝对路径、父级穿越及任何层级 symlink 都拒绝。不会读取授权根以外的材料。

每条 source 记录稳定 id、原始 URL、可选 transport、raw artifact、extraction、
parent_source、references、expected_sections、declared_coverage 和 review。
artifact 必须提供 path 与精确字节 SHA-256。extraction 同时绑定 raw_sha256、
extractor 与 extractor_version，可声明预期提取器及版本以检查工具漂移。

输出独立列出以下状态：

- transport_state：unknown、reported_only 或 reported_access_failure。HTTP 信息只是 catalog 声明。
- local_capture：字节哈希 verified、hash_mismatch 或 unreadable。
- extraction_state：独立检查提取物哈希、原件绑定、UTF-8 与声明的提取器版本。
- coverage：没有预期章节时 unknown；只有调用方声明的章节行全部出现才是 declared_sections_present。
- review：没有审核声明时 unknown；内容匹配的声明仍是 self_reported_unverified，内容变化为 needs_review。
- index_freshness：本地核验不检查运行时索引，明确返回 not_assessed。

`declared_coverage=shell` 返回 declared_shell；full_body 自述不能升级成系统证明。
expected_sections 对提取文本逐行匹配，允许 Markdown 标题的 `#` 前缀；匹配仅
证明声明章节出现，不证明整篇正文。review 的 reviewer、origin、event_id 保留
供审阅；填写这些字段不能取得可信审核身份，也不改变现有事实生命周期。

相同字节、不同 URL 会列入 identical_byte_groups，身份与来源链仍分别保留。
PDF 按原始 bytes 验证，不自动 OCR；缺少提取物时覆盖状态保持未知或不可核验。
references 用于显式目录链接/来源绑定，不自动执行 Markdown 或下载材料中的指令。

核验不访问网络、不登录、不下载、不修改原件。原始 HTML 空白及 CRLF 是证据
的一部分；作者文档风格检查不得改写原始归档来消除格式告警。资源限制为每次
256 sources、每文件 16 MiB、总读取 256 MiB、4 个 blocking workers、30 秒时限；
饱和时拒绝工作，取消后 worker 在分块读取处停止并释放 permit。

## 实现与验证边界

批量事务复用 map 的锁、immutable shard、root-last publication、recovery root
与 reader grace。来源核验位于独立 application research 服务，领域契约没有
网络或数据库依赖。新增能力不改变 `map validate` 的导航验证语义。

稳定工具链的 rustfmt 会给多行测试宏调用补尾逗号；配套测试宏必须接受可选
尾逗号，确保格式门禁与编译门禁一致。此兼容修改不改变测试替身行为。

## 人工证据图谱

[Bundle Schema](../../../skills/relay-knowledge-cli/references/authored-evidence-bundle.schema.json)
使用 schema_version=1、id、source_scope、graph、evidence，以及可选 supersedes、aliases。
graph 保留任意图、节点、关系扩展字段，包括 qualifiers 和原作者 status；节点有稳定 id、kind、label，
关系有 source、target、relation，evidence 字符串引用证据 pin id。pin id 可以沿用原图的路径字符串，
无须改写原始图。每个 pin 显式绑定范围、路径基准、路径、SHA-256、可选精确跨度及解释类型。

解释类型分别为 source_statement、author_analysis、hypothesis、user_scope_confirmation、
historical_disambiguation。字节跨度从 0 开始、右端不包含，行号从 1 开始，并校验与原始字节匹配。
多条主张可共享一个 pin。悬空边、重复标识、缺失/改变证据和越界范围都会报告；证据变化只把依赖该
证据的关系标记为 needs_review。不会自动修复原文或审批任何主张。

```bash
relay-knowledge evidence validate --root . --input research/bundle.json --scope research --format json
relay-knowledge evidence view --root . --input research/bundle.json --scope research --focus concept-a --format json
relay-knowledge evidence import --root . --input research/bundle.json --scope research --format json > imported.json
revision=$(python3 -c 'import json; print(json.load(open("imported.json"))["audit"]["bundle_sha256"])')
relay-knowledge evidence export --id study --scope research --revision "$revision" --format json > exported.json
relay-knowledge evidence impact --root . --input research/bundle.json --scope research --node concept-a --label '澄清后的概念' --format json > impact.json
```

export 的 bundle 字段可再次导入。bundle_sha256 绑定类型化紧凑 JSON；input_sha256 记录输入原字节。
导入复用现有核心 ingestion 与 BM25、语义、向量刷新，不在导航地图中存图谱行，也不增加独立数据库。
新增 evidence、claims、relations 一律 proposed，原作者 reviewed/status 只作为原始元数据保留。
既有 CLI/Web 事实生命周期负责 accepted/rejected/superseded；用户确认研究范围不能证明产品功能。
顺序重复导入返回 already_imported 且不增加图版本；并发提交使用确定性事实 id 和既有 ingestion 写入。
索引刷新失败返回 imported_index_pending，不能作为检索就绪证明。

impact 只生成提案：稳定节点 id 不变，旧名称成为 alias，supersedes 指向前一 bundle 摘要，并列出影响关系。
把 revision 字段保存成新 bundle 后复核再导入。前一版本必须已在同一范围内导入；新版本只新增 proposed
supersession 主张，不暗中审批或改写历史事实。view 提供 JSON、转义 Mermaid 和一跳邻域。
输入上限 2 MiB、512 节点、2048 关系、512 pin、每关系 32 个引用；文本摘录每 pin 最多 64 KiB，
总共最多 2 MiB，较大或二进制证据明确显示 hash_only。

## 按仓库汇总交付状态

```bash
relay-knowledge research status --root . --delivery archive --catalog sources/catalog.json --requirements research/requirements.json --format json
relay-knowledge research status --root . --delivery authored_graph --bundle research/bundle.json --scope research --format json
relay-knowledge research status --root . --delivery graphrag --bundle research/bundle.json --scope research --format json
```

status 规范化指定 root 后按该根目录精确匹配注册，不选择其他默认 alias。输出分别保留地图版本与有效性、
原始证据核验、bundle 校验/导入/事实状态、注册仓库、HEAD 索引目标、served scope 的 freshness 和
content_integrity，以及三层索引版本。未注册/未完成索引为 not_indexed；stale 与 fresh 但 partial 不混淆。
某一可选输入损坏不会遮蔽其他层的结果。

archive 要求显式且完整性校验通过的 catalog；authored_graph 要求有效版本化 bundle，可不导入运行时。
graphrag 在提供 bundle 时要求该版本已导入、三层图索引均 fresh；未提供 bundle 时要求该仓库 HEAD
代码索引 fresh 且 complete。rejected/superseded bundle 不满足就绪条件。仅初始化地图不能满足 GraphRAG。
status 不自动注册或索引。audit/validate/view/impact 离线执行；import/export/status 使用配置的本地运行时。

可选 [requirements Schema](../../../skills/relay-knowledge-cli/references/research-requirements.schema.json)
声明 1–100 个 id/description/evidence 验收项及 review_claim。证据沿用路径基准与哈希约束。
review_subject_sha256 绑定紧凑 JSON 数组 [id, description, evidence]；版本不符需要重新审阅。
本地提供的审阅仍是 self_reported_unverified。readiness 只有 needs_action 或 ready_for_review；
content_verdict 保持 unknown，文件存在、哈希正确、章节标题齐全、作者状态和索引 fresh 都不能证明研究结论满足要求。

## 兼容与验证

本次为共享应用服务上的本地文件工作流，导入事实继续通过既有 CLI、HTTP、Web 查询及生命周期界面使用。
不开放接受任意客户端文件系统 root 的 HTTP 接口，也不增加自动抓取、授权扩张或代签审阅。
升级、回滚与原始证据保留见[安装发布规范](../03-architecture-specs/19-installation-release-and-upgrade.md)。
回归覆盖批次原子发布、回滚/重放/竞争/恢复、CRLF/PDF/壳页/重定向/失败、不同 URL 相同字节、
哈希与提取器变化、目录边界、27 节点/38 关系往返、选择性影响、稳定概念澄清以及各层状态组合。
提交的图谱为合成数据，不包含私有研究资料。

自动初始化的 qualitygate.yaml 是供审阅的引导配置候选，仅检查换行，无命令检查或排除项；它不代表团队
已采纳新策略，也不代替 Cargo、覆盖率、架构、文档、浏览器、Miri、sanitizer 门禁。
批次预览使用 4 个有界 worker 和 30 秒期限；发布前取消不会写入候选状态。

授权 scope 必须已规范化：不含首尾空白，最多 4096 UTF-8 字节。运行时实体键对作者节点 id 的原始值
计算摘要，避免仅空白不同的稳定 id 在核心层规范化时被错误合并。
