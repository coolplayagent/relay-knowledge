# Code Repository Basics

[English](./08-code-repository-basics.md) | [中文](../../zh/02-capabilities/08-code-repository-basics.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Code repository basics let users register Git repositories as first-class sources, index clean snapshots, and query code context through the same application service.

## User-visible Behavior

```bash
relay-knowledge repo register /path/to/repo --alias core --path src --language rust
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo status core --format json
```

`repo query` supports `--limit`, `--ref`, repeatable `--path`, repeatable `--language`, and freshness policy.

## Competitive Features

Repository indexing binds repository id, resolved commit, tree hash, path filters, and language filters. Equal trees can reuse scopes, rebased or force-moved heads require new indexes, and dirty worktrees are represented through explicit worktree overlays.

## Command/API Entry Points

Narrow query kinds include `symbol`, `definition`, `references`, `callers`, `callees`, and `imports`. `--kind hybrid` searches symbols, definitions, references, imports, calls, and chunks together.

## Degradation and Diagnostics

Unsupported, invalid UTF-8, binary, or oversized files degrade to text-only chunks. Syntax trees with error nodes are indexed as partial and record file diagnostics.

## Related Architecture Chapters

- [Source Scope Model](../03-architecture-specs/04-source-scope-model.md)
- [Tree-sitter Extraction and Incremental Indexing](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)

---

Navigation: Previous: [7. Multimodal Evidence Capability](07-multimodal-evidence-capability.md) | Next: [9. Code Graph Competitive Features](09-code-graph-competitive-features.md)
