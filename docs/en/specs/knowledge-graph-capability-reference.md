# Knowledge Graph Capability Reference

[English](../../en/specs/knowledge-graph-capability-reference.md) | [中文](../../zh/specs/knowledge-graph-capability-reference.md)

This is the English documentation page for `specs/knowledge-graph-capability-reference.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> **文档版本**: 1.0
> **编制日期**: 2026-05-10
> **调研范围**: tirth8205/code-review-graph (v2.3.3) + n24q02m/better-code-review-graph (v3.16.0-beta.2)
> **数据来源**: GitHub 仓库源码、README、docs/、pyproject.toml、本地克隆代码

---

## 概述

### 项目定位

Code-Review-Graph 是一个面向 AI 编码助手的**本地代码知识图谱**。它通过 Tree-sitter 解析代码库并构建持久化结构图，经 MCP (Model Context Protocol) 协议将精准上下文暴露给 AI 助手，实现 token 高效的代码审查。

两个核心仓库：

| 仓库 | URL | Stars | 当前版本 | 定位 |
|------|-----|-------|---------|------|
| **code-review-graph** (原始) | [tirth8205/code-review-graph](https://github.com/tirth8205/code-review-graph) | ~16,000 | v2.3.3 | 面向 Claude Code 的全功能本地知识图谱 |
| **better-code-review-graph** (fork) | [n24q02m/better-code-review-graph](https://github.com/n24q02m/better-code-review-graph) | 43 | v3.16.0-beta.2 | 关键 bug 修复版，专注 MCP 服务器语境 |

### 核心价值主张

- **Token 效率**: 平均 8.2× token 缩减（naive 全量 vs graph 精准上下文）
- **增量更新**: 文件变更后 < 2 秒完成增量重建
- **零外部依赖**: SQLite 本地存储，无云依赖，无外部数据库
- **100% 召回率**: 影响分析绝不遗漏实际受影响的文件
- **多平台支持**: 自动检测 14+ AI 编码平台并配置 MCP

---

## 图谱数据模型

### 节点类型

代码图谱定义了 5 种节点类型，通过 Tree-sitter AST 提取：

```python
# 源码: code_review_graph/parser.py
class NodeInfo:
    kind: str           # File | Class | Function | Type | Test
    name: str           # 节点名称
    file_path: str      # 所在文件绝对路径
    line_start: int     # 定义起始行
    line_end: int       # 定义结束行
    language: str       # 源码语言
    parent_name: str | None    # 嵌套容器（类名或父类名）
    params: str | None         # 函数参数列表
    return_type: str | None    # 返回类型标注
    modifiers: str | None      # 访问修饰符
    is_test: bool              # 是否为测试
    extra: dict                # 扩展元数据
```

各节点类型详述：

| 节点类型 | 说明 | 识别方式 |
|----------|------|----------|
| **File** | 源代码文件 | 绝对路径；language 为 Tree-sitter 检测语言；file_hash 为 SHA-256 |
| **Class** | 类/结构体/接口/枚举/模块 | 语言特定的 class/struct/interface/enum 节点类型匹配 |
| **Function** | 函数/方法/构造器 | 语言特定的 function/method_declaration/constructor 节点匹配 |
| **Test** | 测试函数 | is_test=true；通过命名前缀/后缀+文件模式识别 |
| **Type** | 类型别名/接口 | 主要用于 TypeScript/Go/Rust 的 type alias/interface 声明 |

### Qualified Name 格式

节点唯一标识采用层级限定名：

```
文件节点:      /absolute/path/to/file.py
顶层函数:      /absolute/path/to/file.py::function_name
类方法:        /absolute/path/to/file.py::ClassName.method_name
嵌套类方法:    /absolute/path/to/file.py::OuterClass.InnerClass.method_name
```

### 边类型

```python
# 源码: code_review_graph/parser.py
class EdgeInfo:
    kind: str       # CALLS | IMPORTS_FROM | INHERITS | IMPLEMENTS | CONTAINS |
                    # TESTED_BY | DEPENDS_ON | REFERENCES | INJECTS | CONSUMES | PRODUCES
    source: str     # 源节点 qualified name
    target: str     # 目标节点 qualified name
    file_path: str  # 所在文件
    line: int       # 行号
    extra: dict     # 扩展元数据
