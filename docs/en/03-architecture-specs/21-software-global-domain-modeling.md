# Software Global Domain Modeling Architecture

[English](../../en/03-architecture-specs/21-software-global-domain-modeling.md) | [中文](../../zh/03-architecture-specs/21-software-global-domain-modeling.md)

> Document version: 1.0
> Prepared: 2026-05-28
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Software global domain modeling brings source graphs, dependency graphs, build graphs, configuration graphs, test graphs, release graphs, and runtime diagnostics into one versioned fact space. It does not replace the current code knowledge graph. It extends `repository_snapshot`, source scopes, mutation logs, derived-index freshness, and background recovery with software-lifecycle elements.

The design must satisfy four constraints:

- Base facts remain partitioned by real source scope; global views must not copy or merge single-repository code facts.
- SDKs, dependencies, build targets, generators, configuration, tests, and release artifacts are first-class entities, not code chunk attributes.
- All change propagation must go through durable graph mutations and bounded refresh tasks, not query-time recursive scans of repositories, package caches, or SDK directories.
- Missing external source, unauthorized dependencies, and missing SDKs produce unresolved edge metadata, never resolved graph facts.

## 2. Core Model

The global model adds these entity families on top of `CodeRepository`, `CodeFile`, `CodeSymbol`, `CodeChunk`, and `CodeChangeSet`:

| Entity | Responsibility |
| --- | --- |
| `SoftwareSystem` | Stable product, service, or tool identity |
| `BuildTarget` | Build entry point, profile, platform, feature set, and output artifact |
| `PackageComponent` | Package, module, library, container image, or third-party component |
| `Sdk` | Platform SDK, compiler, system header set, language runtime, or generated SDK |
| `Generator` | Code generator, schema compiler, IDL compiler, or template engine |
| `Configuration` | Environment variable, config key, feature flag, or deployment parameter |
| `RuntimeService` | Installed service, HTTP endpoint, worker, or external service dependency |
| `TestCase` | Unit, integration, browser, performance, or acceptance test entry |
| `DeploymentUnit` | Service definition, container, package-manager install unit, or platform deployment unit |
| `ReleaseArtifact` | Binary, archive, installer, SBOM, checksum, or release note |
| `Vulnerability` | Vulnerability, weakness, affected version range, and fix advice |
| `License` | License, exception, attribution, and compliance constraint |
| `DocumentationUnit` | Requirement, design, interface, operations, or release document |

Entity keys must bind stable identity and scope. Dependency packages and SDK versions must not use a plain name as identity; they need at least ecosystem, name, version or range, source authority, and scope.

## 3. Relationship Model

The global relationship set includes:

| Relationship | Meaning |
| --- | --- |
| `depends_on` | Direct or transitive dependency |
| `uses_sdk` | Source, build target, or generator depends on an SDK/API surface |
| `generates` / `generated_from` | Generator, schema, template, and generated file relationship |
| `builds` | Build target produces an artifact |
| `packages` | Artifact contains a component, file, or SBOM |
| `configures` | Configuration affects a service, build, or code path |
| `deploys` | Deployment unit installs or starts a runtime service |
| `tests` | Test covers a symbol, configuration, service, or artifact |
| `documents` | Documentation explains an entity, relationship, behavior, or constraint |
| `exposes_api` | SDK, package, service, or symbol exposes an API surface |
| `affects` | Change, vulnerability, configuration, or dependency affects another element |
| `constrains_generation` | SDK, dependency, configuration, platform, or document constrains generation direction |
| `supersedes` | Version, artifact, configuration, or fact replaces an older one |

Every relationship carries `source_scope`, `graph_version`, `resolution_state`, `confidence`, `evidence_refs`, `valid_from`, and `valid_to`. Cross-repository, cross-package, and cross-SDK relationships also expose target hints and resolution basis.

## 4. Change Propagation

Global updates use the same event chain:

```text
source or manifest changed
  -> evidence extracted
  -> candidate software facts produced
  -> graph mutation committed
  -> affected scopes recorded
  -> dependency/sdk/build/test/retrieval refresh tasks enqueued
  -> read model cursors advanced or stale/degraded diagnostics recorded
```

Propagation rules:

- Manifests, lockfiles, BOMs, build scripts, SDK metadata, and import/include facts can trigger dependency refresh.
- SDK or generator version changes affect generation context, API surface read models, and related test suggestions.
- Build target changes affect reachable source, conditional compilation, release artifacts, and deployment units.
- Configuration changes affect guarded code, runtime service diagnostics, and test selection.
- Worker failures change only derived-index state and dead-letter records; they do not roll back committed graph facts.

## 5. Retrieval and Generation Context

Global retrieval continues to fuse BM25, semantic, vector, and graph-path signals, but candidates and explanations must include lifecycle elements. A generation-oriented context pack should include:

- Current repository snapshot, build target, target platform, and language.
- Dependency, SDK, lockfile, SBOM, feature flag, and generator version constraints.
- Available API surface, deprecated APIs, unresolved external targets, and evidence.
- Related code symbols, tests, documents, release artifacts, runtime diagnostics, and impact paths.
- Read-model freshness, conflicting facts, confidence, and degradation reasons.

If these constraints are missing, generation entry points must expose the gap as risk instead of widening authorization or scanning unindexed directories.

## 6. Acceptance Criteria

- SDK or dependency version changes produce affected scopes and drive derived read-model refresh or stale diagnostics.
- Generation context explains the SDK, dependency, build target, configuration, test, and documentation evidence it uses.
- SBOM dependencies and source import/include facts can be linked, while unauthorized external dependencies remain unresolved.
- Query, CLI, Web, and agent context packs expose freshness, resolution state, and provenance for global elements.
- The global model does not copy single-repository code facts or weaken repository snapshots as the minimum code-fact partition.

## 7. Initial Implementation Slice

The first foundation slice remains bounded by repository snapshot/source scope and projects existing code-index facts into a software global read model:

- `software_components` is derived from `code_repository_dependencies`, separates manifest `declared` and lockfile `locked` relationships, and preserves ecosystem, package name, requirement, resolved version, dependency group, evidence path, and line range.
- `software_dependency_usages` links declared dependency components to matching code/config import evidence when the module root matches the package identity, preserving import `resolution_state`, `target_hint`, evidence path, and confidence without resolving unauthorized package source.
- `software_sdk_usages` is derived from unresolved, ambiguous, or external `code_repository_imports` so SDK/API-surface usage candidates retain `resolution_state` and `target_hint` without resolving unauthorized external source.
- `software_global_status` records projected graph version, stale state, component count, SDK usage count, and the last projection error for each source scope.
- CLI exposes the projection through `relay-knowledge repo software <alias> --kind dependencies|sdks|all`; query hot paths read committed projection rows and do not scan package caches, SDK directories, or the full repository.

---

Navigation: Previous: [20. Multi-Repository Code Graph Overlay](20-multi-repository-code-graph-overlay.md)
