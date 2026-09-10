# Multimodal Evidence Ingestion

[English](../../en/03-architecture-specs/05-multimodal-evidence-ingestion.md) | [中文](../../zh/03-architecture-specs/05-multimodal-evidence-ingestion.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Ingestion does not flatten everything into plain text. It converts different modalities into one evidence contract. Text, images, OCR, captions, tables, layout regions, and code snippets retain source, parent-child relationships, confidence, and extraction state.

## 2. Unified Evidence Model

Evidence expresses at least source scope, source path, span or asset region, modality, content hash, parent evidence, extraction method, confidence, lifecycle status, and created graph version.

Derived evidence references parent evidence:

```text
image asset
  -> OCR text evidence
  -> caption evidence
  -> layout/table region evidence
  -> image embedding metadata
```

Retrieval groups derived hits by parent evidence so OCR, caption, and embedding hits from the same image do not become duplicate context items.

## 3. Ingestion Pipeline

```text
source discovery
  -> scope normalization
  -> evidence write
  -> extraction task enqueue
  -> worker extraction
  -> proposal or derived evidence commit
  -> mutation log
  -> index refresh request
```

Ingestion creates raw evidence and bounded background tasks. OCR, captions, embeddings, table extraction, and large parsing do not run on query hot paths.

## 4. Worker Boundary

Worker tasks carry kind, scope, input evidence id, attempt, lease, timeout, budget, redacted config snapshot, and output contract. When an external model or OCR service fails, the task retries or dead-letters; it cannot write half-structured facts that bypass validation.

## 5. Deduplication and Versioning

- Content hashes identify duplicate source payloads.
- Extraction output hashes prevent duplicate derived evidence commits.
- Evidence lifecycle supports proposed, accepted, rejected, and superseded.
- New extraction output does not overwrite old evidence; it appends a version or records a supersedes relation.

## 6. Acceptance Criteria

- Derived evidence can be traced to the original source and worker attempt.
- Retrieval groups multimodal hits by parent evidence.
- External extraction failures do not block existing text or graph retrieval.

---

Navigation: Previous: [4. Source Scope Model](04-source-scope-model.md) | Next: [6. Graph Fact Model and Versioning](06-graph-fact-model-and-versioning.md)

Python overload binding proofs stop at `await` and yielding operations because protocol dispatch or suspension can change a provider before its decorator executes. Delegating to a syntactically empty built-in tuple, list, or dictionary has neither dispatch nor suspension. Match capture names bind their lexical target; dotted value references, mapping keys, class names, and keyword labels do not. A direct zero-argument call freezes a deferred decorator lookup only when a bounded proof establishes synchronous execution along a straight path to that specific decorator. Async functions, generators, conditional paths, and preceding exits cannot establish that proof; yields inside an uncalled nested function do not turn its enclosing function into a generator. All three analyses consume the shared proof budget.

The first synchronous invocation is insufficient if later execution can invoke the function again, including through aliases. Freezing at that call additionally requires a bounded tail containing only proven nonexecuting linear statements; later calls, control flow, or imports invalidate the shortcut.

Direct-invocation evidence also checks imports before the decorator: an import can execute module code that replaces its provider. Deferred imports inside an uncalled nested function stay outside that execution path. If the direct invocation is an assignment value, the shortcut requires an unannotated plain-name target; attribute and subscript stores can invoke setters after the call and re-enter the function with a different decorator provider, while destructuring and annotations carry unproven effects. These uncertain cases retain executable classification and canonical ambiguity, with each symbol snapshot preserving its own call edges. The checks share the existing proof budget.

C/C++ callable signature keys normalize standard builtin specifiers structurally: optional `int`, explicit `signed` for integer types, and legal specifier order do not separate equivalent declarations (`unsigned`/`unsigned int`, `long`/`long int`, and `long double`/`double long`). This normalization consumes the existing per-callable and shared per-file work budgets and preserves displayed signatures. It never equates different integer ranks or signedness, plain `char` with signed or unsigned `char`, floating types, or differing pointer/reference/member qualifiers based on platform widths. Typedefs, macro-dependent tokens, array adjustments, unsupported declarator forms, and exhausted evidence still produce an unknown key and require an explicit symbol snapshot for ambiguous canonical requests. Real Git regression fixtures preserve declaration-owned callers and exact snapshot selection across both equivalent and distinct builtin types; previous completed scopes are rebuilt under the new C/C++ source-fact component before these keys are used.
