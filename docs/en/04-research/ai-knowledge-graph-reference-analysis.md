# ai-knowledge-graph Reference Analysis

[English](../../en/04-research/ai-knowledge-graph-reference-analysis.md) | [中文](../../zh/04-research/ai-knowledge-graph-reference-analysis.md)

> Date: 2026-05-15
> Reference project: <https://github.com/robert-mcdermott/ai-knowledge-graph>
> Reviewed revision: `40b7019`, 2025-12-27, `Merge pull request #19 from Deepak-png981/dj/Introduce-prompt-factory`
> Scope: architecture, algorithm, performance, and reliability lessons only. This document does not introduce implementation changes or copy source code from the reference project.

## 1. Executive Summary

`ai-knowledge-graph` is a Python single-machine pipeline: it reads a text file, chunks it by word count, uses an OpenAI-compatible Chat Completions endpoint to extract Subject-Predicate-Object triples, standardizes entities, infers relationships, and renders an interactive HTML graph with NetworkX, Louvain community detection, and PyVis. It is useful as a minimal closed loop for LLM-extracted knowledge graphs, but its implementation style should not be transplanted directly into `relay-knowledge`.

The useful lessons fall into four areas:

- Architecture: the pipeline has explicit phase boundaries: chunking, SPO extraction, entity standardization, relationship inference, and visualization/export.
- Algorithms: entity standardization and relationship inference follow a practical pattern of deterministic candidate generation plus optional LLM adjudication.
- Performance: chunk budgets, candidate caps, representative entity sampling, context truncation, and original/inferred edge separation show that GraphRAG pipelines need budgets before model calls or graph traversal.
- Reliability: the script also exposes risks: synchronous network calls, no timeout/retry policy, no durable task state, repair-based JSON parsing, and inferred edges without evidence or confidence. `relay-knowledge` should turn those risks into architecture constraints instead of copying the script behavior.

## 2. Reference Pipeline

The reference project main path is in `src/knowledge_graph/main.py`:

1. `load_config` reads LLM, chunking, standardization, inference, and visualization settings from `config.toml`.
2. `chunk_text` splits input by word count and keeps fixed overlap.
3. `process_with_llm` obtains prompts from a prompt factory, calls the LLM, and tries to extract a JSON array from the response.
4. Each valid object must contain `subject`, `predicate`, and `object`; predicates are trimmed to at most three words.
5. `standardize_entities` first applies deterministic grouping with lowercasing, stopword removal, containment, and short-root heuristics, then can call an LLM to group entity aliases.
6. `infer_relationships` identifies connected components, calls an LLM for inter-community and intra-community candidates, and adds transitive and lexical-similarity inference.
7. `_deduplicate_triples` deduplicates by triple and prefers non-inferred relationships.
8. `visualize_knowledge_graph` calculates degree, betweenness, eigenvector centrality, community ids, node sizes, and inferred-edge styling, then writes HTML and JSON output.

This proves the value of a low-cost closed loop: unstructured text can quickly become an inspectable graph. Its engineering assumptions, however, are local batch-script assumptions. They do not match this repository's async-first, service-first, recoverable indexing, and shared application-service architecture.

## 3. Architecture Lessons

### 3.1 Keep the Phase Semantics, Not the Script Boundary

The reference project makes extraction, standardization, inference, and visualization explicit phases. `relay-knowledge` can adopt that product language while preserving existing boundaries:

- `application` coordinates ingestion, proposals, index refresh, and diagnostics.
- `domain` owns evidence, entities, relations, claims, events, inferred/derived state, source spans, confidence, and graph versions.
- `storage` persists raw evidence, accepted facts, proposals, derived indexes, and mutation logs.
- `net` remains the boundary for HTTP and external provider communication; LLM, embedding, OCR, and other network calls must not bypass `net` and QoS.

There should be no separate script-style "generate graph" path. CLI, Web, MCP, and ACP should continue to share the same application service.

### 3.2 Model-Assisted Phases Should Be Policy

The reference project can disable `standardization` and `inference`, which is an important product capability. In `relay-knowledge`, this should become policy:

- `disabled`: no model call, only raw facts and deterministic indexes.
- `candidate-only`: deterministic candidate generation writes proposals, diagnostics, or review queue entries.
- `assisted`: an external model generates entity-merge or inferred-relation proposals with provider, model, prompt version, source hash, scope, confidence, and review state.

These policies belong in configuration, API responses, and diagnostics. They should not remain hidden inside one-off command flags.

