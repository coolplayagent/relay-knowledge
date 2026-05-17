# 派生索引与新鲜度

[中文](../../zh/03-architecture-specs/08-derived-indexes-and-freshness.md) | [English](../../en/03-architecture-specs/08-derived-indexes-and-freshness.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

派生索引的价值不只在召回速度，而在可解释的新鲜度。每个 read model 都必须能回答：它覆盖哪个 scope、哪个 graph version、哪个 backend、哪个 model/dimension、是否 stale、为何 degraded。

## 2. Index Family

| Index family | 用途 |
| --- | --- |
| `bm25` | 词法精确召回、别名、代码符号、chunk |
| `semantic` | 本地语义签名或外部 embedding 前语义摘要 |
| `vector` | 向量近邻、图像/文本 embedding metadata |
| `graph_path` | schema path、实体邻域、多跳候选 |
| `community` | 社区摘要、global context |
| `code` | 代码符号、引用、调用、导入、chunk FTS |
| `local_file_path` | 本机文件 normalized path、basename、目录 token、扩展名、路径 trigram/posting list |
| `local_file_metadata` | 文件 size、mtime、hash、mime、language、permission snapshot、symlink/hidden/system metadata |
| `local_file_content` | 文件文本 chunk、BM25/trigram、可选 semantic/vector metadata |
| `local_file_change_cursor` | Windows USN、macOS FSEvents、Linux inotify/fanotify 或 bounded rescan cursor |

## 3. 本机文件索引契约

本机文件检索不依赖 Everything、Spotlight、Windows Search、locate 或其他外部搜索工具。平台事件机制可以作为后续 watcher 后端，但 relay-knowledge 的派生索引必须能通过授权 root 的 bounded scan 和 persistent cursor 独立恢复。

文件名/路径索引、metadata 索引和内容索引分离：交互式文件定位不能等待 OCR、压缩包展开、大文件 hash、embedding 或全文内容抽取。所有文件查询先应用 source scope、授权 root、exclude/ignore、permission snapshot 和 freshness policy，再进入候选窗口。

`local_file_change_cursor` 必须记录 last event、overflow、missed event、scan watermark、last scan error 和 stale reason。平台事件丢失或游标失效时，查询返回 degraded/stale metadata 并触发有界重扫，不能静默报告 fresh。

## 4. Freshness 状态机

```text
missing -> stale -> refreshing -> fresh
                 -> degraded
                 -> failed -> dead_letter
```

`fresh` 只表示 index cursor 覆盖到目标 graph version；不表示事实正确。`degraded` 可以服务请求，但 context pack 必须说明缺失 family、backend 或 scope。

文件索引的 freshness 同时包含 graph version 和 file cursor watermark。文件内容索引落后时，路径索引仍可 fresh；响应必须说明哪一层 stale，而不是把整个本机文件 scope 视为不可用。

## 5. Refresh 调度

刷新任务由 mutation log 和 explicit refresh 请求产生。调度必须满足：

- queue bounded，不能无限增长。
- task 有 scope、family、target graph version 和 source hash。
- worker claim 使用 lease 和 owner。
- completion 必须匹配 active lease、attempt 和 target version。
- 任务完成期间若 graph version 又前进，cursor 保持 stale 并补发后续 work。

本机文件刷新任务还必须携带 root id、platform cursor、scan budget、max files per root 和 content extraction budget。内容索引、OCR、压缩包和大文件解析只能作为后台 worker work item，不能在查询热路径执行。

## 6. 查询策略

查询 freshness policy 至少包括：

- `allow-stale`：可返回 stale 结果，但 metadata 必须说明 lag。
- `wait-until-fresh`：等待必要索引推进到目标 version，超时返回稳定错误。
- `require-fresh`：发现 stale 直接失败，不隐式刷新。

## 7. 验收标准

- `health` 和 context pack 都能解释 index lag、missing family、dead-letter 和 last error。
- 显式 refresh 入队失败时返回可重试错误，不伪装成 fresh。
- startup reconciler 能从 mutation log 补发遗漏刷新任务。
- 本机文件名查询不依赖内容索引；文件查询输出说明 path、metadata、content 和 change cursor 的 freshness/degraded 状态。

---

导航: 上一章: [7. 存储引擎与 Mutation Log](07-storage-engine-and-mutation-log.md) | 下一章: [9. 混合检索与 Context Packing](09-hybrid-retrieval-and-context-packing.md)
