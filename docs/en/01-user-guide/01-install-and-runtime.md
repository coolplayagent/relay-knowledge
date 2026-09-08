# Chapter 1: Installation and Runtime Directories

[English](../../en/01-user-guide/01-install-and-runtime.md) | [中文](../../zh/01-user-guide/01-install-and-runtime.md)

This chapter covers the shortest path for getting the local development environment running. Full release, installer, and service-hosting requirements are covered in [Chapter 9: Service Deployment and Resident Operation](09-resident-service.md) and [Installation, Release, and Upgrade](../03-architecture-specs/19-installation-release-and-upgrade.md).

## 1.1 Prerequisites

The repository uses Rust 2024 edition, with a compatible toolchain pinned in `rust-toolchain.toml`. Install Rust with `rustup`, then run from the repository root:

```bash
./setup.sh
```

`setup.sh` prepares Rust components and hooks. It does not build release artifacts, start services, or run the full quality gate.

Common scripts are split by responsibility:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
./check.sh
./check.sh --deep
```

`build.sh` builds `target/release/relay-knowledge` and `web/dist`. `run.sh` only manages a local service process and asks you to run `./build.sh` if artifacts are missing. `check.sh` runs documentation, fmt, `cargo check`, Clippy, tests, coverage, the Web build, and the browser integration gate when available. `check.sh --deep` additionally runs the deterministic benchmark, an FFI-free Miri subset, and AddressSanitizer. The deep profile requires nightly components and fails with an installation command instead of silently skipping them:

```bash
rustup toolchain install nightly --profile minimal --component miri,rust-src
./check.sh --deep
```

AddressSanitizer is limited to the Linux and macOS targets selected by the script. Linux x86_64 CI is the authoritative cross-developer sanitizer gate.

## 1.2 Local Execution

When the binary is not installed on `PATH`, run the debug binary directly:

```bash
cargo build
target/debug/relay-knowledge status
target/debug/relay-knowledge --version
target/debug/relay-knowledge setup doctor --format json
```

You can also use Cargo:

```bash
cargo run -- status --format json
cargo run -- query -- --help
```

`relay-knowledge` starts a Tokio runtime. CLI, Web, MCP, and the local ACP adapter all enter the core through the same application service so their behavior does not diverge.

## 1.3 Same-Port Local Service

When you need the browser workspace or a local MCP endpoint, build first and then start the same-port Web/API/MCP service:

```bash
./build.sh
./run.sh start --port 8791 --daemon
curl http://127.0.0.1:8791/api/health
./run.sh stop --force
```

The underlying command is:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
  target/release/relay-knowledge service run --web --mcp streamable-http
```

Do not use unmanaged CLI loops for long-running background operation. Use the service-manager path in Chapter 9 instead.

## 1.4 Zero-Config Defaults

Normal local use does not require environment variables. Defaults are:

- Runtime directories are resolved by platform rules and do not write into the repository.
- Local SQLite storage and deterministic semantic/vector read models are enabled.
- Network and QoS budgets use conservative defaults.
- MCP writes, remote listening, and silent updates are disabled by default.
- File watcher (fs.watch) is enabled by default, automatically detecting source changes and pushing incremental index tasks.
- Interactive text CLI commands check for newer stable versions on a 24-hour cache interval and only print a notice; they do not install or replace binaries.

`status --format json` shows current configuration and status. For an isolated one-off experiment, set a temporary `RELAY_KNOWLEDGE_HOME`:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-demo \
  target/debug/relay-knowledge status --format json