### 3.3 Prompt Factory Implies Versioned Contracts

The reference project separates prompts for main extraction, entity resolution, and relationship inference. `relay-knowledge` does not need to copy prompt text, but it should treat prompts as auditable contracts:

- prompt id, prompt version, model, temperature, input evidence hash, and output schema version should be provenance on derived output.
- prompt upgrades should be able to trigger scoped proposal/index refresh instead of silently overwriting old results.
- raw LLM responses may be saved as diagnostics or audit material, but not as accepted facts.

### 3.4 Original and Inferred Edges Must Stay Layered

The reference project marks inferred relationships with `inferred` and displays them with dashed edges. `relay-knowledge` already has evidence/status/version foundations and should strengthen this distinction:

- extracted facts, normalized entity aliases, inferred relations, community summaries, and generated answers are separate layers.
- inferred edges are not accepted facts by default; they belong in derived facts or proposals.
- query responses must explain relationship origin: source evidence, rule inference, LLM inference, community summary, or index derivation.

## 4. Algorithm Lessons

### 4.1 SPO Extraction Is a Candidate Fact Source

The reference project asks the LLM to return `{subject, predicate, object}` JSON arrays and validates field presence. For `relay-knowledge`, this is useful as an input shape, but it needs candidate-fact semantics:

- each candidate must cite source scope, source URI/hash, chunk id, source span, extractor, model, and prompt version.
- predicates should not only be trimmed to three words; they should map to typed relations, aliases, or unnormalized labels.
- candidates need status: proposed, accepted, rejected, superseded, or derived.
- repeated extraction over the same chunk should be idempotent by source hash, chunk range, model, and prompt version.

### 4.2 Entity Standardization Should Be Two-Stage

The reference project first groups by lowercasing, stopword removal, word containment, and root heuristics, then optionally asks an LLM to group entity names. The reusable shape is:

- deterministic logic only creates alias-group candidates and similarity explanations.
- the LLM adjudicates high-value or ambiguous candidates instead of scanning every entity.
- high-frequency entity priority, batch-size caps, and context-length caps should become explicit budgets.
- entity merge must be reversible: canonical id, alias, scope, and query-impact changes need auditability.

Directly lowercasing and overwriting entity names is not acceptable for people, project names, code symbols, paths, acronyms, or multilingual entities. The original display label must be preserved.

### 4.3 Relationship Inference Should Remain Explainable

The reference project uses four inference sources: inter-community LLM inference, intra-community LLM inference, two-hop transitive inference, and lexical-similarity inference. `relay-knowledge` can use these as candidate generators:

- inter-community inference can find graph fragmentation points, but must cap top-k communities, representative entities, and evidence context.
- intra-community inference can fill semantic neighbors, but shared words must not become facts automatically.
- transitive inference should only run for a predicate whitelist with clear transitive semantics, such as constrained forms of `part_of`, `located_in`, or `depends_on`.
- lexical similarity should produce `possibly_related` or entity-alias proposals, not accepted factual edges.

Every inferred result needs `inferred_by`, input facts, rule id or prompt id, confidence, review state, and stale invalidation conditions.

### 4.4 Graph Metrics Belong in Read Models and Diagnostics

The reference project calculates degree, betweenness, eigenvector centrality, and Louvain communities, then uses node sizes, colors, and dashed edges to explain graph structure. `relay-knowledge` can reuse this idea in two places:

- retrieval ranking: centrality, community membership, and bridge-node status can be rerank signals.
- UI and diagnostics: graph inspection, Web canvas, and agent context packs can show community, original/inferred edge status, node importance, and truncation reason.

These metrics are read-model or diagnostic output, not domain facts. They must be invalidated by graph version and scope.

## 5. Performance Lessons

The reference project already contains implicit performance controls: fixed chunk size/overlap, LLM entity resolution capped to 100 entities, only the five largest communities, at most five representative entities per community, twenty context triples, and ten intra-community candidate pairs. These should become explicit resource budgets:

- ingestion chunking should account for tokens, sentence boundaries, source spans, and overlap, not only whitespace words.
- LLM extraction, entity resolution, and inference need bounded concurrency, request timeouts, cancellation, retry backoff, provider rate limits, and dead-letter handling.
- graph traversal, community detection, centrality, and full index rebuilds must not run in the query hot path.
- community summaries, centrality, and relation inference should be scoped read-model or maintenance-worker output.
- context-pack APIs should expose `limit`, `timeout`, `truncated`, `degraded`, `budget_exhausted`, and retriever source.
- LLM outputs should be cached or reused for identical source hash, chunk range, model, and prompt version.

