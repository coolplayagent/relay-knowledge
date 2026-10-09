# Research evidence workflow

[中文](../../zh/01-user-guide/17-research-workflow.md) | [English](../../en/01-user-guide/14-research-workflow.md)

Navigation maps, captured bytes, authored claims and retrieval indexes have separate
completion criteria. Passing one check cannot prove the other layers complete.

## Batched map changes

Initialize or migrate with `map init` before using the v4 transaction surface.
The [transaction Schema](../../../skills/relay-knowledge-cli/references/map-transaction.schema.json)
accepts schema_version 1, a stable transaction_id and 1–100 ordered add/update/remove
operations. Input is limited to 256 KiB. File/config URIs must be confined repository
paths. Existing source identities, reserved routes and source invariants still apply.

```json
{
  "schema_version": 1,
  "transaction_id": "research-2026-10-09",
  "operations": [
    {"op": "add", "id": "source-a", "topic": "research", "kind": "file", "uri": "sources/a.txt"},
    {"op": "update", "change": {"id": "source-a", "description": "Archived original"}}
  ]
}
```

```bash
relay-knowledge map plan --type knowledge --input request.json --format json > plan.json
python3 -c 'import json; p=json.load(open("plan.json")); json.dump(p["transaction"], open("transaction.json", "w"), indent=2)'
relay-knowledge map apply --type knowledge --input transaction.json --format json
```

Planning is read-only and returns normalized before/after states, affected routes,
the first invalid operation, and a transaction bound to the current version and root
SHA-256. Apply requires both preconditions and rechecks them under the existing writer
locks. It publishes one root version after the entire batch validates. Invalid
batches never publish a successful prefix. Inspect state and diagnostics: a JSON
response alone is not success. States are planned, applied, unchanged,
already_applied, invalid and conflict.

One source.batch history entry stores a JSON receipt in its existing summary string,
including transaction id, request digest and the complete ordered operation list.
Older v4 writers preserve this field; no writer schema bump is needed. The existing
16-entry retention window remains. Exact replay with a retained receipt does not
publish; conflicting reuse fails. Once history expires, the old version/digest
preconditions reject replay. Use Git for long-term audit and do not reuse retired
transaction ids.

Publication remains root-last with immutable shards and recovery roots. Interrupted
publication cannot expose a batch prefix. cleanup.state=deferred reports that normal
bounded maintenance continues through `map init`. Recovery references remain protected;
retired shards require the existing 60-second reader grace before deletion.

## Local source audit

Use the explicit [capture Schema](../../../skills/relay-knowledge-cli/references/source-catalog.schema.json)
with adapter relay-capture-v1 and schema_version 1. Arbitrary capture catalogs and
MANIFEST layouts are not guessed.

```bash
relay-knowledge sources audit --root . --input sources/catalog.json --format json
```

The input path is relative to the authorized repository root. Each raw, extraction
or reference artifact declares path_base=repository or catalog, a path and the
exact byte SHA-256. Catalog-relative paths start at the input JSON directory.
Absolute paths, parent traversal and symlinks in any component are rejected.

Sources retain id, original URL, optional transport, raw bytes, extraction,
parent_source, references, expected_sections, declared_coverage and review.
Extraction metadata binds the raw hash, extractor and version; optional expected
extractor/version fields detect a declared tool change.

Audit keeps access, capture integrity, extraction integrity, coverage, review and
index freshness separate. Transport is reported_only, reported_access_failure or
unknown. Hash checks produce verified, hash_mismatch or unreadable. Extraction
also checks the raw binding and UTF-8 text. Coverage stays unknown without caller
expectations. Expected sections match whole extracted lines, allowing Markdown
heading prefixes; declared_sections_present proves only those lines are present.
A shell declaration stays declared_shell. A full_body assertion is never proof of
complete content.

Missing review metadata remains unknown. A hash-bound review claim remains
self_reported_unverified, preserving reviewer, origin and event_id; changed content
is needs_review. These fields cannot authenticate a reviewer or change the existing
fact lifecycle. Local audits report index_freshness=not_assessed.

Different URLs with identical bytes appear in identical_byte_groups without merging
source identities. PDF bytes are checked without implicit OCR. Explicit references
check catalog links and source bindings. Diagnostics include machine-readable codes
and repair suggestions. integrity_valid describes these integrity checks only.

Audits never access the network, log in, download, execute source instructions or
modify originals. Whitespace and CRLF are hashed unchanged; authoring style checks
must not rewrite archived bytes. Limits are 256 captures, 16 MiB per artifact,
256 MiB total, four blocking workers and a 30-second deadline. Saturated workers
reject admission; cancellation stops subsequent read chunks and retains permits until
the worker exits.

## Implementation and verification

Map transactions reuse locks, immutable shards, root-last publication, recovery
manifests and reader grace. Source audits use a separate application service and
pure domain contracts. Neither changes the meaning of map validate.

Current stable rustfmt adds trailing commas to multiline test macro invocations.
Matching test macros accept optional trailing commas so formatting and compilation
agree; this compatibility adjustment does not change test-double behavior.

## Authored evidence bundles

The [bundle Schema](../../../skills/relay-knowledge-cli/references/authored-evidence-bundle.schema.json)
wraps an existing graph in schema_version=1, id, source_scope, evidence, optional
supersedes and aliases. The graph retains arbitrary graph/node/edge metadata, including
qualifiers and original author status. Nodes require stable id, kind and label; edges
require source, target and relation. Edge evidence strings reference pin ids. Pin ids
may retain the original graph's path strings without rewriting that graph.

