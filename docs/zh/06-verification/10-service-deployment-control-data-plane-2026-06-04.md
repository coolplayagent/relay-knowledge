# 服务化部署、控制面与数据面分离文档刷新审计 2026-06-04

[中文](../../zh/06-verification/10-service-deployment-control-data-plane-2026-06-04.md) | [English](../../en/06-verification/10-service-deployment-control-data-plane-2026-06-04.md)

> 文档版本: 1.2
> 编制日期: 2026-06-04
> 范围: GitHub issue #250、第三卷第 22 章、相关架构章节、常驻服务用户指南、README 和本次文档同步记录。

## 1. 刷新内容

- 新增第三卷第 22 章《服务化部署、控制面与数据面分离》。
- 将 issue #250 的服务化部署目标落成 `embedded_cli`、`resident_single_process`、`resident_partitioned_sqlite` 和未来 `split_worker_preview` 拓扑。
- 同步存储、统一 API、后台服务、安装升级、常驻服务和顶层 README，明确控制面 API、数据面 shard、split worker lease、备份/迁移/卸载边界。
- 后续实现补齐 `service status`/`health` 的 storage topology diagnostics、只读 `/api/v1/control/*` preview route、`service plan` runtime state path/warning、以及 `service worker run [--task-id <id>]` split-worker preview CLI。
- Codex review 后补强控制面实现：缺失 partitioned shard 时 `health` 返回降级诊断而不是 API 错误；`health` 的 storage topology 查询纳入同一个超时预算；`/api/v1/control/service/status` 使用只读状态路径，不触发 index refresh 入队。

## 2. 来源核验

本次竞争力分析覆盖图数据库、多模型数据库、向量数据库、事件流平台、工作流运行时和嵌入式/边缘存储。引用来源均为产品官方文档，主要包括 Neo4j、NebulaGraph、SurrealDB、Qdrant、Milvus、NATS JetStream、Kafka KRaft 和 Temporal。

来源用于判断架构方向，不作为 v1 外部依赖引入依据。v1 默认仍为 SQLite-first、本地零依赖、async-first、QoS、有界 worker、持久 task lease 和平台 service manager。

## 3. 目录一致性

- `docs/zh/README.md` 和 `docs/en/README.md` 增加第三卷第 22 章入口。
- 第 21 章导航增加第 22 章后继链接。
- `README.md` 和 `README.zh-CN.md` 增加服务化拓扑和控制面/数据面说明。
- `docs/zh/README.md` 和 `docs/en/README.md` 增加附录 B.10 入口。

## 4. 验证说明

建议验证命令：

```bash
rg -n "第 22 章|Chapter 22|22-service-deployment-control-data-plane|B.10" docs README.md README.zh-CN.md
rg -n "split_worker_preview|resident_partitioned_sqlite|控制面|data plane" docs/zh docs/en README.md README.zh-CN.md
rg -n "/api/v1/control|service worker run|runtime_state_paths|missing_shard_count" src docs/zh docs/en
cargo test --all-targets --all-features service_status_reports_partitioned_storage_diagnostics -- --nocapture
cargo test --all-targets --all-features control_service_status_does_not_queue_index_refresh_work -- --nocapture
wc -l docs/zh/03-architecture-specs/22-service-deployment-control-data-plane.md \
  docs/en/03-architecture-specs/22-service-deployment-control-data-plane.md \
  docs/zh/06-verification/10-service-deployment-control-data-plane-2026-06-04.md \
  docs/en/06-verification/10-service-deployment-control-data-plane-2026-06-04.md
cargo test --all-targets --all-features
```

Rust 实现变更必须通过 focused storage/service/Web/CLI 测试和全量 `cargo test --all-targets --all-features`。