```

| 边类型 | 方向 | 说明 |
|--------|------|------|
| **CALLS** | 调用者 → 被调用者 | 函数调用关系，最核心的边类型 |
| **IMPORTS_FROM** | 导入文件 → 被导入模块 | 文件级导入依赖 |
| **INHERITS** | 子类 → 父类 | 类继承关系 |
| **IMPLEMENTS** | 实现类 → 接口 | 接口实现 (Java/C#/TypeScript/Go) |
| **CONTAINS** | 容器 → 包含节点 | 结构化包含：文件含类，类含方法 |
| **TESTED_BY** | 被测函数 → 测试函数 | 测试覆盖关系 |
| **DEPENDS_ON** | 源 → 目标 | 通用依赖 |
| **REFERENCES** | 引用者 → 被引用者 | Python 回调引用 (v2.3.3) |
| **INJECTS** | 注入点 → 依赖 | Spring DI 注入 (v2.3.3，better fork) |
| **CONSUMES / PRODUCES** | 消费者/生产者 → Topic | Kafka 消息流 (v2.3.3，better fork) |

### 边置信度 (v2.3.3)

```sql
-- 源码: code_review_graph/graph.py
confidence REAL DEFAULT 1.0,
confidence_tier TEXT DEFAULT 'EXTRACTED'  -- EXTRACTED | INFERRED | AMBIGUOUS
```

三级评分体系：**EXTRACTED**（AST 直接提取，置信度最高）→ **INFERRED**（跨文件推导）→ **AMBIGUOUS**（无法确定具体目标）。

---

## 存储方案

### SQLite 数据库设计

图存储使用 SQLite（WAL 模式），文件路径：`.code-review-graph/graph.db`。核心表结构：

```sql
-- 节点表
CREATE TABLE nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,              -- File | Class | Function | Type | Test
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL UNIQUE,
    file_path TEXT NOT NULL,
    line_start INTEGER, line_end INTEGER,
    language TEXT,
    parent_name TEXT,
    params TEXT, return_type TEXT, modifiers TEXT,
    is_test INTEGER DEFAULT 0,
    file_hash TEXT,
    extra TEXT DEFAULT '{}',
    community_id INTEGER,           -- 关联社区 (migration v4)
    updated_at REAL NOT NULL
);

-- 边表
CREATE TABLE edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,             -- CALLS | IMPORTS_FROM | INHERITS | ...
    source_qualified TEXT NOT NULL,
    target_qualified TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line INTEGER DEFAULT 0,
    extra TEXT DEFAULT '{}',
    confidence REAL DEFAULT 1.0,
    confidence_tier TEXT DEFAULT 'EXTRACTED',
    updated_at REAL NOT NULL
);

-- 全文搜索虚拟表 (FTS5)
CREATE VIRTUAL TABLE nodes_fts USING fts5(
    name, qualified_name, file_path, signature,
    content='nodes', content_rowid='rowid',
    tokenize='porter unicode61'
);

-- 嵌入向量表 (独立 DB)
CREATE TABLE embeddings (
    node_id INTEGER,
    model TEXT,
    vector BLOB,
    hash TEXT
);
```

辅助表：`metadata`（键值对）、`flows`（执行流）、`flow_memberships`（流成员）、`communities`（社区检测结果）、`community_summaries`（预计算汇总）。

### better fork 的扩展 (v2.0)

better fork 增加了时间维度列，支持历史状态查询：

```sql
ALTER TABLE nodes ADD COLUMN valid_from_sha TEXT NOT NULL;
ALTER TABLE nodes ADD COLUMN valid_to_sha TEXT NULL;     -- NULL = 当前有效
ALTER TABLE nodes ADD COLUMN security_tags TEXT NULL;     -- JSON: "CWE-89:HIGH"

ALTER TABLE edges ADD COLUMN valid_from_sha TEXT NOT NULL;
ALTER TABLE edges ADD COLUMN valid_to_sha TEXT NULL;

