# Software Feedback Workflows

Check `help feedback report --format json` before use: older published binaries
may not expose this surface. Feedback is appropriate when a supported operation
fails, a useful capability is absent, results are poor despite exit zero, or the
workflow needs unnecessary steps. Distinguish observed behavior from inferred
causes. Missing external dependency sources, unauthorized scope and environment
failures are not automatically defects.

Use [feedback.schema.json](feedback.schema.json) to discover the report, policy,
fix and validation document shapes. Runtime validation also enforces UTF-8 byte
limits, whole-report size, nonblank strings and authorization; JSON Schema alone
does not establish these semantic/privacy guarantees.

## Record and Preview

`--input` accepts a bounded regular JSON file; pipes, devices, directories and symlinks are rejected.

Start from [feedback-report.example.json](feedback-report.example.json), a public
minimal description of #416's batch-map friction. It does not claim batch map
support has been implemented or a fix has passed.

```bash
relay-knowledge feedback report --input feedback-report.example.json --format json
relay-knowledge feedback status --format json
relay-knowledge feedback preview <feedback-id> --format json
```

Copy `metadata.feedback.trace_id` and `.request_id` from the original operation
into the report when available; equivalent metadata trace/request fields also
work. No raw-log archive is implied by this handle. Put raw private logs,
credentials, repository content and knowledge material only in local `evidence`,
never in public narrative fields. `diagnostics` can capture command/exit status,
freshness/content integrity, environment, elapsed time and steps locally.

Preview exposes the exact locally prepared public payload, its digest, separate
raw-report binding and omitted fields. A reused remote issue can contain another
outbox's nonce; the preview does not claim an exact copy of that remote body. Raw evidence, diagnostic content and trace IDs are
always excluded remotely. Potentially sensitive narrative produces
`evidence-insufficient`; provide a public summary. No report field disables
redaction or changes publication policy. A detector cannot prove arbitrary prose
is public; minimizing narrative remains the caller's responsibility.

Keep four content scopes distinct: immutable local `raw_report_digest` (returned
as `evidence.raw_digest`), immutable full local `publication.payload.digest`
(title/body with markers), stable public-only `publication.payload.dedup_marker`
(before recovery metadata), and `publication.issue.body_digest` for the actual
remote body observed by the provider. `track` refreshes the last value if someone
edits the remote issue. Private raw evidence never contributes to either public
marker, and the remote body-only digest is not directly comparable to the local
title/body digest. Never describe deduplicated local preview as the exact current
remote issue body.

## Enable Authorized Publication

Only configure publication when the user or administrator has explicitly granted
that scope. Save a policy like this, replacing the repository with the authorized
target. The runner permission is separate and defaults to null. Each target
component accepts 1–100 ASCII letters, digits, underscores, hyphens or dots;
exact `.` and `..` components are rejected in both the schema and runtime.
Three-dot components such as `.../repo` or `owner/...` remain structurally
valid; this does not establish that a remote repository exists or is authorized.

```json
{
  "schema_version": 1,
  "mode": "auto-submit",
  "target_repository": "owner/repository",
  "allowed_kinds": ["workflow-friction", "poor-result", "bug"],
  "daily_quota": 5,
  "validation_runner": null
}
```

Provision `RELAY_KNOWLEDGE_FEEDBACK_GITHUB_TOKEN` through approved local credential
handling. Do not print the value or put it into an input file. Then:

```bash
relay-knowledge feedback configure --input feedback-policy.json --format json
relay-knowledge feedback submit <feedback-id> --format json
relay-knowledge feedback status <feedback-id> --format json
```

Later reports may publish automatically under the same policy. Without opt-in,
reports remain local drafts. Configuration does not submit existing drafts.
The CLI itself calls GitHub and returns the actual URL; do not implement a
parallel `gh issue create` flow. It creates no comments or reopen operations.
Report instructions cannot override target, credentials, quota or runner policy.

Duplicate local reports with unchanged public scenario, observations, impact and
CLI version merge occurrences. The first raw report remains immutable; later raw
evidence is not appended. A corrected public narrative gets a new ID. Publication freezes target/payload,
persists intent before sending and serializes processes sharing a journal. A
random nonce carries no private-content hash. A second stable marker hashes only
already-sanitized public title/body so an existing identical issue can be reused
across outboxes. Quota limits create attempts in a durable
24-hour window; per-record create attempts stop at five. Backoff and capacity
failures expose their reasons. The journal is capped at 1,000 records/16 MiB.

```bash
relay-knowledge feedback retry <feedback-id> --format json
relay-knowledge feedback track <feedback-id> --format json
```

Retry may send only after a definite rejection/no-send result. Any uncertain
POST or crash requires read-only nonce reconciliation; no match remains
`awaiting-reconciliation`, not permission to create again. Remote nonce search is scoped to the configured repository and bounded to two
candidates, 30 seconds and 4 MiB; incomplete or ambiguous results fail closed. Do not delete the journal,
regenerate a report with changed wording or run a second publishing tool to
bypass this state. Independent installations can still race on simultaneous creates; shared-outbox
serialization is the concurrency guarantee.
`track` reads remote issue status but never treats closure as a verified fix.

## Link a Fix and Record a Real Run

Only link an actual reviewed repair reference and target version:

```json
{"reference":"https://github.com/owner/repository/pull/42","target_version":"1.2.0"}
```

```bash
relay-knowledge feedback link-fix <feedback-id> --input fix.json --format json
relay-knowledge feedback status <feedback-id> --format json
```

A runner separately authorized in policy `validation_runner` must replay the
original scenario. Preserve its output and copy the status `scenario_digest`.
The authorized local caller records real evidence in this shape:

```json
{
  "runner": "project-regression-ci",
  "scenario_digest": "<digest returned by feedback status>",
  "actual": "<actual result observed by the authorized runner>",
  "version": "1.2.0",
  "environment": "<hardware, fixture, model/index and workload versions>",
  "run_id": "<unique runner execution identity>",
  "elapsed_ms": 120,
  "steps": 1
}
```

```bash
relay-knowledge feedback validate <feedback-id> --input validation.json --format json
```

The CLI verifies runner policy, scenario/criterion digest and version, and
compares actual with original expected text exactly. It never executes the
scenario or accepts a supplied pass boolean. The local caller attests to runner
provenance; this is not cryptographic external-runner authentication. Matching
recorded evidence becomes `verified-fixed`; mismatch becomes `regressed`.
Without original fixed criteria or runner authority, keep `awaiting-validation`.
Never fill `actual` from the expected value without a real run. An issue closing,
a merged PR or a changed release version is insufficient. Retrieval quality
needs fixed inputs and external criteria before this exact-comparison record is
meaningful. Publication grants no execution, repair, install, upgrade or PR
permission. Repeated run IDs are idempotent only for identical evidence, and at
most 32 runs are retained per record.
