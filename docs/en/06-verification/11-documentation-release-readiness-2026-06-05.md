# Documentation Release Readiness Audit 2026-06-05

[English](../../en/06-verification/11-documentation-release-readiness-2026-06-05.md) | [中文](../../zh/06-verification/11-documentation-release-readiness-2026-06-05.md)

> Date: 2026-06-05
> Scope: documentation-only release preparation
> Related contract: [Installation, Release, and Upgrade](../03-architecture-specs/19-installation-release-and-upgrade.md)

## 1. Objective

This audit records a release-readiness refresh for the documentation surface. The
goal is to make release entry points easier to follow, make book navigation
complete, expose language-edition coverage gaps explicitly, and leave product
behavior unchanged.

## 2. Inventory

- The docs tree now contains 169 Markdown files, including this English audit
  page and its Chinese counterpart.
- The tracked Markdown set stays below the repository's 1000-line file limit;
  the longest tracked Markdown page remains
  `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md` at 998
  lines.
- The English bookshelf now lists Appendix A.6 and A.7, which already had
  English pages but were missing from the index.
- The Chinese bookshelf now lists Appendix A.6 through A.10 and Appendix B.11.
- English navigation calls out Chinese-only benchmark Appendix A.8 through A.10
  and verification Appendix B.5 through B.6 until they are translated.

## 3. Documentation Changes

- Root `README.md` and `README.zh-CN.md` now include a release-readiness reading
  path that points to the bookshelf, installation guide, release architecture
  contract, and this audit.
- `docs/README.md` now describes the release-readiness reading path and the
  current language-coverage policy.
- The English and Chinese bookshelf indexes now expose the latest benchmark and
  verification records instead of leaving them discoverable only through file
  listing.
- Chapter 19 now requires a dated `06-verification` documentation readiness
  record before tagging a release.
- `.knowledge/knowledge-map.yaml` now includes this release-documentation route
  for future agent-assisted documentation work.

## 4. Safety Boundaries

This refresh intentionally does not modify Rust source code, Web source code,
GitHub Actions workflows, build scripts, package metadata, generated release
artifacts, CLI command behavior, service behavior, indexing, retrieval, storage,
or network behavior.

The release documentation must not imply unsupported artifacts, unmanaged
background loops, automatic silent binary replacement, or package-manager paths
that are not produced from the same release tag.

## 5. Validation

Local validation for this pass:

- `git ls-files '*.md' | xargs wc -l | awk '$1 > 1000 && $2 != "total" {print}'`
  reported no tracked Markdown file above 1000 lines.
- A code-span-aware local Markdown link check verified repository-local relative
  links and ignored examples such as ``rk_pipeline[index](dev)`` that are source
  text, not documentation links.
- `cargo fmt --all -- --check` confirms the documentation-only change did not
  disturb Rust formatting.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test --all-targets --all-features` passed.

Full release validation still requires the normal release gates from the root
README and CI, including package checks, coverage, browser integration setup,
and release workflow dry-run validation when a release tag is being prepared.

---

Navigation: Previous:
[10. Service Deployment, Control Plane, and Data Plane Documentation Refresh Audit](10-service-deployment-control-data-plane-2026-06-04.md)
