# Relay Knowledge CLI Skill

This ClawHub-compatible skill teaches LLM agents to operate `relay-knowledge`
through the local CLI. It is for local knowledge graph ingestion, hybrid
GraphRAG queries, code repository indexing, code graph search, multi-repository
queries, authored business-term and technical-mapping queries, software graph
relationship queries, feature flag graph queries,
OKF Markdown neighborhoods, commit-driven impact/context loops, setup
diagnostics, installation checks, and upgrade checks. For large repositories,
it tells agents to treat cold and incremental indexing as durable single-writer
tasks so command-runner timeouts do not interrupt or obscure progress.

Repository bootstrap initializes or upgrades the
`.knowledge/knowledge-map.yaml` contract and the code map as one recoverable
workflow. The YAML contains stable `software-model` and `business-knowledge`
routes; the latter points to the version-controlled authored
`.knowledge/business-glossary.yaml`. Snapshot-bound business, architecture,
build, deployment, dependency, and design facts remain in the indexed `repo
business`/`repo software`/`repo view` read models. Before a spec or coding task,
agents pin one ref and combine those models with business/domain views and code
context. After a commit, they refresh the durable code task, impact/context
evidence, and final map validation together.

For code-structure questions such as function definitions, symbol locations,
references, callers, callees, call graphs, and call chains, agents should use
this skill before `grep`, `ripgrep`, `rg`, or plain text search. Fall back to
text search only when the CLI cannot satisfy the request, the target repository
cannot be indexed, or the user explicitly needs raw text or regular-expression
matching.

For `repo query --kind` prompts, the supported code query kinds are `hybrid`,
`symbol`, `definition`, `references`, `callers`, `callees`, `imports`, and
`sbom`. Agents should choose one of these kinds first and treat `grep`/`rg` as
fallback tools, not the preferred path.

For repository-wide software graph prompts, agents should use
`repo software --kind` with `dependencies`, `sdks`, `files`, `topics`,
`relationships`, `build`, `iac`, `design`, or `all`. Use
`repo software --kind relationships` when the user asks for graph
relationships, dependency paths, architecture maps, or `代码图关系`.

For authored domain terms, aliases, acronyms, semantics, conflicts, or
business-to-technical links, agents should read `map route business-knowledge`
and then use `repo business --kind terms|mappings|all` at the same immutable ref
as `repo context`, software, and architecture/business-domain views.

For YAML-frontmatter Markdown knowledge bundles, agents should use `repo graph`
with an explicit focus file, bundle-root path, and immutable indexed ref. This
returns the bounded OKF v0.2 concept/source neighborhood; it is distinct from
the callers/callees code graph.

For feature flag, config gate, environment-variable gate, settings gate, or
guarded-code questions, agents should use `repo feature-flags`. Feature flags
are not a `repo query --kind` value.

Kind values are command-local. Do not use `index refresh --kind` values
(`bm25`, `semantic`, `vector`), worker values (`embedding`, `ocr`, `vision`,
`extractor`), or knowledge-map source values (`repo`, `file`, `doc`, `config`,
`db`, `ci`, `runtime`, `wiki`, `monitoring`) as `repo query` or
`repo software` kinds.

For cold or incremental repository indexing in non-interactive sessions,
agents should run `repo index` or `repo update`, then inspect `repo status
<alias> --format json` because either operation may return a task id or time out
after claiming a durable lease. Agents should let a managed service drain
active tasks; only a local/service-host client may use `repo index-worker` for
bounded single-shot attempts when status shows queued/retrying work. Each local
attempt also advances one bounded retention pass and returns
`maintenance_active` plus optional `maintenance_error`; an error makes a false
activity value inconclusive, so status remains the maintenance source of truth.

The normal Git loop is `repo update <alias>`: head defaults to `HEAD` and base
defaults to the last published clean commit, including unwrapping a prior
worktree identity. Agents must wait until the exact resolved target is fresh,
use a local completed response's `summary.base_resolved_commit_sha` and
`summary.resolved_commit_sha` when present, or treat a queued task's pinned
incremental base/head as authoritative, run `repo impact` on that immutable
pair, then run `repo context --ref <resolved-head>` without reissuing update to
obtain a summary.
Markdown/spec/map changes also require
`repo software --kind topics|relationships` and a focused `repo graph` read.
With the managed watcher enabled, service-side Git ref reconciliation submits
the same durable update automatically; CLI update remains recovery/manual
ingress rather than an unmanaged polling loop.

