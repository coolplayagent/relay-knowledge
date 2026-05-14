# 第 2 章 CLI 基础

## 2.1 命令结构

CLI 使用 git-style 子命令。全局 `--format` 可以放在命令前后；命令参数仍按各子命令解析:

```bash
relay-knowledge [command] [command options] [--format text|json|markdown|streaming-json]
```

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

面向 skill、脚本和 LLM 工具时，优先读取机器可消费的 CLI 规格:

```bash
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

该 JSON 规格包含 command path、operation、读写影响、参数语义、是否必填、默认值、允许取值、是否可重复、示例和注意事项。后续新增或修改 CLI 参数时，必须同步更新这份自描述规格。

CLI 输入会先按这份自描述规格解析成内部语法树，再映射到运行时命令。这个语法树是 CLI 自举入口: 人类可以读文本 help，脚本、skill 和 LLM 工具应读取 `help --format json`，解析失败时也应优先使用结构化 diagnostic 中的 `matched_path`、`expected`、`usage` 和 `suggestion` 字段，而不是猜测参数含义。

## 2.2 输出格式

支持四种输出:

- `text`: 面向终端的短文本摘要，默认格式。
- `json`: 单行 JSON，适合脚本、测试和其它工具消费。
- `markdown`: 面向人工阅读的 Markdown；目前主要用于 `repo report` 和版本输出，其它命令会按通用响应渲染能力处理。
- `streaming-json`: 输出 started、item、completed 等事件，适合长操作和未来前端流式展示。

示例:

```bash
relay-knowledge status --format text
relay-knowledge status --format json
relay-knowledge repo report core --format markdown
relay-knowledge status --format streaming-json
```

`version` 和 `--version` 按 CLI 帮助公开支持 `text` 和 `json`，不支持 `streaming-json`:

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

Provider 诊断:

```bash
relay-knowledge provider probe --format json
```

`provider probe` 读取环境边界解析出的 remote embedding provider 配置，并执行一次轻量探测。JSON 响应包含 `ok`、`provider`、`model`、`dimension`、可选 `latency_ms`，失败时还包含 `error_code`、`error_message` 和 `retryable`。它不会输出 API key 原文，也不会绕过 `env` 模块直接读取环境变量。endpoint host、batch、timeout、并发和 cursor metadata 属于 `status`、`health` 或 Web Providers 面板的运行时诊断。

## 2.4 Freshness 策略

查询类命令可使用 `--freshness` 控制索引新鲜度:

- `allow-stale`: 允许返回旧索引结果，并在 metadata 中标记 stale 或 degraded。
- `wait-until-fresh`: 查询前尝试刷新落后的索引；无法满足时返回错误或 degraded 状态。
- `graph-only`: 绕过 BM25、semantic 和 vector 等派生索引，只读图事实路径。

普通知识检索默认使用当前实现的默认 freshness。代码仓库查询默认 `allow-stale`，需要严格读最新图状态时显式传入 `wait-until-fresh`。

## 2.5 命令总览

```bash
relay-knowledge status
relay-knowledge help [command...] [--format text|json]
relay-knowledge ingest --source <scope> --content <text> [--entity <label>]
relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness allow-stale|wait-until-fresh|graph-only]
relay-knowledge repo register <path> --alias <name> [--path <filter>] [--language <id>]
relay-knowledge repo index <alias> [--ref <ref>] [--dry-run]
relay-knowledge repo scope preview <alias> [--ref <ref>]
relay-knowledge repo update <alias> --base <ref> --head <ref>
relay-knowledge repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo report <alias> [--format markdown|json]
relay-knowledge repo status <alias>
relay-knowledge graph inspect
relay-knowledge index refresh [--kind bm25|semantic|vector]
relay-knowledge worker status|run-once [--kind embedding|ocr|vision|extractor]
relay-knowledge proposal list [--state proposed|accepted|rejected|superseded] [--limit <n>]
relay-knowledge proposal show <proposal-id>
relay-knowledge proposal accept|reject|supersede <proposal-id> --by <actor> [--reason <text>]
relay-knowledge audit query [--operation <name>] [--limit <n>]
relay-knowledge provider probe
relay-knowledge health
relay-knowledge service status
relay-knowledge service doctor
relay-knowledge service plan install|uninstall
relay-knowledge service definition write
relay-knowledge service operator status|pause|resume
relay-knowledge service run [--web] [--mcp streamable-http]
relay-knowledge version
```

## 2.6 参数边界

`--limit` 必须是正整数并由各 API 层继续执行上限校验；`0` 会被 retrieval、repository 和 audit/proposal 等请求校验拒绝。`--kind` 在不同命令中含义不同: `index refresh` 只接受 `bm25`、`semantic`、`vector`；`worker` 只接受 `embedding`、`ocr`、`vision`、`extractor`；`repo query` 只接受 `hybrid`、`symbol`、`definition`、`references`、`callers`、`callees`、`imports`。当查询文本或 reason 中包含以 `-` 开头的词时，使用 `--` 或引号避免被解析成选项。

参数解释是 CLI contract 的一部分。新增命令或参数必须在 `relay-knowledge help --format json` 中说明语义、默认值、取值范围、是否必填、是否可重复、读写影响、失败模式和示例，避免 skill 或 LLM 只能凭参数名猜测行为。

## 2.7 语法诊断

CLI 解析错误会根据语法树返回最接近的上下文。文本模式下，错误输出到 stderr，并尽量包含 `Try:` 和 `Usage:`:

```bash
relay-knowledge repo qurey core --query rust
```

会提示未知命令 `repo qurey`，并建议 `repo query`。

```bash
relay-knowledge query --query SQLite
```

会提示 `query` 的查询文本是位置参数，并建议:

```bash
relay-knowledge query SQLite
```

当请求中包含 `--format json` 且解析失败时，stderr 输出单行 JSON diagnostic，包含:

```json
{"error":"...","matched_path":["query"],"unexpected_token":"--query","expected":["<text>","--source","--limit","--freshness"],"suggestion":"relay-knowledge query SQLite","usage":"relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness <policy>]"}
```

该 JSON 只描述解析错误，不代表业务 API 响应；成功响应仍输出到 stdout。