The reference project's synchronous `requests.post`, serial chunk processing, and missing timeout policy are not suitable for `relay-knowledge`. External model calls must run behind worker boundaries and must not block async runtime executors.

## 6. Reliability Lessons

The reference project has useful fault-tolerance instincts: per-chunk failure isolation, invalid-triple filtering, JSON recovery from code blocks or incomplete responses, raw JSON export, and separate original/inferred edge representation. In `relay-knowledge`, these should become stricter reliability design:

- LLM response parsing must use schema validation; repaired JSON can enter degraded proposals but must not be accepted automatically.
- each chunk's success, failure, retry, model, prompt, token budget, and error kind should enter structured diagnostics.
- stage output must be recoverable: extraction, standardization, inference, index refresh, and visualization/read models each need cursors or leases.
- failure isolation should be recorded by scope, source, chunk, provider, and stage, so one failed chunk or model does not block other sources.
- secrets must not be committed in example `config.toml`; API keys must flow through `env` and redacted diagnostics.
- inferred results must be reversible, and graph mutation, proposal acceptance, and index invalidation must remain consistent.

The reference example logs can report inconsistent added-inference counts before later reporting final totals. That illustrates why diagnostics cannot rely on `print`; `relay-knowledge` counts need durable task state and tested stage results.

## 7. relay-knowledge Follow-Up Reference

If these lessons are later implemented, they should enter specs and tracked work in this order. This document does not implement them:

1. Define LLM SPO extraction as a `proposal` producer, not a direct graph commit path.
2. Add deterministic entity-resolution candidates, LLM adjudication workers, review state, and reversible merge audit.
3. Add rule/prompt provenance, confidence, input fact ids, stale conditions, and default non-accepted state for inferred relations.
4. Add community, centrality, bridge-node, and original/inferred edge metadata to read models plus Web/agent diagnostics.
5. Standardize provider timeout, QoS, rate limit, retry, dead-letter, cursor, and prompt-version metadata across extraction, standardization, and inference workers.
6. Extend GraphRAG evaluation fixtures with entity merge, false merge, transitive-rule whitelist, inferred-edge rejection, and degraded JSON response scenarios.

### 2026-05-15 Selective Adoption Progress

This round only adopts the parts that fit the existing worker/proposal path. It
does not change normal `ingest` behavior for directly submitted accepted facts
and does not add a separate script-style extraction entry point:

- Proposals now persist provenance metadata: producer, provider, model, prompt
  id/version, schema version, input source hash, input fact ids, stale
  conditions, and budget notes.
- `worker run-once` sends external endpoints a `contract_version=2` request with
  manual-review policy, HTTP timeout, lease, max-attempt, and max-in-flight
  budgets.
- Structured relation/claim/event facts returned by an external extractor remain
  in the proposal payload but are normalized to `proposed`, so LLM SPO
  extraction or relationship inference cannot write accepted facts directly.
- Existing deterministic fallback proposals, OCR/vision/embedding behavior,
  worker queues, manual accept/reject/supersede, audit sink, index refresh, and
  GraphRAG retrieval behavior remain intact.

The remaining items stay future work: reversible entity-merge audit,
transitive-rule whitelists, community/centrality read models, full provider
rate-limit/cursor handling, and larger GraphRAG evaluation fixtures.

## 8. What Not to Copy

- Do not copy Python source, prompt text, HTML templates, or PyVis output.
- Do not add a script-style main flow that bypasses `env`, `paths`, `net`, `application`, or `storage`.
- Do not write LLM-inferred relationships directly into accepted facts.
- Do not overwrite canonical display labels with lowercased entity names.
- Do not run external LLM calls, whole-graph community detection, centrality, or full index rebuilds in the query hot path.
- Do not use synchronous HTTP model calls without timeouts.

## 9. Documentation Impact

This analysis should be cited as research material. Before implementation work starts, the corresponding specs need updates:

- GraphRAG product roadmap: define SPO proposal, entity resolution, and inferred-relation stage state.
- Source Scope and multimodal ingestion: define chunk, source span, extractor provenance, and provider diagnostics.
- Background service/self-healing: add lease, retry, and dead-letter requirements for extraction, standardization, and inference workers.
- Advanced observability: add prompt version, model, provider latency, schema validation failure, and degraded proposal metrics.

As of 2026-05-15, this analysis has started to feed the worker/proposal path selectively; unfinished items still need separate architecture-backed implementation work.
