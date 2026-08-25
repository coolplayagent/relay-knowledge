# Code Repository Tree-sitter Retrieval Research

[English](05-code-repository-tree-sitter-retrieval-research.md) | [中文](../../zh/04-research/05-code-repository-tree-sitter-retrieval-research.md)

[Documentation index](../README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

> Document version: 1.0
> Prepared: 2026-05-12
> Scope: structured Tree-sitter parsing, Git incremental change discovery, code
> knowledge graphs, and high-performance indexing and retrieval
> Purpose: support the design choices in
> [Tree-sitter Extraction and Incremental Indexing](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)

## Research Positioning

| Dimension | Conclusion |
| --- | --- |
| Sources | Tree-sitter, Git, libgit2, GitHub code navigation, ripgrep, Codebase-Memory, and this repository's code-graph experience. |
| Goal | Move repository retrieval from full-text search to a combination of Git snapshots, syntax structure, symbol/reference graphs, and incremental indexing. |
| Competitive focus | Structured code facts, file-level incremental work, scope authorization, hybrid recall, impact analysis, and recoverable indexing are core agent-facing advantages. |
| Scenarios and future | Targets large-repository understanding, dirty-worktree queries, code review, impact reports, agent context packing, and later language-service adapters. |

## 1. Conclusions

Repository retrieval cannot rely on full-text indexing alone. The mature design
combines Git version boundaries, Tree-sitter syntax, symbol/reference graphs,
lexical/vector retrieval, and incremental indexing:

1. Tree-sitter is a strong v1 parsing foundation. It produces concrete syntax
   trees across languages and supports incremental parsing, query captures,
   error recovery, and navigation tags.
2. Git is the source of truth for snapshots and change discovery.
   `diff --name-status -z -M` supports commit-to-commit updates,
   `status --porcelain=v2 -z` supports worktree overlays, and commit-graph
   changed-path Bloom filters can accelerate history queries.
3. Performance comes from file-level deltas and scoped index refresh, not full
   rebuilds: changed paths -> content-hash skips -> reverse dependents ->
   bounded parse -> scoped refresh.
4. Retrieval is hybrid. BM25 is strong for symbols, paths, and error codes;
   semantic/vector retrieval serves conceptual questions; graph expansion serves
   calls, references, dependencies, and impact. A bounded exact-text source
   fallback fills specific structured-recall gaps but never masks stale or
   degraded graph state.
5. v1 does not promise compiler semantics. Tree-sitter can extract syntax-level
   definitions and references, while cross-file resolution, dynamic calls,
   macro expansion, and type inference need later language-service or compiler
   adapters.

## 2. Source Ledger

| Source | Key evidence | Design consequence |
| --- | --- | --- |
| Tree-sitter README [R1] | Parser generator and incremental library designed for speed and error tolerance | Local structured extraction foundation |
| Advanced parsing [R2] | Edited old trees can be reparsed; included ranges support mixed-language documents | Reuse old trees for editor changes; use ranges/region adapters for embedding |
| Code navigation [R3] | Query captures mark definitions, references, calls, and documentation | Adopt `@definition.*`, `@reference.*`, `@name`, and `@doc` contracts |
| Rust `Parser` [R4] | `parse` accepts UTF-8 text and an optional edited old tree | Model full and incremental parsing explicitly |
| Rust `Query` [R5] | Language-bound syntax patterns; immutable query references can be shared | Cache a versioned per-language query registry |
| Git diff [R6] | Status output, NUL-separated paths, rename detection | Machine-readable commit deltas |
| Git status [R7] | Stable porcelain v2 and NUL separation | Machine-readable dirty-worktree overlays |
| Commit-graph [R8] | Optional changed-path Bloom filters | Later history/path acceleration |
| libgit2 diff [R9] | Tree-to-tree, tree-to-index, and index-to-workdir APIs | Possible structured adapter beyond Git CLI |
| GitHub navigation [R10] | Tree-sitter definitions and references at repository scale | Validates tag-based navigation |
| Codebase-Memory [R11] | Tree-sitter graph over MCP, parallel workers, calls, impact | Supports an agent-facing code KG |
| Local capability research [R12] | Tree-sitter, multilingual facts, SQLite/FTS5, hashes, impact | Reuse lessons while adding event/scope/QoS boundaries |
| ripgrep notes [R13] | Fast exact text, still requiring scope, timeout, and output budgets | Historical external-search evidence for a bounded fallback role, not an indexing substitute |

## 3. Tree-sitter Capability Boundary

### 3.1 Strong Uses

- **Structured extraction:** definitions, references, calls, imports,
  documentation comments, and source ranges.
- **One multilingual entry point:** grammars and queries map language syntax to
  shared `CodeSymbol` and `CodeReference` facts.
- **Error tolerance:** a partially edited file can still yield a useful tree.
- **Editor-style incremental parsing:** an old tree plus exact edit ranges can
  reduce the cost of hot single-file changes.

### 3.2 What It Does Not Provide Alone

Tree-sitter does not perform type inference, fully expand macros/templates,
resolve dynamic targets, understand complete build/module semantics, or
guarantee a unique cross-file reference. Its output is syntax-level evidence.
Cross-file targets carry `resolution_state`; ambiguous and unresolved are normal
outcomes, not fabricated certainty.

### 3.3 Query Captures

Tree-sitter navigation conventions such as `@definition.class`,
`@definition.function`, `@reference.call`, and `@name` should be paired with a
version identity:

```text
query_identity = language_id + grammar_version + query_name + query_version
```

Persisting that metadata explains extraction changes after grammar/query
upgrades and supports rebuild or rollback.

### 3.4 Incremental-Parse Boundary

Old-tree reuse requires an exact edit and an edited old tree. A Git
commit-to-commit delta usually supplies old/new blobs, not editor operations.

| Scenario | Strategy |
| --- | --- |
| Editor save/watch with exact edit and old tree | Incremental Tree-sitter parse |
| Pull, checkout, or commit-to-commit update | Reparse changed files |
| Grammar/query version change | Reparse the affected language |
| Restart recovery | Reconstruct from Git snapshot and durable state, not an in-memory tree |

This avoids a complex, unrecoverable cache-consistency system merely to reuse
one file's tree.

## 4. Git Incremental Updates

### 4.1 Resolve an Immutable Snapshot

```text
selector: branch/tag/HEAD/sha
  -> resolved_commit_sha
  -> tree_hash
  -> scope_id
```

This prevents a moved branch, rebase, or force-push from corrupting old result
identity. The same tree can reuse an index, and every response can name its
commit.

### 4.2 Commit-to-Commit Delta

`git diff --name-status -z -M base head` provides status, safe NUL-separated
paths, and rename candidates. File changes alone are not graph changes. The
indexer also reads old/new blob hashes, skips identical content, removes old
facts, records move/lineage candidates, and finds reverse dependents whose call
or reference edges may change.

### 4.3 Worktree Overlay

Dirty state is useful for review and local agents but cannot mutate a clean
snapshot. Parse `git status --porcelain=v2 -z` into a `worktree_overlay` or
`git_changeset` scope.

- The clean commit remains the default source.
- Dirty changes enter only when explicitly selected.
- Results expose `uncommitted=true` and path status.
- Overlay indexes can use a short lifetime and are not long-term authority.

### 4.4 Commit-Graph and History

Commit-graph metadata and changed-path Bloom filters can cheaply indicate
whether a commit may have touched a path. They can support history search,
blame-like path evolution, and large-repository optimization. v1 need not parse
the commit-graph format directly; Git can maintain it and a later adapter can
use supported command/library acceleration.

## 5. Code-Knowledge-Graph Lessons

### 5.1 Local Code-Graph Research

Reusable lessons from code-review-graph/better-code-review-graph include Git
tracked-file enumeration, ignore filtering, multilingual Tree-sitter parsing,
SQLite+FTS5 for local graphs, SHA-256 content skips, Git-diff plus dependent
expansion, and recursive-CTE BFS impact rather than loading the whole graph.

The controlled grammar registry now spans source and configuration formats
including Rust, Python, JavaScript/JSX, TypeScript/TSX, Go, Java, Kotlin, Scala,
C/C++, C#, Ruby, PHP, Swift, Bash, Markdown, XML, Bazel/Starlark, Make, CMake,
Dockerfile/Containerfile, Java properties, TOML, INI, YAML, JSON, Go modules,
Ninja, Jinja2, and Go templates. The implementation contract, not this research
inventory, remains authoritative for exact current coverage.

`relay-knowledge` adds stronger boundaries: async/event-driven coordination,
per-scope versions, platform-managed background operation, bounded queues, QoS,
dead letters, observability, and a unified API between adapters and storage.

### 5.2 GitHub Code Navigation

GitHub's Tree-sitter-based definitions, references, and symbol search validate
repository-scale query tags. Its documented repository-size limits are also a
warning: large repositories need path filters, partitions, budgets, and explicit
degradation rather than a promise of unlimited real-time full-repository work.

### 5.3 Codebase-Memory

Codebase-Memory exposes a Tree-sitter graph to coding agents through MCP and
emphasizes worker pools, call traversal, impact, and community discovery [R11].
That supports structured agent context. `relay-knowledge` nevertheless keeps
runtime ownership separate: the core is a knowledge substrate and every MCP or
agent adapter uses the unified API.

## 6. Rust Implementation Options

### 6.1 Tree-sitter Crates

- Use the `tree-sitter` Rust binding as the parser API.
- Load controlled, versioned grammar/query resources per language.
- Keep `Parser` worker-local instead of sharing mutable parsers across tasks.
- Cache immutable `Query` resources by language and version.
- If an upstream grammar lacks a tag query, maintain a minimal in-repository
  query for function/type definitions and recognizable calls; a configured
  language should not emit only whole-file chunks.

The repository forbids project-authored unsafe code. Dependency internals may
use unsafe, but adapters here do not. Grammar versions enter extractor metadata;
query compile failure is a startup/configuration error or marks the language
unavailable.

### 6.2 Git Adapter

| Option | Strength | Risk |
| --- | --- | --- |
| Git CLI | Matches user Git behavior; supports worktrees, submodules, rename | Process overhead, parsing, platform differences |
| libgit2/`git2` | Structured API without CLI parsing | Behavior can differ from Git; auth/worktree edge cases |
| `gix` | Rust-native long-term integration | Requires more API and compatibility validation |

Define the adapter trait and structured delta contract first. Starting with Git
CLI does not then bind application or retrieval layers to it.

### 6.3 Storage and Indexes

SQLite can support the v1 shape: file snapshot instances, symbol definitions,
references/calls/imports, chunks/ranges, reverse dependencies, and FTS5 over
paths, symbols, chunks, and documentation. Semantic/vector indexes remain
derived read models; embeddings recompute only for changed content hashes.

## 7. Performance Design

### 7.1 Cost Model

```text
full = O(tracked_files + parsed_bytes + extracted_captures + index_writes)
incremental = O(changed_files + affected_files + changed_chunks + refreshed_index_entries)
```

Use Git tree/diff instead of directory rescans, content hashes instead of
reparsing unchanged files, reverse dependencies instead of graph-wide impact,
batched writes instead of many transactions, content-addressed embedding reuse,
and scoped freshness instead of one global stale flag.

### 7.2 Concurrency Model

```text
diff producer
  -> bounded changed-file queue
  -> metadata/hash workers
  -> bounded parse queue
  -> parse/extract workers
  -> bounded mutation batch queue
  -> storage writer
  -> index refresh workers
```

The producer backpressures when queues fill. Parser concurrency fits CPU/memory
budgets. One storage writer batches commits. Embeddings/community rebuilds run
at lower priority. Queries read the newest permitted index generation and do not
wait indefinitely for low-priority maintenance.

### 7.3 Large Repositories

Risks include file count, generated code, vendors, lockfiles, binaries, and many
grammars. Enumerate tracked files, bound file size and every stage, let users
narrow path/language scope, and expose partitioning requirements when product
budgets cannot admit the scope. Do not silently skip required index stages or
move unbounded work to query time.

## 8. Retrieval Quality

### 8.1 BM25

BM25 excels at exact names, paths/modules, configuration keys, error/log text,
feature flags, and concrete API calls. Symbol, path, chunk, and documentation
fields therefore need high-quality lexical materialization.

### 8.2 Bounded Exact-Text Source Fallback

Exact-text fallback fills syntax-capture misses, unresolved but explicit source
usages, and a small recall gap after structured hits. It is not a semantic edge.
It operates only over candidate content from the indexed commit and is bounded
by authorization, path/language filters, file count, materialized bytes, line
length, timeout, and result limit. Hits carry `lexical`/`text_fallback`
provenance, never resolved-edge confidence. The original ripgrep research
motivated this role; the current product uses its internal bounded scanner.

### 8.3 Semantic/Vector Retrieval

Vector/semantic layers serve conceptual questions, onboarding without exact
names, distributed themes, and links between documentation and code. Similarity
from the wrong scope is a major risk, so scope is filtered before or strictly
after ANN and freshness is returned in metadata.

### 8.4 Graph Expansion

Graph expansion serves definition-to-reference, callers/callees, reverse
imports, change impact, tests, and architecture hotspots. Traversal is bounded
by depth, nodes, time, and output; a limit reports `truncated=true` rather than
silently hiding work.

## 9. Risks and Mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Grammar/query drift | Extraction differs for the same source | Record versions and reindex affected language scopes |
| Dynamic-language uncertainty | Noisy calls/references | `resolution_state`; later language services |
| Rename detection cost | Slow large deltas | Bounded/configurable detection; degrade to delete+add |
| Generated/vendor volume | Parse/index explosion | Explicit scope and bounded admission, without weakening declared freshness |
| Dirty state contaminates clean snapshot | Unstable results | Isolated worktree overlay scope |
| Parser saturation | Query latency | QoS priority, worker budget, maintenance boundaries |
| SQLite writer contention | Low throughput | Single-writer batched publication and snapshot reads |
| Embedding cost | Slow refresh | Content-hash reuse, background priority, BM25 degradation |
| Submodules/worktrees | Ambiguous scope | Hierarchical repository ids; explicit submodule/external treatment |
| Oversized cold index | Resource budget breach | Bounded partitions and observable incomplete state |

## 10. Recommended Specification Shape

1. **Git source adapter:** snapshot resolution, diff/status, blob metadata, and
   tracked-file enumeration.
2. **Tree-sitter extraction adapter:** parse, captures, range mapping, and
   diagnostics.
3. **Code graph domain/storage:** versioned facts, dependencies, chunks, and
   changesets.
4. **Retrieval/indexing:** scoped lexical/semantic/vector/code-graph read models
   and unified context packs.

Minimum v1 research target: register a local Git repository, index definitions,
imports, and chunks for the controlled language surface at `HEAD`, retrieve
paths/symbols/chunks, run bounded exact-text fallback only for a specific
structured gap, update commit-to-commit, and return scope, commit, tree, line,
freshness, and degradation. It does not promise compiler type inference,
precise cross-repository calls, automatic code editing, unlimited real-time
full-repository indexing, or an MCP-owned runtime.

## 11. Benchmark Design

| Scale | Corpus | Use |
| --- | --- | --- |
| Small | 100–500 mixed Rust/TS/Python files | Fast CI regression |
| Medium | 5k–20k files including generated/vendor samples | Local performance gate |
| Large | 50k–100k optional downloaded files | Manual/nightly benchmark |

Measure full-build files/sec and MiB/sec; parse outcome ratios; incremental
p50/p95 for one, ten, and one hundred files; time to fresh BM25/vector state;
concurrent query p95/p99; fallback rate, usefulness, degradation, and ranking
effect; impact traversal p95/truncation; queue depth; CPU/memory; and SQLite
transaction time.

## 12. References

- [R1] Tree-sitter README. <https://github.com/tree-sitter/tree-sitter>
- [R2] Tree-sitter documentation, “Advanced Parsing.” <https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html>
- [R3] Tree-sitter documentation, “Code Navigation Systems.” <https://tree-sitter.github.io/tree-sitter/4-code-navigation.html>
- [R4] Rust `tree_sitter::Parser` docs. <https://docs.rs/tree-sitter/latest/tree_sitter/struct.Parser.html>
- [R5] Rust `tree_sitter::Query` docs. <https://docs.rs/tree-sitter/latest/tree_sitter/struct.Query.html>
- [R6] Git documentation, “diff-options.” <https://git-scm.com/docs/diff-options>
- [R7] Git documentation, “`git status`.” <https://git-scm.com/docs/git-status>
- [R8] Git documentation, “`git commit-graph`.” <https://git-scm.com/docs/git-commit-graph>
- [R9] libgit2 documentation, “diff APIs.” <https://libgit2.org/docs/reference/main/diff/index.html>
- [R10] GitHub Docs, “Navigating code on GitHub.” <https://docs.github.com/en/repositories/working-with-files/using-files/navigating-code-on-github>
- [R11] Martin Vogel et al. “Codebase-Memory: Tree-Sitter-Based Knowledge Graphs for LLM Code Exploration via MCP.” <https://arxiv.org/abs/2603.27277>
- [R12] [Code Knowledge Graph Model](../03-architecture-specs/11-code-knowledge-graph-model.md)
- [R13] ripgrep performance notes. <https://burntsushi.net/ripgrep/>

---

Navigation: Previous: [4. ai-knowledge-graph Reference Analysis](04-ai-knowledge-graph-reference-analysis.md) | Next: [6. Agent Protocol Graph Retrieval Research](06-agent-protocol-graph-retrieval-research.md)
