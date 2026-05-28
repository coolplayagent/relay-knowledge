# Software Global Modeling Documentation Refresh Audit 2026-05-28

[English](../../en/06-verification/08-software-global-modeling-documentation-refresh-2026-05-28.md) | [中文](../../zh/06-verification/08-software-global-modeling-documentation-refresh-2026-05-28.md)

> Document version: 1.0
> Prepared: 2026-05-28
> Scope: verification for software global modeling research and architecture archival.

## 1. Refresh Content

This refresh archives research and architecture documentation. It does not change Rust, CLI, Web, storage schemas, or test harness behavior.

| Area | Update |
| --- | --- |
| Research | Added Chapter 10 covering software global modeling, dependencies and SDKs, code generation direction, dynamic graphs, and SBOM research conclusions. |
| Architecture | Added Chapter 21 defining global entities, relationships, change propagation, generation context, and acceptance criteria. |
| Navigation | Updated Chinese and English bookshelf pages and research volume indexes. |
| Verification | Added this audit page with validation scope and explicit no-code-change status. |

## 2. Verification Scope

- New documents keep Chinese/English mirrors and cross-links.
- New chapter numbers continue the existing `03-architecture-specs` and `04-research` sequences.
- Content follows existing hard constraints: versioned graph facts, derived-index freshness, persistent background tasks, unresolved external dependency metadata, and no unbounded query-hot-path scans.
- This change does not affect installation, release artifacts, service templates, migrations, or runtime directories, so the installation and release spec does not need an update.

## 3. Suggested Checks

```sh
rg -n "software-global-domain-modeling|全域建模|SoftwareSystem|uses_sdk|constrains_generation" docs
find docs -type f -name '*.md' -exec wc -l {} + | sort -nr | head
```

Rust behavior is unchanged, so `cargo test` is not required for this documentation-only refresh. If CI runs Rust gates globally, their result should reflect the pre-existing code state.
