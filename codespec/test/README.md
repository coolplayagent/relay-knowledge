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
