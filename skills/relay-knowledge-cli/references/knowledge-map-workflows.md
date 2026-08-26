# Knowledge Map and Code Map Workflows

Use this reference when an agent initializes repository knowledge, plans a
specification, starts a coding task, reacts to a Git commit, or changes an
authoritative document/configuration source.

The shared entry point is `.knowledge/knowledge-map.yaml`. In schema v2 it is a
small root manifest: topic sources/routes live in content-addressed
`.knowledge/topics/` shards, and history older than the bounded recent window
lives in a verified `.knowledge/history/` archive. `map route <topic>` loads one
shard; `map show` loads current shards and returns only the bounded recent-history
window. Use `map history --from <version> --limit <count>` for explicit pages of
at most 256 entries. Never edit shard refs or archive files directly. Mutations
append only newly completed history chunks and
clean superseded topic shards after committing the root while protecting any
recovery-manifest refs. The code map is the primary source of truth for
repository facts. The map stores stable navigation and repository-model entry
metadata; it must not copy derived architecture narratives, build targets,
deployment resources, framework scan results, or resolved commit ids. Read
those snapshot-bound facts through `repo business`, `repo software`, `repo
context`, and `repo view`.

`map init` creates a new contract or idempotently ensures this default model
entry on an existing contract:

- topic: `software-model`
- source: `repository-software-model`
- kind: `repo`
- URI: `.`
- scope: `repo`

It also ensures the authored business entry:

- topic: `business-knowledge`
- source: `repository-business-glossary`
- kind: `file`
- URI: `.knowledge/business-glossary.yaml`
- scope: `repo`

The Knowledge Map remains routing metadata. The glossary is the intentionally
authored, version-controlled business surface; edit it directly and review it
as source code. `map init` creates only a missing minimal valid glossary and
must never overwrite an existing one.

If that reserved source id has incompatible fields, stop and report the
conflict. Do not overwrite it.

## Agent Decision Rules

- Run `map validate --format json` before reading or changing the contract.
- A missing map may be created with `map init`; an existing invalid map must
  not be replaced automatically.
- Run `map init` during repository bootstrap even when the file exists so a
  valid v1 map is migrated and receives the default software-model route.
- Use `map show` before adding a source. One topic can contain multiple sources,
  each with a distinct stable id.
- Treat `map show.history.complete=false` as an explicit archive omission, not
  data loss; use bounded `map history` pages or `map validate` when old history
  is relevant.
- Use only `map source add`, `map source update`, or `map source remove` for
  normal mutations, then validate again.
- Do not copy the YAML into `AGENTS.md`; keep only
  `Knowledge map: .knowledge/knowledge-map.yaml`.
- Read `map route business-knowledge --format json` before business/spec/coding
  work and verify the routed glossary is the intended authority.
- Do not materialize `repo software` or `repo view` responses into the YAML.
  Do not materialize `repo business` responses into the Knowledge Map or
  glossary. They remain derived, source-scope-bound read models.
- If a map mutation must affect the current uncommitted coding decision,
  refresh a `worktree` overlay after a clean `HEAD` base exists. Otherwise
  commit the map with its related sources and publish it in the next update.
- Edit YAML directly only when the CLI is unavailable and the user explicitly
  requests manual repair.

## Repository Knowledge Bootstrap

Bootstrap is complete only when both the map and code map are ready. It is a
recoverable workflow, not an atomic cross-file/database transaction.

1. Validate the map, create/upgrade it with `map init`, and validate again.
2. Read `repo list`; reuse an entry whose normalized root and registered scope
   match. Otherwise register the repository and capture the returned alias.
3. Index a clean `HEAD` first. If bootstrap changed the map or other authorized
   uncommitted files must be visible, index `worktree` only after the clean base.
4. Treat index responses as durable tasks. Recover command timeouts through
   `repo status`; let an active managed service drain the queue; otherwise use
   only bounded single-shot `repo index-worker` attempts for queued/retrying
   work.
5. Wait for the exact target and completed checkpoint. Do not treat stale,
   queued, running, retrying, or dead-letter state as success.
6. At that same immutable ref, read `repo business --kind all`, `repo software
   --kind all`, and both architecture/business-domain views, then validate the
   map once more.
7. Report alias, map version, resolved ref, source scope, freshness, degraded
   diagnostics, and whether direct source reads are required.

POSIX bootstrap commands:

