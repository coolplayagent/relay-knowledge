# Documentation Refresh Audit 2026-05-14

[English](./documentation-refresh-audit-2026-05-14.md) | [中文](../../zh/02-capabilities/documentation-refresh-audit-2026-05-14.md)

This is the English documentation page for `documentation-refresh-audit-2026-05-14.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

This audit records the documentation pass for the current `relay-knowledge`
implementation on 2026-05-14. The source of truth for command availability is
`relay-knowledge help --format json`; status and health behavior were checked
against the compiled binary.

## Refreshed In This Pass

| Area | Refresh |
| --- | --- |
| README | Added setup diagnostics/profile commands to current capabilities and CLI examples. |
| User guide | Bumped the guide version to 1.2, added `setup doctor`, documented setup profiles, and removed the planned-only setup wording. |
| Advanced configuration | Replaced the old planned setup section with the implemented `setup doctor` and `setup profile` behavior. |
| Operations troubleshooting | Added setup doctor to the diagnostic order and documented obsolete local database migration behavior. |
| Semantic/vector backend | Added the external embedding setup profile as the recommended starting point. |
| Unified API spec | Added setup commands to the current CLI surface and clarified that setup profile is read-only recommendation output. |
| Installation/release spec | Updated service-install guidance to match the implemented `service plan`, `service definition write`, and `setup profile service` flow. |
| Storage migration | Known obsolete SQLite table definitions are migrated in place; derived retrieval tables and refresh queues may be rebuilt without deleting graph data. |

## Current Documentation Status

| Document Group | Status |
| --- | --- |
| Root README and docs index | Current for the implemented CLI/Web/MCP/service/setup surfaces. |
| User guide | Current for local install, CLI basics, GraphRAG, code repository workflows, Web workspace, agent/service operation, troubleshooting, and advanced configuration. |
| Feature docs | Current for GraphRAG context packs, semantic/vector providers, and tree-sitter repository retrieval. |
| Specs | Mixed by design: hard constraints and interface specs are current contracts; product, storage, background service, architecture, and installation specs still include forward-looking requirements. |
| Research docs | Historical/reference material. They intentionally retain roadmap and gap-analysis language rather than being rewritten as user instructions. |
| Benchmarks and verification notes | Snapshot records from 2026-05-14. They should remain dated evidence unless a new benchmark run supersedes them. |

## Implemented From Previous Planned Wording

- `relay-knowledge setup doctor`: storage-free read-only configuration readiness check over
  runtime paths, network/QoS budget, retrieval backend metadata, MCP scope
  policy, service directory, and worker budget, with `configuration_ready`,
  `live_health_checked=false`, `live_health_commands`, and `recommended_actions`
  for remediation.
- `relay-knowledge setup profile local|agent-readonly|service|external-embedding`:
  read-only environment and command recommendations that do not write files,
  mutate shell profiles, or install services.
- SQLite startup migration: known obsolete table definitions are migrated in
  place, and only derived retrieval tables or refresh queues are rebuilt, so
  `health` and `service doctor` do not run against obsolete schemas.

## Remaining Implementation Work

| Capability | Current State | Remaining Work |
| --- | --- | --- |
| Privileged service install/uninstall | `service plan` and `service definition write` are implemented. | Installer or operator flow must execute platform service-manager commands with rollback and uninstall semantics. |
| Package manager distribution | Release workflow produces artifacts; specs describe Homebrew/Scoop/winget/distro expectations. | Publish and maintain package-manager manifests that reference release artifacts. |
| External embedding/OCR/vision providers | Runtime config, provider probe, worker endpoint contracts, deterministic fallback proposals, and setup profile exist. | Productize concrete provider adapters, model coexistence policy, and operator docs for production deployments. |
| Larger evaluation datasets | CI fixture gate exists for GraphRAG behavior. | Add larger real-world datasets, longitudinal reports, and release-facing quality thresholds. |
| Remote ACP productization | Local ACP adapter exists. | Build remote host integration, authentication, and installation guidance when the product surface is ready. |

## Verification Commands

```bash
relay-knowledge help --format json
relay-knowledge setup doctor --format json
relay-knowledge setup profile external-embedding --format json
cargo test --all-targets --all-features cli
cargo test --all-targets --all-features startup_migrates_obsolete_refresh_queue_schema_without_deleting_graph_data
```