-- qualified_name 唯一约束改为部分索引以支持时间超驰
CREATE UNIQUE INDEX idx_nodes_qname_current ON nodes(qualified_name)
    WHERE valid_to_sha IS NULL;
```

### 图数据库选型分析

两仓库均选择 **SQLite** 而非专用图数据库，理由：

1. **零外部依赖** — 无需安装/运维 Neo4j/ArangoDB 等服务
2. **本地单机场景** — 代码图通常 < 100K 节点，SQLite 完全胜任
3. **WAL 模式** — 提供足够的并发读写能力
4. **递归 CTE** — SQLite 3.8.3+ 支持 WITH RECURSIVE，替代图数据库遍历
5. **导出支持** — 内置 Cypher/GraphML 导出，可在需要时接入外部图分析工具

**局限**: SQLite 递归 CTE 在 10M+ 节点规模下可能成为瓶颈；不支持原生图算法（PageRank、社区检测需依赖 igraph）。

---

## 检索能力

### 三层检索架构

```
查询入口
   │
   ├── 1. 精确图查询 (query_graph_tool)
   │     模式: callers_of | callees_of | imports_of | inheritors_of | tests_for
   │     目标: 通过 qualified_name 精确匹配
   │
   ├── 2. 关键词搜索 (FTS5)
   │     实现: SQLite FTS5 + porter stemming
   │     索引: name, qualified_name, file_path, signature
   │     特点: AND 逻辑分词 (better fork)；大小写不敏感
   │
   └── 3. 语义向量搜索 (optional)
          实现: 向量嵌入 + 余弦相似度
          嵌入对象: 函数签名文本 (~10 tokens)
          存储: 独立 SQLite embeddings 表
```

### 嵌入提供商矩阵

```python
# 源码: code_review_graph/embeddings.py
class EmbeddingProvider(ABC):
    def embed(self, texts: list[str]) -> list[list[float]]: ...
    def embed_query(self, text: str) -> list[float]: ...
```

| 提供商 | 安装方式 | 模型 | 体积 | 网络 |
|--------|----------|------|------|------|
| sentence-transformers (本地) | `pip install code-review-graph[embeddings]` | all-MiniLM-L6-v2 (384d) | ~1.1 GB | 离线 |
| Google Gemini (云端) | `pip install code-review-graph[google-embeddings]` | gemini-embedding-001 | - | 需 GOOGLE_API_KEY |
| MiniMax (云端) | 内置 | embo-01 (1536d) | - | 需 MINIMAX_API_KEY |
| OpenAI 兼容 (通用) | 内置 | text-embedding-3-small 等 | - | 需 CRG_OPENAI_* 环境变量 |
| qwen3-embed ONNX (better fork) | 内置 | Qwen3-Embedding | ~200 MB | 离线/云双模 |

### 检索质量现状

| 指标 | 原始仓库 | better fork |
|------|----------|-------------|
| MRR (平均互逆排名) | 0.35 | 改进中 |
| 多词搜索 | 直接子串 | AND 逻辑分词 |
| callers_of 精确度 | 裸名查询可能返回空 | 限定名解析 + 裸名回退 |
| 输出控制 | 无界 (max 500K+) | 分页 (max_results + truncated) |

**已知局限**: 嵌入仅覆盖函数签名（~10 tokens/节点），未利用函数体/docstring；Express 风格模块模式命名导致部分查询零结果。

---

## 图构建与分析

### 构建管线

```
┌──────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ 文件收集       │ → │ Tree-sitter 解析  │ → │ SQLite 持久化     │
│ git ls-files  │    │ 24种语言 AST      │    │ nodes + edges    │
│ + ignore 过滤  │    │ 节点+边提取       │    │ + FTS5 索引      │
└──────────────┘    └──────────────────┘    └─────────────────┘
                                                     │
                                                     ▼
                                            ┌─────────────────┐
                                            │ 后处理            │
                                            │ · 社区检测 (Leiden)│
                                            │ · 执行流追踪       │
                                            │ · 嵌入计算 (可选)  │
                                            └─────────────────┘
