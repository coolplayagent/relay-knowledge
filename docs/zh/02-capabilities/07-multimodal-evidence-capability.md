# 多模态证据能力

[中文](./07-multimodal-evidence-capability.md) | [English](../../en/02-capabilities/07-multimodal-evidence-capability.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

多模态能力让 evidence 不局限于纯文本。系统可记录 text span、image asset、OCR text、caption、image embedding、table 和 layout region 的抽取元数据。

## 用户可见行为

- 派生 OCR、caption 和 image embedding 会按 parent evidence 合并为一个 context item。
- 后台或 maintenance worker 通过 `commit_multimodal_extraction` 提交 OCR、caption、table、layout 和 image embedding 输出。
- 查询热路径不运行 OCR、caption、embedding 或大型抽取。

## 竞争力特性

普通 RAG 常把 OCR 文本当普通 chunk，丢失图片、表格和布局的来源关系。这里保留 parent evidence、modality、extractor、confidence 和 derived metadata，使上下文既能检索，也能解释来自哪种模态。

## 命令/API 入口

```bash
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
```

## 降级与诊断

外部 OCR、视觉或 embedding provider 不可用时，文本 evidence、BM25、图路径和已有派生 evidence 仍可检索。Worker failure 进入 retry 或 dead-letter，不阻塞查询。

## 关联架构章节

- [多模态证据摄取](../03-architecture-specs/05-multimodal-evidence-ingestion.md)
- [Semantic/Vector Provider 架构](../03-architecture-specs/10-semantic-vector-provider-architecture.md)

---

导航: 上一章: [6. 新鲜度与索引恢复](06-freshness-and-index-recovery.md) | 下一章: [8. 代码仓库基础能力](08-code-repository-basics.md)
