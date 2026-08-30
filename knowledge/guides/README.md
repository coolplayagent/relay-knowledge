# Guides

This directory is governed by `knowledge/knowledge-map.yaml`. Update its map entry through `relay-knowledge map directory` and keep reviewed source material within the declared content scope.

## Verifying repository knowledge contracts

Repository-map regressions are covered by the foundational self-iteration cases in `tools/self_iteration/cases/repository_map_targets.json`. They exercise public CLI behavior for both governed maps without mutating repository content or runtime graph state.

Use the cases as an evidence ladder:

1. Read `help map --format json` to confirm that agents can discover typed CodeSpec and Knowledge operations.
2. Run `map validate --type all --format json` and require both `codespec/codespec-map.yaml` and `knowledge/knowledge-map.yaml` to be valid with no diagnostics.
3. Read the CodeSpec `test` and Knowledge `guides` directory filters to confirm their scopes, key files, policies, and cross-map relation.
4. Route `business-knowledge` with `--type knowledge` and require the active authored glossary source at `knowledge/glossary/business-glossary.yaml`.
5. Run the focused fast self-iteration workload and confirm that all five named observations are present and passed.

```bash
cargo build --release --bin relay-knowledge
target/release/relay-knowledge help map --format json
target/release/relay-knowledge map validate --type all --format json
target/release/relay-knowledge map show --type codespec --directory test --format json
target/release/relay-knowledge map show --type knowledge --directory guides --format json
target/release/relay-knowledge map route business-knowledge --type knowledge --format json
tools/self_iteration/target/debug/relay-knowledge-self-iterate evaluate --workspace . --profile fast --categories foundational --use-current-candidate
```

Treat validation and route output as repository-contract evidence only. Code-map, software-projection, and business-fact freshness still require the separate snapshot-bound repository status and query evidence described by the architecture specifications.
