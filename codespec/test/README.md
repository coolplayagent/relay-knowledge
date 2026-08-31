# Test

This directory is governed by `codespec/codespec-map.yaml`. Update its map entry through `relay-knowledge map directory` and keep reviewed source material within the declared content scope.

## Repository map self-iteration contract

The checked-in workload at `tools/self_iteration/cases/repository_map_targets.json` protects the repository map acceptance surface with deterministic, read-only CLI cases:

- `repository_map_help_exposes_typed_contracts` requires agent-visible help to advertise typed CodeSpec and Knowledge maps.
- `repository_map_validation_covers_codespec_and_knowledge` requires both checked-in map roots to validate without diagnostics.
- `codespec_test_directory_contract_is_visible` requires the CodeSpec `test` directory, content scope, key file, load policy, and update policy to remain queryable.
- `knowledge_guides_directory_contract_is_visible` requires the governed Knowledge guide directory and its relation to `codespec:requirements` to remain queryable.
- `knowledge_business_route_preserves_authored_glossary` requires the built-in business route to resolve to the authored repository glossary.

All five cases are foundational fast-profile guardrails. A non-zero CLI exit, malformed JSON, invalid map, reordered or missing required response evidence, or changed governed contract fails the corresponding case. The assertions intentionally use public CLI output instead of parsing YAML directly, so they cover repository-root discovery, map assembly, validation, filtering, routing, and JSON rendering together.

The companion `software_global_fixture` remains the index-backed compatibility rail: its legacy inline Knowledge map includes `updated_at` and contiguous history, then must survive code indexing and expose the routed topic through `repo software --kind topics`. The 1,024-file performance fixture separately requires all 1,024 authorized `src` files at cold publication; its minimum must not count files outside the registered path filter.

Run the focused current-worktree evaluation from the repository root:

```bash
cargo build --manifest-path tools/self_iteration/Cargo.toml --bin relay-knowledge-self-iterate
tools/self_iteration/target/debug/relay-knowledge-self-iterate evaluate --workspace . --profile fast --categories foundational --use-current-candidate
```

The evaluation must report every repository-map case as passed; an empty selection is not evidence of success. Also run `relay-knowledge map validate --type all --format json` when changing either governed map or its key files.

## Partitioned catalog startup regression contract

The storage unit rail proves both sides of catalog startup:

- `current_catalog_schema_revalidation_does_not_request_a_write_lock` holds an independent `BEGIN IMMEDIATE` transaction and requires current-schema revalidation to remain read-only.
- `legacy_scope_publication_columns_migrate_idempotently_and_default_active` requires legacy catalog columns to migrate inside the serialized writer boundary and remain idempotent.

Run the focused tests from the repository root:

```bash
cargo test current_catalog_schema_revalidation_does_not_request_a_write_lock --all-features
cargo test legacy_scope_publication_columns_migrate_idempotently_and_default_active --all-features
```

The end-to-end performance regression rail is:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

The run must have a non-empty selection, all gates and cases must pass, and the C, C++, TypeScript, and cross-language query p95 metrics must remain within their checked-in budgets. These metrics exercise concurrent CLI process startup and fail if current catalog validation again serializes on the control database writer lock.

## Software-global evidence-priority contract

The storage-owner rail fixes the deterministic order of materialized API,
resource, deployment, design, and topic evidence without changing response
limits or reading live source:

```bash
cargo test --lib --all-features prioritize
```

The end-to-end rail is the same release-product performance-focused workload:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

It must keep every gate/case and key metric within budget while improving the
software-global API, resource, deployment, design, and topic primary ranks.
Statement ranking remains outside this change until an independently bounded
or indexed plan is specified and measured.

## Worktree line-budget contract

`all_tracked_text_files_stay_within_line_budget` applies the repository-wide
1,000-line limit to regular tracked and untracked files that exist in the
current worktree. It must continue to inspect retained and newly created files,
while ignoring a cached path that a candidate intentionally deleted. This keeps
the pre-commit gate usable during CodeSpec/Knowledge shard replacement without
turning a valid deletion into an `ENOENT` failure.

The focused regression rail is:

```bash
cargo test --test relay_knowledge architecture_boundaries::layout_contracts:: -- --nocapture
```

`tracked_file_listing_excludes_deleted_cache_entries` creates an isolated Git
repository and proves the same listing includes retained tracked and new
untracked files but excludes the deleted tracked entry.

## Durable worktree delta and lease-recovery contract

Worktree indexing remains direct-first. If the complete overlay exceeds its
frozen writer quantum, the same durable task must clone the immutable base,
apply deterministic file-owned delta batches, and complete through the normal
finalizer. The regression rail must prove that this path neither converts the
synthetic worktree identity to a clean snapshot nor enlarges the resource
budget:

```bash
cargo test --all-targets --all-features code_index_task_ -- --nocapture
```

`oversized_worktree_code_index_task_delta_batches_and_recovers_between_leases`
is the end-to-end owner case. It requires two dirty batches, an expired lease
and a new attempt between them, replay from the persisted cursor, one
task-bound multi-batch receipt, the true maximum `last_path`, and publication
only after ordinary query-index, edge, software, and business finalization.
Planner tests also require deterministic file ownership and reject orphan facts
or any indivisible file whose bytes or owned rows exceed one frozen writer
quantum. Receipt tests keep deletion-only affected-path metrics decodable after
clone ownership is removed while continuing to bound parsed files and SQLite
writes by the recorded durable data batches.

The real-product rail is a `relay-knowledge repo index <alias> --ref worktree`
followed by `repo context` at the returned resolved synthetic identity. The CLI
default keeps auto-workspace detection disabled. API/Web callers that combine a
dirty worktree with a non-empty auto-workspace projection must fail closed until
a separately bounded, persisted workspace manifest is specified.

## CLI adapter workflow and network-boundary contract

The CLI unit rail keeps repository governance and remote execution aligned with their shared domain APIs:

- `map_execution_keeps_cli_and_repository_governance_in_sync` exercises both map types through init, source mutation, filtered routing/show, directory mutation, bounded history, validation, removal, and the repository-free agent snippet, and asserts rendered JSON rather than only exit status.
- `map_migration_commands_preserve_a_recoverable_legacy_root` proves that CLI migration publishes the v3 root and rollback restores the retained legacy root.
- `map_parser_covers_the_complete_governance_command_surface` protects every typed map mutation and the supported directory governance enum values.
- `map_mutation_namespaces_require_an_operation_before_map_type` keeps `map source` and `map directory` aligned with their help surface instead of reporting a later `--type` error before the missing operation.
- `every_repository_remote_command_rejects_an_empty_alias_before_transport` requires every remote repository command family to reject an empty identity before consuming network capacity.
- `remote_command_families_map_connection_failures_without_local_fallback` requires all remote read/write families to preserve `storage_unavailable` on connection failure instead of silently reading local state.
- `non_remote_actions_do_not_consume_network_capacity` and `remote_urls_are_normalized_and_repository_segments_are_encoded` protect remote capability selection and URL construction.

Run the focused adapter rail and the exact CI coverage gate from the repository root:

```bash
cargo test --all-features interfaces::cli::map::mod_tests
cargo test --all-features interfaces::cli::remote::mod_tests
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

The coverage command must include all targets and features and must not lower the 90% line threshold or exclude low-coverage production files. The 2026-08-30 verification covered 135,666 of 150,492 production lines (90.15%), with 3,752 library tests passed, one bounded subprocess fixture ignored, one benchmark target passed, and all 155 integration tests passed.
