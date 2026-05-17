# Chapter 1: Installation and Runtime Directories

[English](../../en/01-user-guide/01-install-and-runtime.md) | [中文](../../zh/01-user-guide/01-install-and-runtime.md)

This chapter covers the shortest path for getting the local development environment running. Full release, installer, and service-hosting requirements are covered in [Chapter 9: Resident Service](09-resident-service.md) and [Installation, Release, and Upgrade](../03-architecture-specs/19-installation-release-and-upgrade.md).

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
```

`build.sh` builds `target/release/relay-knowledge` and `web/dist`. `run.sh` only manages a local service process and asks you to run `./build.sh` if artifacts are missing. `check.sh` runs fmt, clippy, tests, coverage, Web build, and the browser integration gate when available.

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

`status --format json` shows current configuration and status. For an isolated one-off experiment, set a temporary `RELAY_KNOWLEDGE_HOME`:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-demo \
  target/debug/relay-knowledge status --format json
```

After setting `RELAY_KNOWLEDGE_HOME`, config, data, state, cache, logs, temp, runtime, and service directories are placed under that root. See [Chapter 12: Advanced Configuration](12-advanced-configuration.md) for the full directory override list.

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
