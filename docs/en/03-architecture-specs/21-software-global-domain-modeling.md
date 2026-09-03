# Software Global Domain Modeling Architecture

[English](../../en/03-architecture-specs/21-software-global-domain-modeling.md) | [中文](../../zh/03-architecture-specs/21-software-global-domain-modeling.md)

> Document version: 1.4
> Prepared: 2026-09-02
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Software global domain modeling brings source graphs, dependency graphs, build graphs, configuration graphs, test graphs, release graphs, and runtime diagnostics into one versioned fact space. A code repository is a self-describing evidence boundary for design and delivery state, not a closed truth boundary for every software fact. Authorized runtime events, deployment observations, organization catalogs, and external dependency metadata may enter the model, but they retain source, observation time, authorization scope, and conflict state.

The runtime remains a SQLite property graph with materialized read models; this design does not migrate primary storage to RDF triples. A versioned ontology contract, shape validator, and SPDX, CycloneDX, and PROV-O export mappings sit above that runtime. Standards mappings provide interoperability and never become a source for unevidenced facts.

The design must satisfy four constraints:

- Base facts remain partitioned by real source scope; global views must not copy or merge single-repository code facts.
- SDKs, dependencies, build targets, generators, configuration, tests, and release artifacts are first-class entities, not code chunk attributes.
- All change propagation must go through durable graph mutations and bounded refresh tasks, not query-time recursive scans of repositories, package caches, or SDK directories.
- Missing external source, unauthorized dependencies, and missing SDKs produce unresolved edge metadata, never resolved graph facts.

## 2. Core Model

The ontology contract has four layers:

| Layer | Current controlled types or responsibility |
| --- | --- |
| Stable entities | `Domain`, `SoftwareSystem`, `Component`, `Api`, `Resource`, `Configuration`, `BuildDefinition`, `DeploymentUnit`, `RuntimeService`, `TestCase`, `ReleaseArtifact`, `PackageComponent`, `Sdk`, `DocumentationUnit`, `Pipeline`, and `BuildJob` |
| Snapshot and event instances | `RepositorySnapshot`, `FileRevision`, `BuildRun`, `DeploymentRevision`, and `RuntimeObservation` |
| Traceable assertions | First-class `SoftwareStatement` records with subject, predicate, object, source, evidence, extractor, time, confidence, resolution, and fact state |
| Derived read models | Compatible dependency, SDK, file, topic, relationship, build, IaC, and design projections plus new typed query slices; they are rebuildable rather than authoritative facts |

The Issue #362 core-module boundary sits between this data model and storage implementations. `domain::core::ontology` defines the bounded, storage-independent `OntologySchema`, class identity, RDF local names, OWL object properties, and executable domain/range relation shapes. `domain::operations::software::vocabulary` registers and directly exports the `SOFTWARE_ONTOLOGY_SCHEMA` catalog with 21 classes, 15 object properties, `1.0.0` version, and `https://relay-knowledge.dev/ontology/software/1#` namespace. Entity/statement construction, shape validation, SQLite materialization, and PROV-O JSON-LD export consume this one catalog instead of maintaining separate namespaces or domain/range matches.

Core schema validation parses namespace IRIs and requires an absolute HTTP(S) scheme plus a valid host, then checks schema/version identity, class/property capacities, RDF local names, identity uniqueness, nonempty relation shapes, and every class reference before projection publication. Executable relation checks reject subject or object class ids absent from the catalog even when a shape uses `Any`. Current software predicates are OWL object properties, so a literal object is retained as a rejected statement with a `literal_object_for_object_property` shape diagnostic instead of bypassing entity range validation. The schema describes the ontology contract only: it reads no files, performs no network I/O, owns no graph storage, and does not replace the LPG runtime with an RDF triple store.

A stable entity `entity_key` is derived from repository, controlled type, namespace, and normalized name without a commit or source scope, so one entity retains identity across commits. Each evidence observation has a separate `occurrence_id` bound to the entity key, source scope, and evidence ids. Snapshot and event instance kinds intentionally include source scope in their identity. Dependency and SDK versions, requirements, ecosystems, and authorities live in occurrence attributes and statements; a display name alone never proves a resolved version.

`SoftwareStatement` includes at least `statement_id`, `subject_id`, `predicate`, mutually exclusive `object_id`/`object_value`, `source_scope`, `source_kind`, `evidence_refs`, `assertion_mode`, `resolution_state`, `valid_from`, `valid_to`, `observed_at`, `extractor_id`, `extractor_version`, `confidence_basis_points`, and `fact_state`. Assertion modes are `declared|extracted|observed|verified|inferred`; resolution states are `resolved|unresolved|ambiguous|external|conflicting`; fact states are `active|conflicting|superseded|rejected`.

## 3. Relationship Model

Ontology statements use a closed predicate vocabulary. Compatibility names such as legacy projection `uses_sdk` do not extend this vocabulary:

