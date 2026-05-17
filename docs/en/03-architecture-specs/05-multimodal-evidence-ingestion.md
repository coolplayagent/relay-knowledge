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
