# Multimodal Evidence Capability

[English](./07-multimodal-evidence-capability.md) | [中文](../../zh/02-capabilities/07-multimodal-evidence-capability.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Multimodal capability keeps evidence from being limited to plain text. The system can record extraction metadata for text spans, image assets, OCR text, captions, image embeddings, tables, and layout regions.

## User-visible Behavior

- Derived OCR, caption, and image embedding hits are grouped by parent evidence into one context item.
- Background or maintenance workers commit OCR, caption, table, layout, and image embedding output through `commit_multimodal_extraction`.
- Query hot paths do not run OCR, captioning, embedding, or large extraction work.

## Competitive Features

Ordinary RAG often treats OCR text as a normal chunk and loses image, table, and layout provenance. This capability preserves parent evidence, modality, extractor, confidence, and derived metadata so context is both retrievable and explainable.

## Command/API Entry Points

```bash
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
```

## Degradation and Diagnostics

When external OCR, vision, or embedding providers are unavailable, text evidence, BM25, graph paths, and existing derived evidence remain searchable. Worker failure retries or dead-letters and does not block query.

## Related Architecture Chapters

- [Multimodal Evidence Ingestion](../03-architecture-specs/05-multimodal-evidence-ingestion.md)
- [Semantic/Vector Provider Architecture](../03-architecture-specs/10-semantic-vector-provider-architecture.md)

---

Navigation: Previous: [6. Freshness and Index Recovery](06-freshness-and-index-recovery.md) | Next: [8. Code Repository Basics](08-code-repository-basics.md)