```

### 增量更新策略

```
git commit / file save
        │
        ▼
┌───────────────────┐
│ git diff 变更检测   │ → 找出所有变更文件
└───────┬───────────┘
        ▼
┌───────────────────┐
│ find_dependents()  │ → 通过 IMPORTS_FROM 边找出所有依赖文件
└───────┬───────────┘
        ▼
┌───────────────────┐
│ SHA-256 哈希比对   │ → 跳过哈希未变的文件（大部分）
└───────┬───────────┘
        ▼
┌───────────────────┐
│ 仅重解析变更文件    │ → 2,900 文件项目 < 2 秒
└───────────────────┘
```

### 影响分析 (Blast Radius)

SQLite 递归 CTE 实现 BFS 遍历（v2.2.1 起替代 NetworkX，3-10× 加速）：

1. 种子 = 变更文件中所有 qualified_name
2. 前向遍历：追踪 CALLS / IMPORTS_FROM 边（"谁受此变更影响"）
3. 反向遍历：追踪反向边（"此变更依赖谁"）
4. 可配置上限：`CRG_MAX_IMPACT_DEPTH=2`、`CRG_MAX_IMPACT_NODES=500`

**基准数据**（6 个真实仓库，13 次 commit）：

| 仓库 | 文件数 | 节点数 | 边数 |
|------|--------|--------|------|
| express | 141 | 1,910 | 17,553 |
| fastapi | 1,122 | 6,285 | 27,117 |
| flask | 83 | 1,446 | 7,974 |
| gin | 99 | 1,286 | 16,762 |
| httpx | 60 | 1,253 | 7,896 |

### 社区检测与架构分析

- **Leiden 算法** (igraph)：代码社区自动聚类
- **超大社区递归分裂**：> 25% 图的社区自动拆分
- **Hub 节点检测**：度中心性最高的节点
- **Bridge 节点检测**：介数中心性（架构瓶颈点）
- **知识缺口分析**：孤立节点、未测试热点、薄社区
- **惊喜评分**：跨社区/跨语言/外围到枢纽的意外耦合

### 解析语言覆盖

24 种语言 + Jupyter/Databricks 笔记本：

| 类别 | 语言 |
|------|------|
| Web 前端 | TypeScript/TSX, JavaScript, Vue SFC, Svelte |
| 后端 | Python, Go, Java, PHP, Ruby, Kotlin, C# |
| 系统 | Rust, C/C++, Zig |
| 移动 | Swift, Dart, Kotlin |
| 脚本/配置 | Perl, Lua, PowerShell, Julia, R, Nix |
| 智能合约 | Solidity |
| 数据科学 | Jupyter/Databricks (.ipynb)，Python/R/SQL 多语言 Cell |
| 其他 | Scala, ReScript, GDScript, Verilog/SystemVerilog, SQL |

框架感知调用解析：Spring DI (INJECTS)、Temporal、Kafka (CONSUMES/PRODUCES)、Jedi (Python)、Mocha TDD、Bun test。

---

## API 与集成

### MCP 工具集 (原始仓库, 28 个工具)

**核心查询工具：**

| 工具 | 功能 |
|------|------|
| `build_or_update_graph_tool` | 图全量构建 / 增量更新 |
| `get_minimal_context_tool` | 超紧凑上下文 (~100 tokens)，入口工具 |
| `get_impact_radius_tool` | 变更影响范围分析 |
| `get_review_context_tool` | Token 优化的审查上下文 |
| `query_graph_tool` | 模式化图查询 (8 种模式) |
| `traverse_graph_tool` | BFS/DFS 自定义遍历，token 预算控制 |
| `semantic_search_nodes_tool` | 语义/关键词搜索 |
| `embed_graph_tool` | 计算向量嵌入 |
| `list_graph_stats_tool` | 图健康状态 |
| `find_large_functions_tool` | 按行数阈值查找大函数 |

**流/社区/架构分析工具：** `list_flows_tool`, `get_flow_tool`, `get_affected_flows_tool`, `list_communities_tool`, `get_community_tool`, `get_architecture_overview_tool`

**变更分析工具：** `detect_changes_tool`（风险评分）, `get_hub_nodes_tool`, `get_bridge_nodes_tool`, `get_knowledge_gaps_tool`, `get_surprising_connections_tool`, `get_suggested_questions_tool`

**重构工具：** `refactor_tool`（重命名预览/死代码/建议）, `apply_refactor_tool`

**Wiki 与多仓库：** `generate_wiki_tool`, `get_wiki_page_tool`, `list_repos_tool`, `cross_repo_search_tool`

### MCP 工作流模板 (5 个 Prompts)

```python
# 源码: code_review_graph/prompts.py
prompts = [
    "review_changes",     # 预提交审查流程
    "architecture_map",   # 架构文档 + Mermaid 图
    "debug_issue",        # 引导式调试
    "onboard_developer",  # 新开发者上手引导
    "pre_merge_check",    # PR 准备检查
]
```

### better fork 的领域驱动重构 (6 工具)

better fork 将 28 个独立工具重构为 6 个领域工具，采用 action 参数驱动：

| 工具 | Actions | 设计思路 |
|------|---------|----------|
| **graph** | build, update, stats, embed, export, summarize | 图生命周期管理 |
| **query** | query, search, impact, large_functions | 只读查询，不影响图状态 |
| **review** | (单一 action) | 独立审查上下文，不混入其他职责 |
| **config** | status, set, cache_clear | 运行时配置与缓存管理 |
| **setup** | status, start, skip, reset, complete | 凭证设置状态机 |
| **help** | graph, query, review, config | 自文档化 |

### CLI 命令

```bash
# 安装与配置 (14+ AI 编码平台自动检测)
code-review-graph install [--platform <name>] [--dry-run]

