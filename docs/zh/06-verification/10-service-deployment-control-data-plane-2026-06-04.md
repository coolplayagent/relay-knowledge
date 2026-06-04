# 服务化部署、控制面与数据面分离文档刷新审计 2026-06-04

[中文](../../zh/06-verification/10-service-deployment-control-data-plane-2026-06-04.md) | [English](../../en/06-verification/10-service-deployment-control-data-plane-2026-06-04.md)

> 文档版本: 1.0
> 编制日期: 2026-06-04
> 范围: GitHub issue #250、第三卷第 22 章、相关架构章节、常驻服务用户指南、README 和本次文档同步记录。

## 1. 刷新内容

- 新增第三卷第 22 章《服务化部署、控制面与数据面分离》。
- 将 issue #250 的服务化部署目标落成 `embedded_cli`、`resident_single_process`、`resident_partitioned_sqlite` 和未来 `split_worker_preview` 拓扑。
- 同步存储、统一 API、后台服务、安装升级、常驻服务和顶层 README，明确控制面 API、数据面 shard、split worker lease、备份/迁移/卸载边界。
- 本次变更只修改文档，不改变 Rust API、CLI 行为、配置、运行时、测试夹具或发布流程。

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
wc -l docs/zh/03-architecture-specs/22-service-deployment-control-data-plane.md \
  docs/en/03-architecture-specs/22-service-deployment-control-data-plane.md \
  docs/zh/06-verification/10-service-deployment-control-data-plane-2026-06-04.md \
  docs/en/06-verification/10-service-deployment-control-data-plane-2026-06-04.md
```

`cargo test` 不属于本次文档刷新必要验证项，因为没有代码、配置或测试行为变化。
