# Software-Global Evidence-Priority Reads

## Decision

`repo software` kinds without query text return bounded read-model rows in
evidence priority rather than name-only alphabetical order. The policy is
kind-specific because API authority, deployability, architecture context, and
topic specificity are different signals; a single global source ordering
would contradict the ontology's predicate/source authority model.

This change covers typed API, resource, and deployment entities plus the
compatible topic and design slices. Statement ranking remains on its indexed
stable order until a separate design proves an evidence-priority plan that
does not sort the maximum 524,288-statement scope on a query hot path.

## Evidence and implementation mapping

| Requirement | Production owner | Graph or storage evidence | Verification |
| --- | --- | --- | --- |
| Materialize machine-readable API contracts | `storage::sqlite::software::{graph::file_role,ontology::materialize}` | indexed JSON/YAML files with conventional OpenAPI/Swagger filename tokens become `api_schema` file/API entities and same-scope statements | `classifies_machine_readable_api_schemas_without_misclassifying_generated_clients` and `projection_materializes_api_schema_provenance_before_code_contracts` |
| Prefer schema/code API contracts to descriptive metadata | `storage::sqlite::software::ontology::query::entities_for_scope` | committed `software_entities.source_kind` within one source scope and `entity_kind=api` | production materialization test above and `typed_entity_queries_prioritize_actionable_evidence` |
| Prefer deployable resources to provider/module helpers | `storage::sqlite::software::ontology::query::entities_for_scope` | committed resource namespace/provider generated from existing IaC rows | `typed_entity_queries_prioritize_actionable_evidence` and the software-global self-iteration resource case |
| Prefer installed service definitions to generic IaC deployment units | `storage::sqlite::software::ontology::query::entities_for_scope` | committed `source_kind=service_definition` and typed deployment/runtime-service entities | `typed_entity_queries_prioritize_actionable_evidence` and the deployment case |
| Put architecture/capability/module evidence before catalog metadata | `storage::sqlite::software::lifecycle::design::design_elements_for_scope` | committed design element kind and confidence | `design_queries_prioritize_architecture_before_catalog_metadata` |
| Put specific nested document topics before root overviews | `storage::sqlite::software::graph::topics::topics_for_scope` | committed topic kind, source path, and line | `topic_queries_prioritize_specific_documents_before_root_overviews` |

The application software-projection service remains the only caller exposed to
CLI, Web, and MCP. No adapter receives a parallel ranking implementation.

## Invariants

- Source-scope, kind, path, and language filters run before ranking.
- The response limit, projection caps, freshness barrier, and stable
  name/path/identity tie-breaks remain unchanged.
- Reads use committed materialized rows only. They do not scan live source,
  package caches, SDK directories, cloud APIs, or external repositories.
- Projection schema version 7 makes the new derived API-schema classification
  refreshable for existing scopes. It does not change authoritative parser
  facts, ontology/table shape, publication fencing, task leases, checkpoints,
  or the single-writer boundary.
- Product code must not enumerate repository names, fixture paths, case ids,
  queries, or symbols.
- Any future statement-priority change must prove bounded query work on the
  maximum admitted scope or add an indexed priority representation with the
  matching migration, projection-version, recovery, and performance evidence.

## Verification contract

Run focused owner tests first:

```bash
cargo test --lib --all-features prioritize
cargo test --lib --all-features projection_materializes_api_schema_provenance_before_code_contracts
```

Then run the release-product self-iteration workload:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

Acceptance requires every selected gate and case to pass, no key metric budget
failure, and improved ranks for API, resource, deployment, topic, and design
without a software query p95 regression beyond its declared budget. Map and
documentation changes additionally require:

```bash
relay-knowledge map validate --type all --format json
python3 tools/docs/check_docs.py
```

The accepted focused result, report digests, environment, variance, and open
boundaries are preserved in the
[2026-08-31 verification record](../../docs/en/06-verification/14-software-global-evidence-priority-2026-08-31.md).
