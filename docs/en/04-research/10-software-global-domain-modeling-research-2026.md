# Software Global Domain Modeling Research 2026

[English](../../en/04-research/10-software-global-domain-modeling-research-2026.md) | [中文](../../zh/04-research/10-software-global-domain-modeling-research-2026.md)

> Document version: 1.0
> Prepared: 2026-05-28
> Scope: software-wide knowledge graphs, dependency and SDK versions, generation direction, dynamic evolution, and product roadmap.

## 1. Research Conclusion

The long-term target for `relay-knowledge` should move from a code repository graph toward a global software domain model. Software is not a set of source files. It is a versioned system made from source, builds, dependencies, SDKs, configuration, runtime services, tests, documentation, releases, deployments, vulnerabilities, licenses, generated artifacts, and operational events. The knowledge graph must represent the constraints among these elements and automatically propagate impact when any element changes.

Core conclusions:

- Graph facts are the source of truth; BM25, semantic, vector, code indexes, SBOM views, community summaries, and generation contexts are derived read models.
- SDK versions, dependency versions, generator versions, and target platforms must be first-class facts because they control API surface, generation direction, compiler constraints, compatibility, exposure to vulnerabilities, and rollback boundaries.
- Automatic update is not a query-time repository scan. The reliable path is durable mutation logs, affected scopes, version cursors, persistent tasks, leases, retries, dead letters, fresh/stale/degraded states, and observability.
- Global modeling must not turn missing external source, missing SDK headers, or unauthorized packages into resolved facts. These cases remain unresolved metadata with target hints, resolution state, evidence, and confidence.

## 2. Research Thread

Software engineering knowledge graph surveys frame SE KGs as an integration problem across requirements, design, code, tests, maintenance, defects, and project management. For this project, the lesson is that code graphs are an entry point, not the final boundary.

Code Property Graphs show that combining AST, control flow, and data flow into one graph supports vulnerability discovery and program understanding. Graph4Code similarly organizes functions, classes, calls, data flow, and documentation semantics for machine learning over code. These lines of work show that the competitive advantage comes from structural fusion, not from vectorizing every file.

Recent programming knowledge graph and repository-level code generation papers emphasize that generated code depends on repository context, API constraints, call relations, dependencies, and available library versions. For `relay-knowledge`, generation direction should not be selected only by similar snippets. It should be constrained by SDK/API surface, dependency lock state, feature flags, target platforms, historical changes, and test evidence.

SBOM specifications provide mature supply-chain semantics. CycloneDX covers components, services, dependency graphs, licenses, provenance, and pedigree. SPDX 3.0.1 expands its scope to build information, AI models, datasets, provenance, vulnerabilities, quality data, and lifecycle relationships. A global software model should be compatible with SBOM semantics, but it should go beyond SBOM import/export by linking dependency facts to source imports, build targets, release artifacts, and runtime services.

Dynamic graph and dynamic knowledge graph research shows that nodes, edges, and features evolve over time. Work such as EvolveGCN is useful for future ranking, prediction, and risk scoring, but the product foundation must first solve deterministic versioning, refresh, conflict handling, and recovery.

## 3. Guiding Principles

### 3.1 Model Software Elements, Not Files

The global model should organize software elements:

| Element | Graph responsibility |
| --- | --- |
| Source and symbols | APIs, calls, references, imports, changes, and generation candidates |
| Build system | Targets, features, profiles, platforms, toolchains, and artifacts |
| Dependencies and SDKs | Versions, constraints, transitive dependencies, API surface, vulnerabilities, and licenses |
| Configuration and feature flags | Runtime branches, environment dependencies, deployment variance, and generation conditions |
| Tests and quality | Validation coverage, failure signals, performance baselines, and regression protection |
| Release and deployment | Artifacts, services, upgrades, rollback, runtime state, and diagnostics |
| Documentation and design | Requirements, architecture intent, interface contracts, and behavior explanations |

Files, chunks, and embeddings are evidence carriers, not domain boundaries. Queries and generation should prefer structured graph facts and then use text/vector read models to bridge expression gaps.