# 构建与更新
code-review-graph build              # 全量构建
code-review-graph update             # 增量更新
code-review-graph watch              # 文件监视自动更新

# 可视化与导出
code-review-graph visualize          # D3.js 交互式 HTML
code-review-graph visualize --format graphml|csv|obsidian|cypher

# 变更分析
code-review-graph detect-changes [--base HEAD~3] [--brief]

# 多仓库与守护进程
code-review-graph register <path> --alias <name>
code-review-graph daemon start|stop|status|logs
crg-daemon start|stop|restart|status|logs|add|remove

# MCP 服务
code-review-graph serve [--tools <filter>]
code-review-graph serve --http   # HTTP 传输模式
```

### 平台集成

`code-review-graph install` 自动检测并配置以下平台：

Claude Code | Codex | Cursor | Windsurf | Zed | Continue | OpenCode | Antigravity | Gemini CLI | Qwen | Qoder | Kiro | GitHub Copilot (VS Code) | GitHub Copilot CLI

每种平台的 MCP 配置自动生成，指令文件自动注入。

### 关键依赖

```toml
# pyproject.toml
dependencies = [
    "mcp>=1.0.0,<2",                          # MCP 协议
    "fastmcp>=3.2.4",                          # FastMCP 服务框架
    "tree-sitter>=0.23.0,<1",                  # 增量解析引擎
    "tree-sitter-language-pack>=0.3.0,<1",     # 语言语法包
    "networkx>=3.2,<4",                        # 图算法 (已逐步被 CTE 替代)
    "watchdog>=4.0.0,<6",                      # 文件系统监视
]

