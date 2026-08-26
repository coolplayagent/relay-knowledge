# Business Knowledge to Technical Graph Mapping

[English](../../en/03-architecture-specs/27-business-knowledge-technical-mapping.md) | [中文](../../zh/03-architecture-specs/27-business-knowledge-technical-mapping.md)

This chapter defines how repository-authored business knowledge enters the versioned graph shared with code, configuration, and software models. It implements Issue #361 and requires typed, scoped ontology identity instead of treating a label as an entity identity.

## 1. Authority and Data Flow

`.knowledge/knowledge-map.yaml` stores only the `business-knowledge` topic, the `repository-business-glossary` file source, and route order. Business definitions live in version-controlled `.knowledge/business-glossary.yaml`:

```yaml
schema_version: 1
domains:
  - id: revenue
    name: Revenue
terms:
  - id: monthly-recurring-revenue
    domain: revenue
    canonical_name: Monthly Recurring Revenue
    definition: Recurring subscription revenue normalized to one month.
    language: en
    aliases:
      - value: MRR
        kind: abbreviation
    mappings:
      - relation: calculated_from
        target_kind: file
        target: src/billing.rs
```

The fixed flow is `map init → glossary authoring → repo index/update → fenced business projection → repo business/context`. Queries must not scan YAML, start a watcher writer, or persist a second derived snapshot in the map.

`map init` idempotently adds the route and a minimal valid glossary. It validates and preserves an existing glossary. A reserved source, route, URI, type, or scope conflict fails instead of overwriting data. Creating only a missing glossary does not increment map version.

## 2. Schema, Identity, and Bounds

Ontology identity is `(repository source scope, domain_id, term_id, entity_kind)`. Domains and terms use `business_domain` and `business_term` typed identities, so display-name changes do not change IDs. Existing label-only entities remain `untyped` and are not rewritten during upgrade.

The same name may exist in different domains. An exact canonical-name or alias query without a domain returns `ambiguous` when multiple domains match. Route source order selects preferred display only; competing definitions remain visible with conflicts and separate evidence.

Schema v1 supports synonym and abbreviation aliases; non-executable formula, aggregation, unit, grain, time-basis, includes, and excludes semantics; and `represented_by` or `calculated_from` mappings. Target kinds cover file, symbol, config key, API, software component, build target, IaC, design element, database table/column, metric, and external.

Hard limits are 4 MiB per file, 256 domains, 10,000 terms, 32 aliases and 64 mappings per term, 128-byte IDs, 1,024-byte names/aliases/targets, and 32-KiB definitions/formulas. Every string and collection is validated before storage.

## 3. Projection, Resolution, and Publication Fence

The repository indexer reads only active repository-scoped files authorized by the current route at the same immutable Git commit. A v2 topic shard must pass manifest digest, identity, and source-order validation. Absolute paths, parent traversal, backslashes, missing blobs, oversized content, and invalid schema fail the durable attempt. A live non-Git filesystem glossary is not represented as a committed business fact.

Business, code, and software projections share one durable task, lease, attempt, and publication fence. The storage boundary owns business reads and writes through a dedicated `BusinessKnowledgeStore` contract instead of enlarging the code-store contract. Code facts are staged first, followed by the business projection and software projection; one transaction then publishes business/software status and the code scope as fresh. An old lease or mismatched target cannot perform the replacement, and receipts or fast paths cannot report freshness without a matching fresh business status.

Mapping resolution queries only indexed tables in the same authorized source scope. Files, symbols, config keys, APIs, software components, build targets, IaC resources, and design elements can resolve exactly. Database table/column, metric, external, or other uncovered targets retain `resolution_state=unresolved` and `target_hint`. Missing external coverage never sets repository or parser degradation.

Every accepted definition and mapping returns source ID/path/digest, resolved commit, confidence, lifecycle, and valid graph-version range. Scope retirement, repository removal, and shard cleanup delete business tables. Runtime backup and restore treat the control database and all repository shards as one state set.

## 4. Unified Query and Context

The shared request carries a repository selector, fixed ref, domain, query, `terms|mappings|all`, freshness policy, and limit. The unified interfaces are:

```bash
relay-knowledge repo business <alias> --kind all --query MRR --domain revenue --ref <commit> --freshness wait-until-fresh --format json
```

- HTTP: `POST /api/v1/code/repositories/{alias}/business`
- MCP: `relay_business_query`
- Web: read-only terms and mappings; authoring remains a reviewed glossary-file change.

`repo context` resolves canonical names and aliases in the same pinned scope, then uses mapping resolved IDs or target hints as bounded code seeds. `business_context` and code results share the commit/source scope and participate in candidate, limit, byte-budget, truncation, and provenance accounting.

`repo view --kind business-domains` merges declared domains before route, feature-flag, and path inference. Evidence kind `business_glossary` distinguishes authored definitions from `route`, `feature_flag`, and `path` inference.

## 5. Upgrade, Rollback, and Acceptance

Opening an older runtime database adds typed entity-identity columns and business projection tables. Existing code/software facts and label-only entities are not rewritten. An old scope without a fresh business status cannot use the full-index fast path and must rebuild from Git authority through normal `repo index` or `repo update`. Binary-only rollback may ignore the new tables but cannot serve the new projection; exact rollback requires a transaction-consistent pre-upgrade backup of the control database and every shard.

Acceptance covers map initialization and upgrade, path/digest/schema bounds, homonyms, acronyms, competing definitions, fenced publication and replay, stale repair, resolved and unresolved mappings, canonical exact retrieval, business-to-code context, declared-domain views, and a fixed-commit end-to-end loop. Formula execution, OWL/RDF inference, external Wiki/database ingestion, and Web glossary editing are outside v1.

---

Navigation: [Architecture Specifications](README.md) | Previous: [26. Git Commit + Knowledge](26-git-commit-knowledge-development-loop.md) | Next: [Documentation bookshelf](../README.md)