| Relationship | Meaning |
| --- | --- |
| `depends_on` | Direct or transitive dependency |
| `contains` | A system, component, artifact, or deployment contains another entity |
| `provides_api` / `consumes_api` | A component, service, SDK, or code surface provides or consumes an API |
| `builds` / `produces` | A build definition processes inputs and produces an artifact |
| `packages` | Artifact contains a component, file, or SBOM |
| `configures` | Configuration affects a service, build, or code path |
| `deploys` | Deployment unit installs or starts a runtime service |
| `runs_as` | A deployment or component corresponds to a runtime service |
| `tests` | Test covers a symbol, configuration, service, or artifact |
| `documents` | Documentation explains an entity, relationship, behavior, or constraint |
| `derived_from` / `observed_as` | Source relationships among snapshots, artifacts, deployment revisions, and runtime observations |
| `supersedes` | Version, artifact, configuration, or fact replaces an older one |

Each predicate has an authority policy instead of inheriting a global source priority. Manifests declare dependency requirements while lockfiles, SBOMs, or build attestations support resolved dependencies. Build files and CI declare build design while attestations support results. IaC and service definitions declare desired deployment state while authorized runtime or connector input records observations. Machine-readable API schemas and code support API contracts. Contradictory sources coexist as `conflicting` statements and are never silently overwritten.

The shape validator checks evidence and extractor completeness, subject/object cardinality, validity intervals, observed timestamps, confidence range, stable identity, cross-scope evidence and references, and predicate domain/range before publication. Failures become queryable `software_ontology_diagnostics`; a non-conforming statement remains `rejected` and cannot become an accepted fact.

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