### 3.2 Represent Interaction Through Versioned Propagation

Software elements affect each other:

- SDK upgrades can change available APIs, generation templates, compile conditions, vulnerabilities, and test priority.
- Lockfile changes can change SBOMs, license risk, runtime behavior, and patch recommendations.
- Build target changes can change source reachability, conditional compilation paths, and release artifacts.
- Configuration changes can change affected code, test paths, and service diagnostic interpretation.
- Generator changes can change generated files, call shapes, documentation sync, and rollback boundaries.

These interactions must be represented through graph mutation events and derived index refreshes. Query hot paths should only read committed graph facts and freshness-aware read models. If an index lags, the system reports stale or degraded status instead of scanning around the boundary.

### 3.3 Constrain Generation With the Graph

Future code generation and rewrite entry points should extend `AnswerContext` into `GenerationContext`. It should include:

- Current source scope, repository snapshot, target language, and build target.
- SDK, dependency, lockfile, feature flag, and target platform constraints.
- Available API surface, unresolved dependency metadata, and deprecated interfaces to avoid.
- Related symbols, call edges, test evidence, documentation contracts, and historical changes.
- Index freshness, provenance, conflicting facts, and confidence.

Generators should choose a direction only inside those constraints. Missing dependency source should produce unresolved target hints, not accepted edges based on guesses.

## 4. Evolution Direction

1. **Global schema v1**: Add `SoftwareSystem`, `BuildTarget`, `PackageComponent`, `Sdk`, `Generator`, `RuntimeService`, `ReleaseArtifact`, `Vulnerability`, and `License` on top of the code graph.
2. **Dependency and SDK indexes**: Build one dependency read model from manifests, lockfiles, BOMs, build scripts, and import/include facts, with resolved, unresolved, ambiguous, and external states.
3. **Impact propagation tasks**: Make SDK, dependency, configuration, build target, generator, and source changes produce affected scopes and durable refresh tasks using existing leases, retries, and dead letters.
4. **Generation context**: Extend retrieval context packs into pre-generation constraint packs covering API availability, compatibility, test coverage, and risk explanations.
5. **Global quality evaluation**: Track dependency freshness, SDK drift, generation constraint hit rate, impact path recall, SBOM/source alignment, and unresolved edge accuracy.

## 5. References

- Software Engineering Knowledge Graph systematic review. <https://www.sciencedirect.com/science/article/pii/S0950584923001829>
- Yamaguchi et al. "Modeling and Discovering Vulnerabilities with Code Property Graphs." 2014. <https://www.ieee-security.org/TC/SP2014/papers/ModelingandDiscoveringVulnerabilitieswithCodePropertyGraphs.pdf>
- Graph4Code. Semantic Web Journal. <https://www.semantic-web-journal.net/system/files/swj2575.pdf>
- "Context-Augmented Code Generation Using Programming Knowledge Graphs." 2024. <https://arxiv.org/abs/2410.18251>
- "Repository-Level Code Generation with Knowledge Graph." 2025. <https://arxiv.org/abs/2505.14394>
- CycloneDX Specification Overview. <https://cyclonedx.org/specification/overview>
- SPDX Specification 3.0.1 Scope. <https://spdx.github.io/spdx-spec/v3.0.1/scope/>
- "Source-code-based software bill of materials generation." Scientific Reports, 2025. <https://www.nature.com/articles/s41598-025-29762-0>
- "A Survey on Dynamic Knowledge Graphs: Representation Learning and Applications." <https://arxiv.org/abs/2310.04835>
- Pareja et al. "EvolveGCN: Evolving Graph Convolutional Networks for Dynamic Graphs." AAAI 2020. <https://ojs.aaai.org/index.php/AAAI/article/view/5984>

---

Navigation: Previous: [9. GitNexus Feature and UI Implementation Research 2026](09-gitnexus-reference-analysis-2026.md) | Next: [11. Software Global Modeling, CodeGraph, and Search Everything Comparison 2026](11-software-global-codegraph-search-everything-comparison-2026.md)