```bash
relay-knowledge map validate --format json
relay-knowledge map init --format json
relay-knowledge map validate --format json
relay-knowledge map route business-knowledge --format json
relay-knowledge repo list --format json
relay-knowledge repo register . --format json
relay-knowledge repo index <alias> --ref HEAD --format json
relay-knowledge repo status <alias> --format json
relay-knowledge repo index <alias> --ref worktree --format json
relay-knowledge repo business <alias> --kind all --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo software <alias> --kind all --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind architecture-layers --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind business-domains --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge map validate --format json
```

The register command is conditional: do not create a duplicate when `repo
list` already has the matching completed root/scope. The `worktree` command is
also conditional: omit it when no authorized uncommitted state must be modeled.

PowerShell bootstrap commands:

```powershell
relay-knowledge map validate --format json
relay-knowledge map init --format json
relay-knowledge map validate --format json
relay-knowledge map route business-knowledge --format json
relay-knowledge repo list --format json
relay-knowledge repo register (Get-Location).Path --format json
relay-knowledge repo index <alias> --ref HEAD --format json
relay-knowledge repo status <alias> --format json
relay-knowledge repo index <alias> --ref worktree --format json
relay-knowledge repo business <alias> --kind all --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo software <alias> --kind all --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind architecture-layers --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind business-domains --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge map validate --format json
```

cmd.exe bootstrap commands:

```cmd
relay-knowledge map validate --format json
relay-knowledge map init --format json
relay-knowledge map validate --format json
relay-knowledge map route business-knowledge --format json
relay-knowledge repo list --format json
relay-knowledge repo register "%CD%" --format json
relay-knowledge repo index <alias> --ref HEAD --format json
relay-knowledge repo status <alias> --format json
relay-knowledge repo index <alias> --ref worktree --format json
relay-knowledge repo business <alias> --kind all --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo software <alias> --kind all --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind architecture-layers --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind business-domains --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge map validate --format json
```

## Spec-Grounded Incremental Loop

For a normal Git commit, run one `repo update <alias>` and capture the immutable
base/head from its completed summary or queued task. Do not reissue update just
to obtain a non-null summary. Let the service drain the task or run bounded
local worker attempts, then require `repo status` to identify the exact head as
fresh.

Before writing or revising a spec, read `map route business-knowledge` and
combine snapshot-bound business terms/mappings, software, architecture and
business-domain views, and code context. After implementation,
run impact on the pinned pair and repeat the model/context reads at the pinned
head. When Markdown, specs, or the map changed, also inspect software topics,
relationships, and a focused OKF neighborhood.

```bash
relay-knowledge repo update <alias> --format json
relay-knowledge repo status <alias> --format json
relay-knowledge repo impact <alias> --base <pinned-base> --head <pinned-head> --limit 100 --format json
relay-knowledge repo business <alias> --kind all --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge repo context <alias> --query "explain the affected implementation and tests" --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge repo software <alias> --kind all --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind architecture-layers --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind business-domains --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge map validate --format json
```

The spec must map requirements to code symbols, call/dependency edges,
configuration, build/deployment evidence, and tests. Preserve unresolved
external targets and degraded diagnostics instead of filling gaps with guesses
or unbounded text search.

## Source Reconciliation

Add a route only for an authoritative source that actually exists in the
authorized repository or external scope. Typical mappings are:

| Evidence | Map kind | Typical topic |
| --- | --- | --- |
| architecture/design Markdown | `doc` | `architecture` |
| package/build manifest | `config` | `build` |
| CI workflow | `ci` | `build` or `release` |
| container/service/IaC manifest | `config` or `runtime` | `deployment` |
| repository root/model entry | `repo` | `software-model` |

Check `map show` first, use a stable source id, and preserve route order. Remove
or move a source only when authoritative evidence confirms the old route is no
longer valid and the requested task authorizes that mutation.

```bash
relay-knowledge map source add \
  --id cli-reference \
  --topic cli \
  --kind doc \
  --uri docs/zh/01-user-guide/03-cli-command-reference.md \
  --scope docs \
  --description "CLI command reference" \
  --format json
relay-knowledge map source update --id cli-reference --description "User-facing CLI command reference" --format json
relay-knowledge map route cli --format json
relay-knowledge map validate --format json
```

## Completion Evidence

A successful handoff records:

- valid map path and `map_version`;
- matching repository alias/root/scope;
- pinned ref or base/head and code-index source scope;
- completed checkpoint and non-stale state;
- software-model and architecture-view freshness/evidence;
- authored business term/mapping and business-domain view freshness/evidence;
- impact/context evidence used for the spec or code;
- every degraded, unresolved, truncated, or direct-source-read requirement.
