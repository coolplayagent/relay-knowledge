# 第 2 章 CLI 基础

[中文](../../zh/01-user-guide/02-cli-basics.md) | [English](../../en/01-user-guide/02-cli-basics.md)

本章解释 CLI 的通用语法、输出、freshness 和错误诊断。完整命令清单见 [第 3 章 CLI 命令参考](03-cli-command-reference.md)。

## 2.1 命令结构

CLI 使用 git-style 子命令。全局 `--format` 和 `--remote <base-url>` 可以放在命令前后；命令参数仍按各子命令解析:

```bash
relay-knowledge [command] [command options] [--remote <base-url>] [--format text|json|markdown|streaming-json]
```

`--remote` 或 `RELAY_KNOWLEDGE_REMOTE_BASE_URL` 会让支持的代码仓库索引、scope preview、status 和 query 命令访问常驻服务 HTTP API，而不是打开本机 runtime storage。远端模式不会执行 `repo index-worker`，索引任务由远端 `service run --web` 的有界 worker pool 消费。

没有子命令时等同于 `status`。查看帮助:

```bash
relay-knowledge --help
relay-knowledge query --help
relay-knowledge repo query --help
relay-knowledge help repo query --format json
```

查询文本如果以 `-` 开头，使用 `--` 分隔:

```bash
relay-knowledge query -- "--help" --format json
```

## 2.2 自描述规格

面向脚本、skill 和 LLM 工具时，优先读取机器可消费的 CLI 规格:

```bash
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

JSON 规格包含 command path、operation、读写影响、参数语义、是否必填、默认值、允许取值、是否可重复、示例和注意事项。新增或修改 CLI 参数时，必须同步更新这份自描述规格。

CLI 输入会先按这份规格解析成内部语法树，再映射到运行时命令。解析失败时，自动化调用方应读取 diagnostic 中的 `matched_path`、`expected`、`usage` 和 `suggestion` 字段，而不是猜测参数含义。

## 2.3 输出格式

支持四种输出:

- `text`: 面向终端的短文本摘要，默认格式。
- `json`: 单行 JSON，适合脚本、测试和其它工具消费。
- `markdown`: 面向人工阅读的 Markdown，主要用于 `repo report` 和版本输出。
- `streaming-json`: 输出 `started`、`item`、`completed` 等事件，适合长操作和未来前端流式展示。

示例:

```bash
relay-knowledge status --format text
relay-knowledge status --format json
relay-knowledge repo report core --format markdown
relay-knowledge status --format streaming-json
```

`version` 和 `--version` 支持 `text` 和 `json`，不支持 `streaming-json`:

```bash
relay-knowledge version
relay-knowledge --version --format json
```

## 2.4 Freshness 策略

查询类命令可使用 `--freshness` 控制索引新鲜度:

- `allow-stale`: 允许返回旧索引结果，并在 metadata 中标记 stale 或 degraded。
- `wait-until-fresh`: 查询前尝试刷新落后的索引；无法满足时返回错误或 degraded 状态。
- `graph-only`: 绕过 BM25、semantic 和 vector 等派生索引，只读图事实路径。

普通知识检索默认使用当前实现的默认 freshness。代码仓库查询默认 `allow-stale`；需要严格读取最新图状态时，显式传入 `wait-until-fresh`。

## 2.5 参数边界

`--limit` 必须是正整数，并由各 API 层继续执行上限校验；`0` 会被 retrieval、repository、audit 和 proposal 请求校验拒绝。

`--kind` 在不同命令中含义不同:

- `index refresh`: `bm25`、`semantic`、`vector`。
- `worker`: `embedding`、`ocr`、`vision`、`extractor`。
- `repo query`: `hybrid`、`symbol`、`definition`、`references`、`callers`、`callees`、`imports`。

当查询文本或 reason 中包含以 `-` 开头的词时，使用 `--` 或引号避免被解析成选项。

## 2.6 语法诊断

CLI 解析错误会根据语法树返回最接近的上下文。文本模式下，错误输出到 stderr，并尽量包含 `Try:` 和 `Usage:`。

未知命令示例:

```bash
relay-knowledge repo qurey core --query rust
```

会提示未知命令 `repo qurey`，并建议 `repo query`。

位置参数错误示例:

```bash
relay-knowledge query --query SQLite
```

会提示 `query` 的查询文本是位置参数，并建议:

```bash
relay-knowledge query SQLite
```

当请求中包含 `--format json` 且解析失败时，stderr 输出单行 JSON diagnostic:

```json
{"error":"...","matched_path":["query"],"unexpected_token":"--query","expected":["<text>","--source","--limit","--freshness"],"suggestion":"relay-knowledge query SQLite","usage":"relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness <policy>]"}
```

该 JSON 只描述解析错误，不代表业务 API 响应；成功响应仍输出到 stdout。
