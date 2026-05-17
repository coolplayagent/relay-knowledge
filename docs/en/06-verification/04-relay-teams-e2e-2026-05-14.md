# relay-teams E2E Verification 2026-05-14

[English](../../en/06-verification/04-relay-teams-e2e-2026-05-14.md) | [中文](../../zh/06-verification/04-relay-teams-e2e-2026-05-14.md)

This is the English documentation page for `06-verification/04-relay-teams-e2e-2026-05-14.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

## Scope

Used `/opt/workspace/relay-teams` as the external test repository for
end-to-end verification of the current `relay-knowledge` CLI, Web workspace,
same-origin Web APIs, and MCP HTTP surfaces.

Test repository state:

- Branch: `improve-memory-skill-draft-status-ui`
- Commit: `fa3c0ddc9d81400b8d5e58ab7600dd557a056816`
- Baseline branch used for impact checks: `main`

Runtime isolation:

- `RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/home`
- Web bind: `127.0.0.1:8791`
- MCP scopes: `docs,src,frontend,relay-teams-benchmark`
- Raw command logs: `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214`

## Build And Browser Gate

Passed:

- `./build.sh`
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`: 400 library unit tests, 1
  benchmark test, and 41 integration tests passed
- `cargo test --test benchmarks --all-features -- --nocapture`
- `uv run --extra dev python -m playwright install chromium`
- `uv run --extra dev pytest tests/browser`
- Live Playwright smoke test against `http://127.0.0.1:8791`

The live browser check opened the real Rust-served Web workspace, exercised
retrieve, code status, worker status, and mobile layout checks.
Five `wait_until="networkidle"` page-load samples averaged 528.36ms. The page
text included `1653 files / 28125 symbols` and did not contain
`Code graph empty`.

## CLI Coverage

Passed:

- `--version`
- `--help`
- `status --format json`
- `health --format json`
- `service status --format json`
- `service plan install --format json`
- `service plan uninstall --format json`
- `service definition write --format json`
- `service operator status --format json`
- `service operator pause --format json`
- `service operator resume --format json`
- `ingest --source docs ... --format json`
- `query ... --freshness wait-until-fresh --format json`
- `graph inspect --format json`
- `index refresh --kind bm25 --kind semantic --kind vector --format json`
- `provider probe --format json`
- `worker status --format json`
- `worker run-once --kind extractor --format json`
- `proposal list --state proposed --format json`
- `proposal show <proposal-id> --format json`
- `proposal reject <proposal-id> --by e2e --reason ... --format json`
- `proposal accept <proposal-id> --by benchmark --reason ... --format json`
- `proposal supersede <proposal-id> --by benchmark --reason ... --format json`
- `audit query --limit 20 --format json`
- `repo register /opt/workspace/relay-teams --alias relay-teams --format json`
- `repo scope preview relay-teams --ref HEAD --format json`
- `repo index relay-teams --ref HEAD --dry-run --format json`
- `repo index relay-teams --ref HEAD --format json`
- `repo status relay-teams --format json`
- `repo report relay-teams --format json`
- `repo report relay-teams --format markdown`
- `repo query relay-teams --kind hybrid --format json`
- `repo query relay-teams --kind definition --format json`
- `repo query relay-teams --kind references --format json`
- `repo query relay-teams --kind callers --format json`
- `repo query relay-teams --kind callees --format json`
- `repo query relay-teams --kind imports --format json`
- `repo update relay-teams --base HEAD --head HEAD --format json`
- `repo impact relay-teams --base main --head HEAD --format json`

Code indexing result for `relay-teams`:

- Indexed files: 1,653
- Symbols: 28,125
- References: 187,993
- Chunks: 28,436
- Degraded files: 218

Expected degraded/default behavior:

- `provider probe` returned `ok=false` with
  `remote_embedding_not_configured` because no external embedding provider was
  configured. Local semantic/vector read models were still available and fresh.

## Web And HTTP Coverage

Passed:

- `GET /`
- `GET /api/project/status`
- `GET /api/health`
- `GET /api/service/status`
- `POST /api/web/operations/execute` for:
  - `retrieve.context`
  - `graph.ingest`
  - `graph.inspect`
  - `index.refresh`
  - `provider.embedding.probe`
  - `worker.status`
  - `worker.run-once`
  - `proposal.list`
  - `proposal.show`
  - `proposal.reject`
  - `proposal.accept`
  - `proposal.supersede`
  - `audit.query`
  - `code.repo.register`
  - `code.repo.index`
  - `code.repo.update`
  - `code.repo.status`
  - `code.repo.query`
  - `code.repo.impact`
  - `service.doctor`
  - `service.run.streamable_http`

The Web code workflow was also verified with a separate alias
`relay-teams-web`, registered against `/opt/workspace/relay-teams`. Re-registering
the same Git root preserves the existing `relay-teams` alias and resolves the
new alias to the same repository id.

## MCP Coverage

Passed against the same `127.0.0.1:8791` service:

- `initialize`
- `notifications/initialized`
- `tools/list`
- `resources/list`
- `prompts/list`
- `ping`
- `GET /mcp/metrics`

## Findings

Follow-up performance verification in
[`docs/zh/05-benchmarks/01-relay-teams-baseline-2026-05-14.md`](../05-benchmarks/01-relay-teams-baseline-2026-05-14.md)
re-tested the live Rust-served Web page after full-repository indexing. The
dashboard displayed repository code totals and did not show the earlier
`Code graph empty` state.

### RK-E2E-2026-05-14-1: Web Dashboard Shows Code Graph Empty After Successful Repository Index

Severity: Medium

Status: resolved and re-verified. Keep this finding as historical evidence for
the earlier filtered-scope run. In the current re-test, `/api/health` graph code
counters matched `repository_code_totals`, and the live page displayed
`1653 files / 28125 symbols`.

In the earlier filtered-scope run, after indexing `/opt/workspace/relay-teams`,
`/api/health` reported
`repository_code_totals.indexed_file_count=738`,
`symbol_count=14286`, `reference_count=88082`, and `chunk_count=14296`.
The Web page still displayed:

- `Code files 0`
- `Symbols 0`
- `References 0`
- `Code graph empty`
- `0 files / 0 symbols`

Impact: users can successfully register, index, query, and report a code
repository, but the dashboard summary makes the code graph look empty. The
operation composer still works, so this appears to be a Web presentation or
API field-selection issue rather than an indexing failure.

Evidence:

- Historical API output:
  `/tmp/relay-knowledge-e2e-20260514092854/api_health.out`
- Historical live page text dump:
  `/tmp/relay-knowledge-e2e-20260514092854/live_page_text.out`
- Re-test API output:
  `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/web_health.out`
- Re-test live page text dump:
  `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/live_page_text.out`

### RK-E2E-2026-05-14-2: Documented `repo update --base main --head HEAD` Path Is Brittle On Non-main Branches

Severity: Low

Status: re-verified as a documented precondition. In a runtime where only
`HEAD` was indexed, `repo update relay-teams --base main --head HEAD --format json`
still failed:

```text
incremental base ref 'main' resolves to 0a4e709c86f25d4fd475113f20d78f9a99498c37,
but code repository 'relay-teams' is indexed at fa3c0ddc9d81400b8d5e58ab7600dd557a056816
```

`repo update relay-teams --base HEAD --head HEAD --format json` passed, and
`repo impact relay-teams --base main --head HEAD --format json` passed. In a
separate runtime, indexing `0a4e709c86f25d4fd475113f20d78f9a99498c37` first
and then updating to `fa3c0ddc9d81400b8d5e58ab7600dd557a056816` passed in
7.56s.

Impact: the README-style workflow can fail for users validating a feature
branch unless they first index the base ref or use a base/head pair that matches
the indexed scope. This is likely expected validation behavior, but the docs or
CLI error could better explain the required sequence.

Evidence:

- Failed command:
  `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/out/repo_update_main_head_after_head_indexed.err`
- Passing command:
  `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/out/update_base_to_head.out`
