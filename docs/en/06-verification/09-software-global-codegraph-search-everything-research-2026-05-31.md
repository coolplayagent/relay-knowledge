# Software Global, CodeGraph, and Search Everything Research Documentation Refresh Audit 2026-05-31

[English](../../en/06-verification/09-software-global-codegraph-search-everything-research-2026-05-31.md) | [中文](../../zh/06-verification/09-software-global-codegraph-search-everything-research-2026-05-31.md)

> Document version: 1.0
> Prepared: 2026-05-31
> Scope: Chapter 11 research documentation, research volume indexes, bookshelf indexes, and this documentation sync record.

## 1. Refreshed Content

- Added Chapter 11, "Software Global Modeling, CodeGraph, and Search Everything Comparison 2026."
- Synchronized the Chinese and English research volume indexes and added the Chapter 11 entry to the bookshelf indexes.
- Updated Chapter 10 navigation so readers can continue to Chapter 11.
- This change modifies documentation only; it does not change Rust APIs, CLI behavior, configuration, runtime behavior, test fixtures, or release flow.

## 2. Source Verification

This research used 2026 papers, open-source projects, and systems-engineering references as primary material, covering Codebase-Memory, RepoDoc, RAGdeterm, KCoEvo, KG-HiAttention, RPG, SemanticForge, Tree-sitter, Zoekt, Everything, plocate, and ripgrep.

Source selection rules:

- Papers inform the repository-KG, deterministic Code RAG, documentation lifecycle, code-evolution, and vulnerability-explanation routes.
- Open-source projects inform MCP-native CodeGraph, Tree-sitter extraction, FTS/BM25, SQLite/embedded storage, and agent-tool interface trends.
- Systems-engineering references inform file-path indexing, trigram/regex search, change cursors, candidate windows, and hybrid ranking.

## 3. Index Consistency

- `docs/zh/04-research/README.md` and `docs/en/04-research/README.md` add the Chapter 11 guide.
- `docs/zh/README.md` and `docs/en/README.md` add the Book 4 Chapter 11 entry.
- `docs/zh/README.md` and `docs/en/README.md` add the Appendix B.9 entry.

## 4. Verification Notes

Recommended verification commands:

```bash
wc -l docs/zh/04-research/11-software-global-codegraph-search-everything-comparison-2026.md \
  docs/en/04-research/11-software-global-codegraph-search-everything-comparison-2026.md \
  docs/zh/06-verification/09-software-global-codegraph-search-everything-research-2026-05-31.md \
  docs/en/06-verification/09-software-global-codegraph-search-everything-research-2026-05-31.md
rg -n "第 11 章|Chapter 11|11-software-global-codegraph-search-everything-comparison-2026|B.9" docs/zh docs/en
rg -n "T(O)DO|待[[:space:]]*补|TB[D]" docs/zh/04-research docs/en/04-research docs/zh/06-verification docs/en/06-verification
```

`cargo test` is not required for this documentation refresh because no code, configuration, or test behavior changed.