- `software_components` is derived from `code_repository_dependencies`, separates manifest `declared` and lockfile `locked` relationships, and preserves ecosystem, package name, requirement, resolved version, dependency group, evidence path, and line range. Declared rows remain evidence-specific because their manifest directory owns dependency-usage matching. Repeated locked rows are coalesced only in this derived model by the repository/scope-level semantic key `(ecosystem, package_name, requirement, resolved_version, dependency_group, source_kind, language_id)`; the first `(evidence_path, line_start, line_end)` in deterministic order is the representative. The resulting projection remains capped at 65,536 components and rejects the first distinct semantic component beyond that cap. Authoritative `code_repository_dependencies` rows and `repo query --kind sbom` evidence are not deleted or coalesced.
- `software_dependency_usages` links declared dependency components to matching code/config import evidence when the module root matches the package identity, preserving import `resolution_state`, `target_hint`, evidence path, and confidence without resolving unauthorized package source. Imports from generated files remain available as code and SDK facts but do not enter this derived dependency matcher. Each import retains a 32 KiB matching-input bound: identical module and target-hint text is charged and scanned once, while distinct text remains cumulative and an overflow fails and rolls back the projection transaction.
- `software_sdk_usages` is derived from unresolved, ambiguous, or external `code_repository_imports` so SDK/API-surface usage candidates retain `resolution_state` and `target_hint` without resolving unauthorized external source.
- `software_files` is derived from `code_repository_files` so code, config, docs, build manifests, deployments, tests, templates, machine-readable API schemas, and the knowledge map are whole-file nodes. JSON/YAML filenames with a standalone case-insensitive `openapi` or `swagger` token receive the `api_schema` role; generated source clients with the same token retain their normal source/generated classification.
- `software_files` refresh stays bounded at 512 authoritative rows per page. It advances with the unique `(source_scope, path)` key rather than rescanning prior pages with `OFFSET`, validates every row through the same domain constructor, and reuses one prepared projection insert for the file phase. Fenced software projection v2 commits nine checkpointed transactions: reset, dependencies, SDK usages, lifecycle, files, topics, relationships, ontology, and publish. Every phase validates the same task/attempt/generation/lease fence before and after its work and atomically commits its rows with the next checkpoint. Intermediate rows remain attached to a stale, non-queryable scope; only the final publish transaction exposes the code scope, software status, and terminal checkpoint together. Releasing the SQLite writer for lease renewal therefore does not weaken freshness, rollback, or visibility semantics. A legacy v1 `publish` checkpoint resumes at the added ontology phase rather than skipping ontology materialization.
- `software_topics` is derived from Markdown/spec headings and `knowledge/knowledge-map.yaml` topic ids so repository documentation themes, architecture constraints, and knowledge routes are first-class nodes. Ordinary README headings, including “Getting Started” and “Chapter Index,” can become documentation topics but cannot become a `SoftwareSystem` from heading text or path alone.
- `software_relationships` is derived from committed dependency, SDK usage, feature-flag/config, and documentation-topic evidence to expose cross-domain edges such as `depends_on`, `uses_sdk`, `configures`, and `documents` with resolution state, target hints, confidence, evidence path, and line range.
- `software_build_targets` is derived from indexed chunk evidence in Dockerfile/Containerfile, Cargo, npm, Python, Go, Maven effective `pom.xml`, Gradle, CMake, Makefile, and CI workflow files. It covers definitions, packages, scripts, targets, features, modules, profiles, plugins, goals, pipelines, and jobs. A Dockerfile is a `BuildDefinition` and its image hint is a `ReleaseArtifact`; GitHub Actions and GitLab CI jobs are `BuildJob` entities rather than IaC resources. Maven effective models resolve repository-local parent POMs, properties, dependency management, plugin management, modules, profiles, and imported BOM declarations from indexed evidence only. The projection records evidence and command hints only; it does not execute build tools, read package caches, or contact registries.
- `software_iac_resources` is derived only from explicit deployment evidence such as Compose, Kubernetes YAML, Helm, Terraform, systemd, and launchd, preserving provider, resource kind, name, scope hint, target hint, and resolution state. Dockerfiles and ordinary CI jobs do not enter this projection, and queries neither call cloud APIs nor infer live cluster state.
- `software_design_elements` remains available for compatibility, but a Markdown heading defaults to `DocumentationUnit`. Promotion to `SoftwareSystem`, `Component`, `Api`, or `Resource` requires explicit frontmatter (`software-system`, `system`, `component`, `api`, or `resource`), a controlled manifest/schema, or other structured code evidence.
- `software_entities`, `software_statements`, and `software_ontology_diagnostics` materialize in parallel with the legacy tables. API traits/interfaces/protocols, OpenAPI/Swagger schema files, test symbols, configuration flags, build definitions, release artifacts, deployment units/resources, service definitions, and documentation units become typed entities. Every accepted active statement requires same-scope evidence, source kind, and extractor version. Projection schema version 7 marks older scopes stale and rebuilds them through the existing durable task, lease, checkpoint, and single-writer-per-repository path rather than a destructive in-place conversion.
- Typed read models without query text use deterministic evidence priority instead of name-only alphabetical order. `topics` returns document headings with explicit directory context before knowledge-map topics and root overviews; `design` prioritizes architecture, capability, and module evidence before API/system metadata; `apis` prioritizes API schemas and code declarations; `resources` follows the Kubernetes, Terraform, Compose, systemd, launchd, and Helm provider order; and `deployments` prioritizes platform service definitions before IaC and runtime observations. These rules sort only materialized rows already filtered by source scope, kind, path, and language and retain stable name/path/identity tie-breaks. Ranking must not read live source, widen the limit, or enumerate repositories, cases, paths, or symbols.
- In addition to legacy counts and the last error, `software_global_status` records `ontology_version`, `projection_schema_version`, `source_coverage`, `completeness_basis_points`, `freshness`, `entity_count`, `statement_count`, `conflict_count`, and `diagnostic_count`. `completeness_basis_points=10000` means statement provenance is complete within the current projection; it does not claim completeness for knowledge outside the authorized scope.
- CLI, Web, and MCP share one application service. Existing `dependencies|sdks|files|topics|relationships|build|iac|design|all` kinds remain compatible, and `systems|apis|resources|tests|deployments|releases|statements|conflicts` are added. The Web Software page selects a pinned commit from the bounded repository list and reads `statements` and `conflicts` in parallel to show stable-entity relationships, provenance/freshness, and conflict/shape diagnostics; it does not create a separate frontend fact model. `relay-knowledge repo software export <alias> --profile spdx-3|cyclonedx-1.7|prov-o` exports an interoperability document from the same snapshot-bound statement view. Query and export hot paths read committed projection rows and do not scan package caches, SDK directories, cloud APIs, unindexed external source, or the full repository documentation.
- For `repo software --kind all`, `--limit` is one strict total across the twelve response arrays, in this fixed priority order: `components`, `dependency_usages`, `sdk_usages`, `files`, `topics`, `relationships`, `build_targets`, `iac_resources`, `design_elements`, `entities`, `statements`, and `diagnostics`. The query reads at most the requested bound of candidates from each slice, then allocates one row to every non-empty, non-exhausted slice per round; unused capacity is redistributed in the next round. A retained statement may reclaim only a later-round surplus allocation for its required entity endpoints, so it never displaces a slice's initial fair row or produces dangling statement references. Statement endpoint batches are globally restored to canonical entity order before fair allocation, and diagnostics whose entity or statement evidence does not survive the request path/language filters are omitted before they can claim a fair slot. Therefore a bound at least as large as the number of non-empty slices returns at least one row from each, while a smaller bound deterministically favors earlier arrays in the listed order. This allocation changes neither each slice's evidence ordering nor the request maximum of 500, and the combined returned row count never exceeds `--limit`.

## 8. Knowledge Development Loop Boundary

The repository knowledge map records a stable `software-model` route to the
repository root; it does not duplicate projection rows or generated narratives.
Repository bootstrap, pinned-ref spec context, and post-commit reconciliation
are defined by [Code-Map-Backed Knowledge Development Loop](24-code-map-backed-knowledge-development-loop.md).
Code-index publication remains the only writer path that refreshes these
software projections for a source scope.

---

Navigation: Previous: [20. Multi-Repository Code Graph Overlay](20-multi-repository-code-graph-overlay.md) | Next: [22. Service Deployment, Control Plane, and Data Plane](22-service-deployment-control-data-plane.md)
