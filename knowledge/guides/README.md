# Guides

This directory is governed by `knowledge/knowledge-map.yaml`. Update its map entry through `relay-knowledge map directory` and keep reviewed source material within the declared content scope.

## Verifying repository knowledge contracts

Repository-map regressions are covered by the foundational self-iteration cases in `tools/self_iteration/cases/repository_map_targets.json`. They exercise public CLI behavior for both governed maps without mutating repository content or runtime graph state.

Use the cases as an evidence ladder:

1. Read `help map --format json` to confirm that agents can discover typed CodeSpec and Knowledge operations.
2. Run `map validate --type all --format json` and require both `codespec/codespec-map.yaml` and `knowledge/knowledge-map.yaml` to be valid with no diagnostics.
3. Read the CodeSpec `test` and Knowledge `guides` directory filters to confirm their scopes, key files, policies, and cross-map relation.
4. Route `business-knowledge`, `software-model`, and `architecture`; require the two reserved sources and the complete ordered architecture source set.
5. Read versions 15 through 18 as one bounded history page and require it to cross archived version 16 into the recent window without a gap.
6. Run the focused fast self-iteration workload and require the eight CLI observations plus three v4 index-backed observations. The generated fixture must expose all eight root-authorized topic/relationship dimensions and exclude its locally valid orphan shard.

```bash
cargo build --release --bin relay-knowledge
target/release/relay-knowledge help map --format json
target/release/relay-knowledge map validate --type all --format json
target/release/relay-knowledge map show --type codespec --directory test --format json
target/release/relay-knowledge map show --type knowledge --directory guides --format json
target/release/relay-knowledge map route business-knowledge --type knowledge --format json
target/release/relay-knowledge map route software-model --type knowledge --format json
target/release/relay-knowledge map route architecture --type knowledge --format json
target/release/relay-knowledge map history --type knowledge --from 15 --limit 4 --format json
tools/self_iteration/target/debug/relay-knowledge-self-iterate evaluate --workspace . --profile fast --categories foundational --use-current-candidate
```

Treat validation and route output as repository-contract evidence only. The generated v4 fixture supplies the separate snapshot-bound code-index and software-projection evidence; real-repository freshness and business facts still require the status/query workflow described by the architecture specifications.
