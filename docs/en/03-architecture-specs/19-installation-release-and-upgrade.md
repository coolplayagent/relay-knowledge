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

## 6. Acceptance Criteria

- Release artifacts, checksums, versions, and documentation match each other.
- Service installation uses systemd, launchd, or Windows Service instead of unmanaged loops.
- Uninstall removes binaries and service definitions while preserving runtime data unless the user explicitly confirms removal.

---

Navigation: Previous: [18. Observability, Diagnostics, and SLO](18-observability-diagnostics-and-slo.md)
