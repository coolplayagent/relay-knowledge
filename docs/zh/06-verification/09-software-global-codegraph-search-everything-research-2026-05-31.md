# 软件全域、CodeGraph 与 Search Everything 研究文档刷新审计 2026-05-31

[中文](../../zh/06-verification/09-software-global-codegraph-search-everything-research-2026-05-31.md) | [English](../../en/06-verification/09-software-global-codegraph-search-everything-research-2026-05-31.md)

> 文档版本: 1.0
> 编制日期: 2026-05-31
> 范围: 第 11 章研究文档、研究卷目录、书籍式总目录和本次文档同步记录。

## 1. 刷新内容

- 新增第 11 章《软件全域建模、CodeGraph 与 Search Everything 对比研究 2026》。
- 同步中文与英文研究卷目录，并在书籍式总目录中加入第 11 章入口。
- 将第 10 章导航补充为可继续阅读第 11 章。
- 本次变更只修改文档，不改变 Rust API、CLI、配置、运行时、测试夹具或发布流程。

## 2. 来源核验

本次研究使用 2026 年论文、开源项目和系统工程资料作为主要来源，覆盖 Codebase-Memory、RepoDoc、RAGdeterm、KCoEvo、KG-HiAttention、RPG、SemanticForge、Tree-sitter、Zoekt、Everything、plocate 和 ripgrep 等材料。

来源筛选规则：

- 论文用于判断 repository KG、确定性 Code RAG、文档生命周期、代码演化和漏洞解释路线。
- 开源项目用于判断 MCP 原生 CodeGraph、Tree-sitter 抽取、FTS/BM25、SQLite/嵌入式存储和 Agent 工具接口趋势。
- 系统工程资料用于判断文件路径索引、trigram/regex 搜索、变更游标、候选窗口和混合排序机制。

## 3. 目录一致性

- `docs/zh/04-research/README.md` 和 `docs/en/04-research/README.md` 增加第 11 章导读。
- `docs/zh/README.md` 和 `docs/en/README.md` 增加第四卷第 11 章入口。
- `docs/zh/README.md` 和 `docs/en/README.md` 增加附录 B.9 入口。

## 4. 验证说明

建议验证命令：

```bash
wc -l docs/zh/04-research/11-software-global-codegraph-search-everything-comparison-2026.md \
  docs/en/04-research/11-software-global-codegraph-search-everything-comparison-2026.md \
  docs/zh/06-verification/09-software-global-codegraph-search-everything-research-2026-05-31.md \
  docs/en/06-verification/09-software-global-codegraph-search-everything-research-2026-05-31.md
rg -n "第 11 章|Chapter 11|11-software-global-codegraph-search-everything-comparison-2026|B.9" docs/zh docs/en
rg -n "T(O)DO|待[[:space:]]*补|TB[D]" docs/zh/04-research docs/en/04-research docs/zh/06-verification docs/en/06-verification
```

`cargo test` 不属于本次文档刷新必要验证项，因为没有代码、配置或测试行为变化。
