# 第 10 章 Worker、Proposal 与 Audit

[中文](../../zh/01-user-guide/10-workers-proposals-audit.md) | [English](../../en/01-user-guide/10-workers-proposals-audit.md)

Worker 负责把 CPU-heavy 或 I/O-heavy 工作移出查询热路径。Proposal 负责人工审核模型或外部 worker 产出的图谱变更。Audit 负责让 CLI、Web、service 和 agent 操作可追踪。

## 10.1 Worker 配置

多模态 evidence 写入后会进入持久 worker 队列。可配置外部 HTTP worker endpoint:

```text
RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT
RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT
RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED
```

worker endpoint 负责 embedding、OCR、视觉 caption、表格/layout 抽取等重任务。worker 结果先进入 proposal 或 multimodal extraction commit path，不在查询热路径里同步调用外部服务。

## 10.2 常用命令

```bash
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
relay-knowledge proposal show <proposal-id> --format json
relay-knowledge proposal accept <proposal-id> --by <actor> --reason "reviewed"
relay-knowledge audit query --limit 50 --format json
```

未配置外部 endpoint 时，`worker run-once` 使用 deterministic fallback 生成 proposal，不阻塞 BM25、graph retrieval 或 ingest。proposal 必须人工 accept 后才会通过 graph mutation pipeline 写入 accepted facts。

## 10.3 Extractor Contract

设置 `RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT` 后，foreground worker 会通过 `net::http` 按全局 request timeout 发送 `contract_version=2` 的 JSON 请求。请求携带 manual-review policy、timeout/lease/max-attempts/max-in-flight 预算，以及 provenance 要求。

外部 extractor 返回的 `ingest_request` 会继续走 proposal 存储，不会直接提交 graph mutation。其中 relation、claim 和 event 即使声明为 `accepted`，也会在 proposal payload 中被降为 `proposed`，避免模型抽取或关系推断绕过事实审批。

## 10.4 Provenance

Worker 返回值可以附带 `provenance` 对象，字段包括 `producer`、`provider`、`model`、`prompt_id`、`prompt_version`、`schema_version`、`input_source_hash`、`input_fact_ids`、`stale_when` 和 `budget_notes`。这些 metadata 会随 proposal 持久化，供 CLI/Web/API 审核和 audit 查询使用。

## 10.5 Audit Sink

Agent audit 持久化默认关闭。开启后，MCP 和本地 ACP audit events 会通过有界 async queue 写入 `paths` 管理的 log 目录:

