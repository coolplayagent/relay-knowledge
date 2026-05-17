# 多模态证据摄取

[中文](../../zh/03-architecture-specs/05-multimodal-evidence-ingestion.md) | [English](../../en/03-architecture-specs/05-multimodal-evidence-ingestion.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

摄取层的目标不是把所有内容变成文本，而是把不同模态转成同一套 evidence contract。文本、图片、OCR、caption、表格、布局区域和代码片段都应保留来源、父子关系、置信度和提取状态。

## 2. Evidence 统一模型

Evidence 至少表达：source scope、source path、span 或 asset region、modality、content hash、parent evidence、extraction method、confidence、lifecycle status 和 created graph version。

派生 evidence 必须引用父 evidence：

```text
image asset
  -> OCR text evidence
  -> caption evidence
  -> layout/table region evidence
  -> image embedding metadata
```

检索时按父 evidence 聚合派生命中，避免把同一图片的 OCR、caption 和 embedding 当成三份重复上下文。

## 3. 摄取流水线

```text
source discovery
  -> scope normalization
  -> evidence write
  -> extraction task enqueue
  -> worker extraction
  -> proposal or derived evidence commit
  -> mutation log
  -> index refresh request
```

摄取只负责创建原始 evidence 和有界后台任务；OCR、caption、embedding、表格抽取和大型解析不得在查询热路径运行。

## 4. Worker 边界

Worker task 必须有 kind、scope、input evidence id、attempt、lease、timeout、budget、redacted config snapshot 和 output contract。外部模型或 OCR service 失败时，任务进入 retry/dead-letter，不允许写入半结构化事实绕过验证。

## 5. 去重与版本

- content hash 用于识别重复 source payload。
- extraction output hash 用于避免重复提交派生 evidence。
- evidence lifecycle 支持 proposed、accepted、rejected、superseded。
- 新提取结果不能覆盖旧 evidence，只能追加新版本或建立 supersedes 关系。

## 6. 验收标准

- 任一派生 evidence 都能追溯到原始 source 和 worker attempt。
- 查询结果能按父 evidence 聚合多模态命中。
- 失败的外部提取不会阻塞已有文本或图检索。

---

导航: 上一章: [4. Source Scope 模型](04-source-scope-model.md) | 下一章: [6. 图事实模型与版本化](06-graph-fact-model-and-versioning.md)
