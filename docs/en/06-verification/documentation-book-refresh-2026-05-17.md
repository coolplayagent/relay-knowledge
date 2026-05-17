# Documentation Book Refresh Audit 2026-05-17

[English](./documentation-book-refresh-2026-05-17.md) | [中文](../../zh/06-verification/documentation-book-refresh-2026-05-17.md)

This audit records the 2026-05-17 refresh of the documentation bookshelf,
directory responsibilities, and closed implementation status. This pass updates
documentation only; it does not change Rust, Web, or tool behavior.

## Refreshed In This Pass

| Area | Refresh |
| --- | --- |
| Bookshelf | Defined the responsibilities of `01-user-guide`, `02-capabilities`, `03-architecture-specs`, `04-research`, `05-benchmarks`, and `06-verification`. |
| Directory layout | Moved `documentation-refresh-audit-*` from the capabilities volume to the verification volume so audit evidence is not presented as product capability. |
| Indexes | Updated the root README, `docs/README.md`, and both language bookshelves to point at the new verification paths; the Chinese bookshelf now also links the Linux verification and self-iteration optimization records. |
| Roadmap spec | Reframed the GraphRAG roadmap as Phase 1-4 closed and Phase 5 open productization work, closing implemented items such as `setup doctor/profile`, MCP resources/prompts, service definition preview, audit sink, and metrics exporter. |
| Research reference | Reframed the implementation reference as closed status plus open productization tracks instead of treating local semantic/vector, MCP resources/prompts, Web operations, and the basic proposal lifecycle as missing. |
| Architecture specs | Updated the storage, background service, resident agent, and capability reference specs from implementation-order wording to closed-status or open-productization wording. |

## Open Productization Work

Open work is concentrated in concrete external providers, privileged service
install/rollback/package-manager distribution, watchdog/maintenance workflows,
valid-time and conflict product semantics, query routing, A2A gateway, remote ACP
host integration, and larger real-world datasets with release-facing quality
thresholds.

## Verification Commands

```bash
rg -n '02-capabilities/documentation-refresh' docs README.md README.zh-CN.md --glob '!docs/*/06-verification/documentation-book-refresh-2026-05-17.md'
rg -n '剩余实现工作|Remaining Implementation|Done:' docs README.md README.zh-CN.md --glob '!docs/*/06-verification/documentation-book-refresh-2026-05-17.md'
find docs -type f -maxdepth 4 -exec wc -l {} + | sort -nr | head -30
git diff --check
```