```text
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

队列深度在运行时 capped 到 65536。队列满时持久镜像可以丢弃事件，内存 audit log 仍保留最近事件。CLI/Web/service operation 还写入持久 audit sink，可通过 `audit query` 检查最近操作。

## 10.6 软件使用体验反馈

`feedback` 支持 bug、缺失能力、结果不佳、工作流摩擦、性能与文档问题，使用独立持久生命周期，不写入已接受图事实，也不复用 proposal 审核。exit 0 不代表结果满意；缺依赖、权限拒绝也不自动判为产品缺陷。

输入遵循[反馈 Schema](../../../skills/relay-knowledge-cli/references/feedback.schema.json)及[工作流示例](../../../skills/relay-knowledge-cli/references/feedback-workflows.md)。`intent`、`expected`、`actual`、`impact`、observations 和可选 reproduction 必须是允许公开的最小摘要。观察来源区分 `agent-observation`、`user-experience`、`cli-fact`、`hypothesis`，表示调用者的来源声明，不代表 CLI 自动证明其真实性。私有日志和知识原文仅放 `evidence`，基线元数据放 `diagnostics`，两者始终留在本地。引用原操作时复制结构化输出 `metadata.trace_id`/`metadata.request_id`；缺省时补当前反馈请求身份。CLI 自行记录实际版本和平台。

```bash
relay-knowledge feedback report --input feedback.json --format json
relay-knowledge feedback status --format json
relay-knowledge feedback preview <feedback-id> --format json
```

默认 `local-only` 只保存持久草稿。preview 展示本地准备好的完整公开 title/body、公开 payload digest、独立原始 report digest 及省略证据标签。复用的远端 issue 可能保留另一个 outbox 的 nonce，因此本地 preview 不声明与当前远端正文逐字节一致。原始 evidence、diagnostics、trace ID 不进入 issue。公开叙述包含可识别凭据、邮箱、私人路径或不安全控制字符时进入 `evidence-insufficient`；需要重新提供可公开最小摘要，不能由模型豁免。扫描器不能判断任意自然语言是否属于私有知识，即使没有敏感模式，也不得把私有知识放入公开叙述。

内容绑定的范围分别为：

| 字段 | 内容与生命周期 |
| --- | --- |
| `raw_report_digest`（status 中为 `evidence.raw_digest`） | 不可变本地原始 report，包含私人证据，绝不发布。 |
| `publication.payload.digest` | 不可变本地完整 title/body，包含 marker。 |
| `publication.payload.dedup_marker` | 加入恢复元数据前的已脱敏公开 title/body 稳定 digest，不受私人证据或本 outbox nonce 影响。 |
| `publication.issue.body_digest` | adapter 实际观察到的远端 issue 正文 digest；远端正文变化后，track 会刷新它。 |

这些 digest 的内容范围不同，不应直接比较本地 title/body digest 与远端 body-only digest，也不能把去重复用的 issue 正文报告成本地 preview 的逐字节副本。

## 10.7 显式发布与恢复

本地 `feedback configure --input feedback-policy.json` 保存版本 1 策略：`mode`、`target_repository`、`allowed_kinds`、`daily_quota` 及可选 `validation_runner`。`auto-submit` 必须明确 `owner/repository` 和非空 kind 白名单。配额为持久 24 小时窗口内 1–100 次创建尝试，默认 5。configure 本身不提交已有草稿。通过进程或托管服务环境提供 `RELAY_KNOWLEDGE_FEEDBACK_GITHUB_TOKEN`，凭据需要指定仓库的 issue 读取/创建权限；不要把凭据值写入报告、策略或 issue 正文。

```bash
relay-knowledge feedback configure --input feedback-policy.json --format json
relay-knowledge feedback submit <feedback-id> --format json
relay-knowledge feedback retry <feedback-id> --format json
relay-knowledge feedback track <feedback-id> --format json
```

启用后 report 可由内置 GitHub adapter 创建真实 issue 并返回 URL，不委托 Agent 运行 `gh`，也不执行日志指令。submit/retry 仍检查策略、类型、配额与已固定的目标/payload。相同公开场景、观察、影响及 CLI 版本的本地重复报告合并出现次数。原始 report 不可变，后续重复请求的 raw evidence 不追加；修正公开叙述会得到新 ID。随机不透明 nonce 用于单次发布恢复；另一个稳定 marker 只对已脱敏、允许公开的 title/body 求哈希，让不同 outbox 的相同公开报告复用已有 issue，不暴露私有内容指纹。独立实例同时创建仍可能竞争，只有共享 outbox 的进程受串行保护。当前 adapter 不发布评论，也不重开 issue。

outbox 使用跨进程序列化并在 HTTP 前持久化发送意图；最后一个事务/工作线程所有者显式释放系统锁，避免无关子进程短暂继承描述符而延迟后续事务。取消提交时，磁盘工作线程完成前仍保留锁。最多 1,000 条记录/16 MiB，每条最多五次创建尝试，重试延迟有界。容量耗尽保留现有记录并拒绝新写入。离线、认证失效、权限或配额不足均返回明确状态、原因和下一次尝试时间。发布失败不丢弃已保存报告，也不改变此前知识查询结果。`retryable-failed` 在期限后可安全重试；`blocked` 需按原因修复配置、认证或预算。

GitHub create-issue 没有客户端幂等键，因此发送结果不确定或进程中断后不会自动再次 POST。retry 只核对原 nonce；命中后返回真实 URL。未命中保持 `awaiting-reconciliation`：远端暂时不存在不能证明在途请求以后不会成功。远端按仓库与 nonce 定向查询，最多两个候选、30 秒及 4 MiB；不完整、歧义或数量不一致的结果阻断创建，命中还必须精确核对 marker 和目标。这个策略通过允许保留未决投递来保护同一 journal 不重复创建，不承诺跨安装的分布式 exactly-once。不要删除未决记录来强制再次创建。

## 10.8 修复关联与回归证据

track 只读取远端 issue 并更新本地观察，关闭 issue、合并 PR 或出现新版本都不能自动视为修复。link-fix 输入 `reference` 和 `target_version` 后进入 `awaiting-validation`。原始 `reproduction.scenario` 与 `.expected` 分别记录不执行的场景文本及精确比较判据。

```bash
relay-knowledge feedback link-fix <feedback-id> --input fix.json --format json
relay-knowledge feedback validate <feedback-id> --input validation.json --format json
```

原场景必须由另行授权的 runner 重跑；`validation_runner` 与发布权限独立配置。验证输入包括匹配的 `runner`、status 中的 `scenario_digest`、`actual`、`version`、`environment`、唯一 `run_id` 和可选 `elapsed_ms`/`steps`。本地调用者声明这些证据确实来自该次运行；这不是外部 runner 的密码学身份认证，也不是 CLI 独立重跑。CLI 核对原场景/判据内容绑定与目标版本并计算精确相等，不接收输入 `passed`，不执行命令。授权证据匹配才进入 `verified-fixed`，不匹配为 `regressed`，无判据或无 runner 权限保持待验证。run ID 不可绑定不同证据，每条最多保留 32 次回归。基线 diagnostics 与候选 environment 应说明硬件、模型/索引、工作负载、耗时和步骤差异；没有固定输入/判据的检索体验先进行外部评测。

反馈发布权不授予修复、下载、升级、发 PR 或运行脚本权限。#416 批量地图示例只是摩擦反馈 fixture，本功能不实现批量地图事务。
