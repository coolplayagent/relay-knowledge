# 第 2 章 CLI 基础

## 2.1 命令结构

CLI 使用 git-style 子命令:

```bash
relay-knowledge [command] [command options] [--format text|json|streaming-json]
```

没有子命令时等同于 `status`。查看帮助:

```bash
relay-knowledge --help
relay-knowledge query -- --help
```

查询文本如果以 `-` 开头，使用 `--` 分隔:

```bash
relay-knowledge query -- "--help" --format json
```

## 2.2 输出格式

支持三种输出:

- `text`: 面向终端的短文本摘要，默认格式。
- `json`: 单行 JSON，适合脚本、测试和其它工具消费。
- `streaming-json`: 输出 started、item、completed 等事件，适合长操作和未来前端流式展示。

示例:

```bash
relay-knowledge status --format text
relay-knowledge status --format json
relay-knowledge status --format streaming-json
```

`version` 和 `--version` 只支持 `text` 和 `json`:

```bash
relay-knowledge version
relay-knowledge --version --format json
```

## 2.3 常用状态命令

项目状态:

```bash
relay-knowledge status --format json
```

健康检查:

```bash
relay-knowledge health --format json
```

服务诊断:

```bash
relay-knowledge service status --format json
relay-knowledge service doctor --format json
```

`service status` 和 `service doctor` 当前复用同一统一 API 输出，报告 service mode、后台更新状态、service definition path、agent protocol status 和 refresh queue diagnostics。

## 2.4 Freshness 策略

查询类命令可使用 `--freshness` 控制索引新鲜度:

- `allow-stale`: 允许返回旧索引结果，并在 metadata 中标记 stale 或 degraded。
- `wait-until-fresh`: 查询前尝试刷新落后的索引；无法满足时返回错误或 degraded 状态。
- `graph-only`: 绕过 BM25、semantic 和 vector 等派生索引，只读图事实路径。

普通知识检索默认使用当前实现的默认 freshness。代码仓库查询默认 `allow-stale`，需要严格读最新图状态时显式传入 `wait-until-fresh`。

## 2.5 命令总览

```bash
relay-knowledge status
relay-knowledge ingest --source <scope> --content <text> [--entity <label>]
relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness allow-stale|wait-until-fresh|graph-only]
relay-knowledge repo register <path> --alias <name> [--path <filter>] [--language <id>]
relay-knowledge repo index <alias> [--ref <ref>]
relay-knowledge repo update <alias> --base <ref> --head <ref>
relay-knowledge repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo status <alias>
relay-knowledge graph inspect
relay-knowledge index refresh [--kind bm25|semantic|vector]
relay-knowledge health
relay-knowledge service status
relay-knowledge service doctor
relay-knowledge service run [--web] [--mcp streamable-http]
relay-knowledge version
```
