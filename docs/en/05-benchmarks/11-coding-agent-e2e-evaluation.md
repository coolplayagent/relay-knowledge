# Coding-Agent E2E Evaluation Gate

Issue #300 adds a reproducible coding-agent workflow gate to the Rust self-iteration harness.

## Scope

- Command: `./self-iterate.sh evaluate --use-current-candidate --profile fast --categories agent_workflows`
- CI job: `agent-workflow-regression` in `.github/workflows/benchmark-checks.yml`
- Fixture file: `tools/self_iteration/cases/agent_workflow_targets.json`
- Generated repository: `agent_workflow_fixture`

The fixture is generated during evaluation and contains Rust, TypeScript, Python, YAML, and Markdown files. It covers definition lookup, cross-language impact tracing, configuration-to-documentation tracing, and freshness policy checks using `wait-until-fresh` and `allow-stale` query steps.

## Metrics

Each workflow fails when any budget is exceeded:

- tool calls
- unique source files read into the context pack
- captured command output characters
- packed context characters
- minimum matched evidence hits
- text-fallback hit ratio
- total query latency

The CI job limits the fast repository set to `agent_workflow_fixture` and checks the generated JSON report for failed gates, failed cases, and failed agent workflow metric budgets. It does not rely on the self-iteration score adoption decision, which can be affected by historical best-run comparison. The gate stays local, deterministic, and cheap enough for pull requests while still failing on oversized context, missing evidence, excessive fallback, and obvious latency regressions.

## Constraints

The gate must remain product-general. Do not fix failures by special-casing the fixture repository name, paths, symbols, query text, or benchmark IDs in product code. Improvements should come from retrieval planning, ranking, indexing, evidence packing, or bounded fallback behavior.