```

After setting `RELAY_KNOWLEDGE_HOME`, config, data, state, cache, logs, temp, runtime, and service directories are placed under that root. See [Chapter 12: Advanced Configuration](12-advanced-configuration.md) for the full directory override list.

New Windows installations store SQLite in
`D:\relay-knowledge\users\<user-sid>\data\relay-knowledge.sqlite`, with shards
under `stores/repositories/` in the same data directory. The user SID comes from
the Windows process token, so profile relocation and LocalAppData changes do not
change the default store. When SQLite is opened, new account directories receive a protected ACL for
the account, SYSTEM, and Administrators. Unsafe existing ACLs, reparse points,
or ancestors granting other accounts deletion or permission changes are rejected.
Service paths pinned to the SID layout retain this policy and revalidate ACLs
and reparse points on every new service startup. Install/upgrade/rollback execution
also provisions or validates these directories before service-manager steps;
plans and uninstall do not provision storage. This layout requires Windows PowerShell 5.1 and an ACL-capable local volume;
use an explicitly configured private directory if D: cannot meet these conditions. `status --format json`
shows the resolved directory. Config, logs, and other runtime directories retain
their AppData/TEMP defaults. Linux and macOS defaults are unchanged.

Data directory precedence is `RELAY_KNOWLEDGE_DATA_DIR` >
`RELAY_KNOWLEDGE_HOME/data` > existing Windows LocalAppData data directory > new
platform default. Without an explicit override, startup preserves
`%LOCALAPPDATA%\relay-knowledge\data` whenever that directory exists, including
its database, recovery files, and shards. If both the old directory and the new
account directory exist, startup requires `RELAY_KNOWLEDGE_DATA_DIR` to select
one explicitly. Directory inspection errors or timeouts are reported instead
of silently opening an empty database elsewhere.

To choose another directory for SQLite in PowerShell:

```powershell
$env:RELAY_KNOWLEDGE_DATA_DIR = 'E:\KnowledgeData'
relay-knowledge status --format json
# Optional: persist for future shells of the current user.
[Environment]::SetEnvironmentVariable('RELAY_KNOWLEDGE_DATA_DIR', 'E:\KnowledgeData', 'User')
```

The value is a directory, not a database filename. Empty values, relative paths,
and paths containing `..` are rejected. For a new installation, if D: is absent
or its directory is not writable, database creation/opening fails; choose an
accessible absolute path. Existing LocalAppData stores remain usable without D:.

Upgrades retain existing databases in place rather than moving them. You can
also pin the old Windows location explicitly, including for a service:

```powershell
$env:RELAY_KNOWLEDGE_DATA_DIR = Join-Path $env:LOCALAPPDATA 'relay-knowledge\data'
```

To migrate, stop the managed service and other writers, back up the complete
data directory consistently, then copy the main database, any WAL/SHM recovery
files, and `stores/repositories` together to the selected directory. Retain the
old copy for rollback. Configure every CLI/service with the same directory;
installed services retain the explicit data path in their service definition,
so changing a shell variable alone does not relocate an existing service.
Regenerate and apply its lifecycle plan, then check `status`, `health`, and
`service doctor`. Follow the [upgrade and rollback contract](../03-architecture-specs/19-installation-release-and-upgrade.md)
when restoring a previous binary. Uninstall preserves runtime data by default.

## 1.5 Configuration Readiness

If you are not sure whether the machine is ready, start with the read-only configuration diagnostic:

```bash
relay-knowledge setup doctor --format json
```

`setup doctor` does not open SQLite, migrate schema, or refresh indexes. It checks runtime paths, network/QoS budgets, retrieval backend metadata, MCP policy, service directories, and worker budgets. After configuration passes, run:

```bash
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

to check graph storage, index freshness, worker/service live health, and telemetry state.

## 1.6 Network and Path Boundaries

All path overrides must be absolute paths and must not contain `..`. Path resolution is owned by the `env` and `paths` boundaries.

Resident service and MCP Streamable HTTP use `net::http` and `net::qos` for network capabilities. Normal local use should not require network budget changes; use [Chapter 12](12-advanced-configuration.md) when enabling remote listening, increasing body limits, or reproducing proxy issues.

Proxy and certificate verification settings inherit `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`, and `SSL_VERIFY`. These variables are read only at the environment boundary; business modules do not read the process environment directly.

Version notices use the same network boundary and proxy/TLS policy. Set
`RELAY_KNOWLEDGE_UPDATE_CHECK_ENABLED=false` to disable notices,
`RELAY_KNOWLEDGE_UPDATE_SOURCES=github,crates.io` to choose sources,
`RELAY_KNOWLEDGE_UPDATE_CHECK_INTERVAL_MS` to tune the cache interval, and
`RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO=owner/name` to point at a forked release
source.
