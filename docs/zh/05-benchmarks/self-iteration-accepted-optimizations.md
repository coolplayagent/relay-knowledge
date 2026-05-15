# 自迭代采纳优化记录

本文档由自迭代 harness 在候选通过质量门禁并被采纳时追加，用于把本轮采用的优化思路传递给后续 Codex 迭代。人工维护的总结可以继续补充在对应条目下。

## 记录格式

- `patch`: 本轮候选补丁在 `.git/relay-knowledge-self-iteration/patches/` 下的路径。
- `score`: 采纳时的总分和 accuracy、performance、stability 分项。
- `cases`: 采纳时通过的检索 case 数量。
- `changed paths`: 本轮变更的主要文件。
- `key improvements`: 相对上一轮改善的 case、gate 或 metric。
- `known degradations`: 相对上一轮已观测到的退化，后续迭代必须优先保护或修复。
- `Adopted optimization notes`: Codex 输出中提取的优化说明，用作下一轮 prompt 的上下文。
