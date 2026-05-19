# Installation, Release, and Upgrade

[English](../../en/03-architecture-specs/19-installation-release-and-upgrade.md) | [中文](../../zh/03-architecture-specs/19-installation-release-and-upgrade.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Installation and release are part of product architecture. Stable releases are verifiable, rollbackable, uninstallable, and diagnosable. Binary install paths and runtime state are separate. Background services are managed by platform service managers.

## 2. Release Channels

- GitHub Releases publish cross-platform prebuilt archives, checksums, and release notes.
- crates.io keeps `cargo install relay-knowledge` working.
- Homebrew, Scoop, winget, or distro packages reference artifacts from the same release tag instead of rebuilding divergent snapshots.
- Release tags use `vX.Y.Z`, `X.Y.Z`, or matching prerelease forms such as `vX.Y.Z-rc.1`; the numeric version must match `Cargo.toml` and `Cargo.lock` before the tag is pushed. Manual dry-run dispatches validate the same version contract without publishing crates.io or GitHub release artifacts, and the workflow default dry-run tag must be updated with each release version bump.
- macOS x64 release jobs must use an active Intel runner label, such as `macos-15-intel`, rather than retired `macos-13` images. Artifact upload/download and attestation actions must stay on Node 24-compatible releases so the release workflow remains runnable after GitHub-hosted runner runtime migrations.
- Release archive attestations use the generated `checksums.txt` as their subject manifest, so GitHub artifact attestations cover the same archive digests that users verify locally.
- CLI version discovery uses configurable dual sources: GitHub Releases and crates.io. Detection must go through the `env`, `paths`, and `net::http` boundaries, inherit proxy, TLS, timeout, and runtime-cache policy, and ordinary commands may only notify about newer stable versions rather than silently replacing binaries.

## 3. Installation Experience

Installers or install scripts support version selection, install directory selection, dry run, checksum verification, service-definition generation, failure rollback, and uninstall plans. Runtime data is not written to release extraction directories by default.

## 4. Runtime State

Configuration, databases, indexes, logs, caches, temporary files, and dead-letter data live in platform directories owned by `paths`. Upgrades preserve runtime state and explicitly run schema/index migrations.
Local file-location indexes store SQLite/FTS5 state in the same runtime data
area. Installers and service templates must not default to scanning a whole
disk, Linux `/opt`, mounted volumes, or non-system Windows drives; those roots
are indexed only when the user configures them or passes them to the CLI.

## 5. Upgrade and Rollback

Upgrade flow:

```text
preflight doctor
  -> backup or migration checkpoint
  -> install new binary
  -> run schema/index migration
  -> service restart through platform manager
  -> post-upgrade doctor
```

On failure, binary and service definitions roll back. Data migrations have checkpoints or clear forward-only documentation.

`relay-knowledge version check` is a read-only diagnostic entry point that reports
the current version, newest stable version, source, release URL, and diagnostics.
Actual upgrades must still be performed explicitly by the user, installer, or
package manager and continue to follow the preflight, checkpoint, service
restart, and post-upgrade doctor flow.

## 6. Acceptance Criteria

- Release artifacts, checksums, versions, and documentation match each other.
- The CLI can explain when a newer stable version is available, JSON output remains machine-readable, and ordinary commands never auto-install an update.
- Service installation uses systemd, launchd, or Windows Service instead of unmanaged loops.
- Uninstall removes binaries and service definitions while preserving runtime data unless the user explicitly confirms removal.

---

Navigation: Previous: [18. Observability, Diagnostics, and SLO](18-observability-diagnostics-and-slo.md) | Next: [20. Multi-Repository Code Graph Overlay](20-multi-repository-code-graph-overlay.md)