[project.optional-dependencies]
embeddings = ["sentence-transformers>=3.0.0,<4", "numpy>=1.26,<3"]
communities = ["igraph>=0.11.0"]
eval = ["matplotlib>=3.7.0", "pyyaml>=6.0"]
wiki = ["ollama>=0.1.0"]
enrichment = ["jedi>=0.19.2"]                  # Python 调用解析增强
```

better fork 额外依赖：

```toml
dependencies = [
    "qwen3-embed>=1.9.2",    # ONNX 嵌入引擎
    "cohere>=6.1.0",         # Cohere 云嵌入
    "google-genai>=2.0.1",   # Google 云嵌入
    "openai>=2.34.0",        # OpenAI 云嵌入
    "alembic>=1.14.0,<2",    # 数据库迁移
    "n24q02m-mcp-core>=1.14.0",
]
```

---

## 对 relay-knowledge 的参考价值

### 可直接借鉴的设计决策

1. **SQLite + 递归 CTE 作为图存储**
   - 中规模代码图 (< 100K 节点) 无需专用图数据库
   - 部署复杂度极低，适合本地优先场景
   - relay-knowledge 可在初期选择相同路径，预留切换到专用图数据库的接口

2. **Tree-sitter 多语言 AST 解析**
   - 24 种语言覆盖证明了增量解析引擎的成熟度
   - 语言特定的节点类型映射表模式清晰可扩展
   - relay-knowledge 可直接复用 tree-sitter + tree-sitter-language-pack

3. **SHA-256 哈希驱动的增量更新**
   - "解析前哈希对比"比"解析后 diff"效率高得多
   - git diff → dependent 发现 → 重解析，三级级联更新模式可靠

4. **MCP 协议作为 API 层**
   - MCP 已成为 AI 工具的标准集成协议
   - relay-knowledge 应从一开始就规划 MCP server 入口
   - 同时预留独立 Web API 层（当前两仓库均缺失）

5. **Qualified Name 作为图谱统一标识**
   - `文件路径::类.方法` 的命名空间格式简洁有效
   - 支持精确图查询和去重

### relay-knowledge 当前对齐状态

截至 2026-05-13，`relay-knowledge` 的代码知识图谱 v1 已完成以下对齐:

- **Tree-sitter 代码仓库索引**: 支持 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash；Unsupported、二进制、超大或解析失败文件降级为 text chunk/diagnostic。
- **SQLite 本地图谱读写**: code repository 文件、symbol、reference、call、import、chunk、diagnostic 和 rename tombstone 通过 storage trait 写入 SQLite，并同步写入 BM25、local semantic 和 local vector read model。
- **符号身份**: `symbol_snapshot_id` 表示某一 Git snapshot 中的符号实例，`canonical_symbol_id` 表示 `repo://{repository_id}/{qualified_name}` 形式的逻辑符号身份；类方法使用 `路径::Class.method` 层级限定名。
- **边解析与置信度**: reference、call 和 import 均持久化 `target_hint`、`resolution_state`、`confidence_basis_points` 和 `confidence_tier`。无法唯一解析时保持 `ambiguous` 或 `unresolved`，不会被报告为确定调用。
- **接口暴露**: CLI `repo query` / `repo impact` / `repo report`、Web operation 和 MCP `relay.code_query` / `relay.code_impact` 共用 application service，查询命中会返回 symbol identity 与 edge metadata。

仍明确属于后续 v2 或更高阶段的能力: 社区检测、wiki/export、真实外部 embedding 索引刷新、多仓库联邦调用解析、reranker、watch/daemon 静默更新和完整时间旅行查询。

### 可超越的关键差距

| 差距 | 当前状态 | relay-knowledge 可改进方向 |
|------|----------|--------------------------|
| **事件驱动** | 两个仓库均非事件驱动 (git hook 轮询) | 从零构建事件驱动+异步优先架构 (AGENTS.md 已要求) |
| **嵌入质量** | 仅嵌入函数签名 (10 tokens) | 嵌入函数体/docstring/commit message/Issue 讨论 |
| **检索精度** | FTS5 MRR 0.35 | 引入 re-ranker + hybrid search + 查询改写 |
| **图数据库** | SQLite 递归 CTE, 无原生图算法 | 预留 Neo4j/SurrealDB 适配层, 支持 PageRank 等原生算法 |
| **Web API** | 仅有 MCP (stdio/HTTP) | 构建完整的 REST/gRPC Web API |
| **知识类型** | 仅代码结构 | 扩展到文档、Issue、PR 讨论、ADR、运行时拓扑 |
| **时间维度** | better fork 有基础时间列 | 构建完整的时间旅行查询 + 差分分析 |
| **多仓库联邦** | 基础 TOML 注册表 | 跨仓库调用解析、统一图谱查询、Blast Radius 跨仓库分析 |