Successful publication prunes old graph scopes and derived indexes. It retains
the active scope plus a rolling window containing the two latest successful
scopes (the window normally includes the active scope), the latest incremental
predecessor, the clean base of an active worktree overlay, unfinished-task
bases/targets, and repository-set pins. Same-tree commit aliases use a bounded
256-row window. Each maintenance transaction advances one scope-GC phase whose physical
deletion is capped at 512 rows in aggregate across affected application tables;
separate succeeded-audit, failure-class-audit, and alias quotas cap primary
cleanup at 2,048 physical rows plus at most one terminal job row per pass. GC
bounds live generations and lets SQLite reuse free pages; it
does not promise immediate OS-visible file shrink, which requires a separate
explicit bounded compaction. In partitioned storage, the control catalog route
remains a counted slot throughout batched shard deletion and only the retention
coordinator removes it immediately before final `scope_metadata`; agents must
not delete or bypass that route to relieve capacity. Pruned refs cannot be incremental bases; agents must publish a
new full snapshot. Non-Git directories stay on the separate
`repo index --ref HEAD` flow and receive no Git commit events.

Before registering, inspect existing completed scopes with
`repo list --format json` and reuse a matching alias. Large-repository budgets
are elastic by default: the historical 180-second value is a baseline, while
the effective budget scales with authorized file count and throughput and is
bounded by an explicit cap.

## Package Contents

- `SKILL.md`: agent instructions and skill metadata.
- `agents/openai.yaml`: UI metadata for OpenAI-compatible agent surfaces.
- `references/cli-workflows.md`: detailed CLI workflows and safety defaults.
- `references/knowledge-map-workflows.md`: agent workflow for CRUD operations
  on the `.knowledge/knowledge-map.yaml` navigation contract plus repository
  bootstrap and spec-grounded incremental development.
- `assets/linux-x86_64/relay-knowledge`: Linux x64 release binary in generated
  GitHub Release packages, built and checked against the glibc 2.31 baseline.
- `assets/windows-x86_64/relay-knowledge.exe`: Windows x64 release binary in
  generated GitHub Release packages.

ClawHub receives the instruction and reference files without embedded binaries
because the registry limits individual files to 10 MB. The runtime-selection
rules therefore use a published `PATH` install when those assets are absent.

Keep the `SKILL.md` frontmatter `description` at or below 1024 Unicode
characters. Local checks, pre-commit, PR CI, release packaging, and ClawHub
publish validation all run the shared skill metadata gate. Quote the
description when it contains YAML-sensitive punctuation such as `: `.

## Runtime Selection

Resolve `relay-knowledge` before running workflow commands. Prefer the bundled
asset binary for the current operating system, CPU, and active command runner
whenever it exists, is executable, and `version --format json` succeeds. Keep
that absolute path in a shell variable and use it for every CLI command.

Do not run the Windows bundled asset from POSIX shells such as bash, sh, zsh,
fish, or WSL bash unless the command intentionally crosses into a Windows shell
boundary. Windows `.exe` examples belong in PowerShell or cmd.exe command
blocks; POSIX examples must use `assets/linux-x86_64/relay-knowledge` or a
POSIX `PATH` install.

Use a published binary on `PATH` only when the bundled asset is absent,
unusable, unsupported on the current operating system or CPU architecture,
unsupported by the active shell boundary, incompatible with the host Linux glibc
version, or explicitly requested by the user. If no usable binary is available,
install `relay-knowledge` from a published channel first, such as a verified
GitHub Release archive or `cargo install relay-knowledge` from crates.io.

## Protocol Boundary

This skill is intentionally CLI-only. It does not configure MCP, call MCP
tools, manage ACP sessions, or replace protocol-level agent access. Use the
project MCP/ACP documentation for those integrations.