Each pin supplies source_scope, an artifact path/base/SHA-256, an optional byte/line
span and one interpretation: source_statement, author_analysis, hypothesis,
user_scope_confirmation or historical_disambiguation. Byte spans are zero-based,
end-exclusive; lines are one-based and must match the actual byte span. Multiple
claims can share one precise pin. Missing endpoints, duplicate identities, missing or
changed evidence and scope violations produce diagnostics. Only relations depending
on invalid pins become needs_review. No content is repaired or approved automatically.

```bash
relay-knowledge evidence validate --root . --input research/bundle.json --scope research --format json
relay-knowledge evidence view --root . --input research/bundle.json --scope research --focus concept-a --format json
relay-knowledge evidence import --root . --input research/bundle.json --scope research --format json > imported.json
revision=$(python3 -c 'import json; print(json.load(open("imported.json"))["audit"]["bundle_sha256"])')
relay-knowledge evidence export --id study --scope research --revision "$revision" --format json > exported.json
relay-knowledge evidence impact --root . --input research/bundle.json --scope research --node concept-a --label 'Clarified concept' --format json > impact.json
```

Export's bundle field is the re-importable envelope. Revision hashes cover the typed,
compact JSON representation, while input_sha256 records original input bytes. Import
uses the existing shared ingestion service and refreshes BM25, semantic and vector
indexes. It does not store graph rows in navigation maps or introduce another database.
All new evidence, claims and relations are proposed, regardless of author metadata.
The existing CLI/Web fact lifecycle remains the authority for acceptance, rejection
and supersession. A user_scope_confirmation never proves a feature is supported.
Repeated sequential imports return already_imported without another graph version;
concurrent submissions retain deterministic fact ids and use normal ingestion writes.
An index refresh failure is imported_index_pending, not evidence of retrieval readiness.

Impact is a read-only proposal: stable concept ids stay unchanged, the previous label
becomes an alias, supersedes names the prior bundle revision, and affected relation
indices are listed. Save the revision field as a new bundle and review it before import.
Import requires the superseded revision in the same scope and records a proposed
supersession claim; it does not silently approve or alter old facts. JSON and escaped
Mermaid views support a full graph or one-hop focus. Bounds are 2 MiB input, 512 nodes,
2048 relations, 512 pins and 32 references per relation; projected snippets are limited
to 64 KiB per pin and 2 MiB total. Larger/binary pins remain explicitly hash_only.

## Repository-scoped delivery status

```bash
relay-knowledge research status --root . --delivery archive --catalog sources/catalog.json --requirements research/requirements.json --format json
relay-knowledge research status --root . --delivery authored_graph --bundle research/bundle.json --scope research --format json
relay-knowledge research status --root . --delivery graphrag --bundle research/bundle.json --scope research --format json
```

Status canonicalizes the requested root and matches registration by that exact root,
never an unrelated default alias. It reports map validity/version, source audit,
bundle validity/import/fact status, repository registration and requested HEAD target,
served scope freshness/content_integrity, and all three graph index versions.
Missing registration or completed indexing is not_indexed; stale and fresh-with-partial
remain separate. Catalog, bundle and requirement errors do not hide other layers.

Archive delivery requires an explicit valid catalog; authored_graph requires a valid
versioned bundle and does not require runtime import. GraphRAG requires the supplied
bundle to be imported with fresh graph indexes, or, without a bundle, this repository's
HEAD code index to be fresh and complete. Rejected/superseded bundle documents do not
qualify. Map-only initialization never satisfies GraphRAG. Status does not register or
index a repository. Offline audit/validate/view/impact need no graph runtime; import,
export and unified status use the configured local runtime.

The optional [requirements Schema](../../../skills/relay-knowledge-cli/references/research-requirements.schema.json)
contains 1–100 explicit id/description/evidence criteria and optional review_claim.
Each binding uses the same path/base/hash contract. review_subject_sha256 binds the
compact JSON tuple [id, description, evidence]; claims about another version require
renewed review. All such locally supplied reviews remain self_reported_unverified.
Readiness is needs_action or ready_for_review, never a semantic completion verdict.
content_verdict remains unknown: file existence, hashes, section headings, author
status and fresh indexes cannot establish that a research question was answered.

## Compatibility and regression coverage

These are local artifact operations on shared application services; imported facts
are available through the existing CLI, HTTP and Web graph/query/lifecycle surfaces.
No HTTP endpoint accepts arbitrary client filesystem roots. No automatic crawling,
external fetching, scope authorization expansion or review impersonation is added.
See [installation and upgrade](../03-architecture-specs/19-installation-release-and-upgrade.md)
for preservation and rollback rules.

Regression fixtures cover ordered batch rollback/replay/concurrency/recovery, raw CRLF,
PDF/shell/redirect/failure captures, identical bytes at different URLs, hash/extractor
drift, confined paths, 27-node/38-relation graph round trips, selective evidence impact,
stable concept clarification, and map-only/unimported/stale/partial/wrong-root states.
The committed graph is synthetic; private research artifacts are not distribution data.

The automatically initialized qualitygate.yaml is a reviewable bootstrap candidate:
it checks line endings only, with no command checks or exclusions. It is not evidence
of team policy adoption and does not replace the existing Cargo, coverage, architecture,
documentation, browser, Miri or sanitizer gates. Batch previews use four bounded workers
and a 30-second deadline; cancellation before publication cannot write candidate state.
