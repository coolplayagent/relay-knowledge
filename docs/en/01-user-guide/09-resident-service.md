# Chapter 9: Service Deployment and Resident Operation

[English](09-resident-service.md) | [中文](../../zh/01-user-guide/09-resident-service.md)

This chapter covers the complete service-deployment path: local foreground
verification, platform service-manager installation, remote access, operational
diagnostics, upgrade, rollback, and uninstall. See
[Chapter 19: Installation, Release, and Upgrade](../03-architecture-specs/19-installation-release-and-upgrade.md)
and [Chapter 22: Service Deployment, Control Plane, and Data Plane](../03-architecture-specs/22-service-deployment-control-data-plane.md)
for the architecture contracts.

The resident service hosts the Web workspace, HTTP API, MCP Streamable HTTP,
startup reconciler, code-index worker pool, repository-set refresh worker, and
operational endpoints. Use a foreground command or `run.sh` for development
verification. Long-running background operation must be managed by systemd,
launchd, or Windows Service.

The CLI provides `service plan`, `service definition`, and staged
`service lifecycle` commands. Lifecycle commands are dry-run by default; only
`service lifecycle <action> --execute` writes local files and invokes the
platform service manager. JSON API callers may send only `execute: true`; they
do not also need `dry_run: false`. An explicit `dry_run: true` still requests a
dry run. An execution failure returns an operation error with the failed step
id instead of wrapping a failed execution report as success.

> **Development only:** `run.sh` and `run.sh --daemon` are for development
> verification, not production deployment. Use systemd on Linux, launchd on
> macOS, or Windows Service for long-running background operation.

The Chinese edition provides deeper operational addenda for
[full service deployment](../../zh/01-user-guide/15-service-deployment-full-guide.md),
an [SRE operations runbook](../../zh/01-user-guide/16-sre-operations-runbook.md),
and [security configuration](../../zh/01-user-guide/17-security-configuration.md).
This English chapter consolidates the executable service lifecycle; it does not
claim one-to-one chapter parity with those addenda.

## 9.1 Choose a Deployment Topology

| Topology | Use case | Service management |
| --- | --- | --- |
| `embedded_cli` | One-off CLI work, tests, and temporary queries | No resident service |
| `resident_single_process` | Default local Web/API/MCP service and workers | One platform service |
| `resident_partitioned_sqlite` | Large or multiple repositories, with one control database and a shard per repository | One platform service; backups must include shards |
| `split_worker_preview` | Preview a separate worker claiming one durable task | Does not replace a service manager |

`resident_single_process` is the default. To use a SQLite shard per repository,
keep the same environment setting during preflight, definition generation,
service startup, and every operational command:

```bash
export RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite
```

After the primary database contains an active shard catalog, do not open the
same runtime state directly in `single_sqlite` mode. Complete an explicit
rollback first and handle the primary database and `stores/repositories/` shard
directory together.

## 9.2 Run Deployment Preflight

Build or install the binary, then check configuration readiness:

```bash
relay-knowledge setup doctor --format json
relay-knowledge setup profile service --format json
relay-knowledge service plan install --format json
relay-knowledge service lifecycle install --dry-run --format json
```

Review these `service plan install` fields:

- `platform`: `linux`, `macos`, or `windows` for the current platform.
- `definition_path`: destination used by `service definition write`.
- `install_command`, `start_command`, `stop_command`, and
  `uninstall_command`: commands for the installer or operator.
- `lifecycle_steps`: staged install, upgrade, rollback, or uninstall actions,
  including written paths, removed paths, and commands.
- `rollback_steps`: actions attempted after lifecycle execution fails.
- `permission_requirements`: privileges required by the platform service
  manager.
- `package_manifest_checks`: checks that Homebrew, Scoop, winget, or
  distribution packages reference the same release tag and checksum chain.
- `runtime_state_paths`: database, configuration, state, log, and cache paths;
  `partitioned_sqlite` also lists the shard directory.
- `checkpoint_path`: rollback-checkpoint location used during lifecycle work.
- `warnings`: shard, backup, migration, rollback, and uninstall warnings.
- `checksum`: stable checksum of the generated service definition.

Write the service definition:

```bash
relay-knowledge service definition write --format json
```

Before writing it, confirm that:

- `RELAY_KNOWLEDGE_DATA_DIR`, `RELAY_KNOWLEDGE_STATE_DIR`,
  `RELAY_KNOWLEDGE_LOG_DIR`, and other runtime directories are absolute and do
  not point into a release extraction directory or source checkout.
- A Linux systemd definition quotes paths that contain spaces and writes a
  literal `$` as `$$`, so systemd does not expand a dollar sign in an install or
  data path as an environment variable.
- `RELAY_KNOWLEDGE_HTTP_BIND` stays on loopback by default, such as
  `127.0.0.1:8791`.
- `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` is set when agents use MCP. Do not expose
  an unrestricted remote scope.
- A remotely reachable service sits behind an external identity-aware gateway
  that authenticates callers and applies a deny-by-default ACL to every Web,
  API, control, and MCP path. Keep the Relay listener on loopback when the
  gateway is on the same host.
- `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` controls code-index concurrency;
  its current maximum is 8.

## 9.3 Verify in the Foreground

Start MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

Start Web, API, and MCP on one port:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --web --mcp streamable-http
```

Check the service from another terminal:

```bash
curl http://127.0.0.1:8791/api/health
relay-knowledge service status --format json
```

The development scripts provide another local verification path:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
```

`run.sh --daemon` remains a development aid. Use the platform service manager
for deployed background operation.

At startup, `service run` runs the startup index reconciler and recovers
orphaned code-index leases. It then acts as the resident master for the
code-index worker pool and repository-set refresh worker. With neither Web nor
MCP enabled, it still waits for a shutdown signal and can therefore run under a
service manager.

## 9.4 Deploy with the Platform Service Manager

The common preparation flow is:

```bash
relay-knowledge setup doctor --format json
relay-knowledge service plan install --format json
relay-knowledge service lifecycle install --dry-run --format json
relay-knowledge service definition write --format json
```

After reviewing the dry-run paths, permissions, and rollback plan, an installer
or operator can execute the reported `install_command` and `start_command`, or
run the lifecycle explicitly:

```bash
relay-knowledge service lifecycle install --execute --format json
```

For a Linux systemd user service:

```bash
systemctl --user status relay-knowledge.service
journalctl --user -u relay-knowledge.service -n 100 --no-pager
```

The preceding lifecycle `--execute` call reloads the user manager, registers the
definition, and starts the service. The current Linux plan is explicitly a
`systemctl --user` plan; changing only `RELAY_KNOWLEDGE_SERVICE_DIR` to
`/etc/systemd/system` does not turn it into a system-service lifecycle.

If the user service must run while the user is logged out, an installer or
administrator should enable lingering:

```bash
loginctl enable-linger "$USER"
```

For macOS launchd:

```bash
launchctl print "gui/${UID}/com.coolplayagent.relay-knowledge"
```

The lifecycle execution loads and starts the launchd job. Do not separately
load a guessed plist path.

On Windows, use an elevated PowerShell session. Review the lifecycle plan first,
then let the executable apply the same definition, arguments, environment, and
rollback contract that it generated:

```powershell
relay-knowledge service plan install --format json
relay-knowledge service lifecycle install --dry-run --format json
relay-knowledge service lifecycle install --execute --format json
Get-Service relay-knowledge
```

The generated Windows `relay-knowledge-service.xml` remains the auditable
service-definition artifact. The lifecycle execution configures Windows Service
Control Manager and the service environment from that contract.

After platform registration, verify application status and HTTP health:

```bash
relay-knowledge service doctor --format json
curl http://127.0.0.1:8791/api/health
curl http://127.0.0.1:8791/api/v1/control/service/status
```

`service status` and `/api/v1/control/service/status` report code-index workers,
operator state, storage topology, queue and dead-letter state, runtime paths,
degradation, and watcher `enabled`/`commit_reconcile_interval_ms`. They are
short-budget diagnostic paths and do not synchronously run large indexes or
shard repair.

When an installed service sets `RELAY_KNOWLEDGE_WATCHER_ENABLED=true`, the
managed watcher handles both uncommitted source changes and changes to the
checked-out Git commit. Native `.git/HEAD`, ref, packed-ref, and HEAD-log events
are low-latency hints only. At startup and every
`RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS` milliseconds (default
`5000`), the service performs a bounded resolution of each watched repository's
`HEAD`. This periodic reconciliation recovers linked-worktree, dropped, and
coalesced watcher events without repository-specific hooks.

When `HEAD` differs from the latest published clean commit, the watcher pins an
immutable base, head, and tree in a durable incremental task. A stable per-ref
fingerprint coalesces duplicate hints, and normal task leases preserve one
writer per repository. Disabling the watcher disables both source events and
commit reconciliation. Put the watcher setting and interval in the
systemd/launchd/Windows Service definition—not only in the installation shell.

## 9.5 Configure Remote Access

The default bind is loopback-only. The Web workspace, HTTP API (including
`/api/v1/control/**`), and MCP Streamable HTTP do not provide built-in inbound
caller authentication. `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS`, Origin
filtering, QoS, MCP sessions, and MCP scopes do not prove caller identity; an
MCP scope is only a resource allow-list shared by every caller that reaches the
endpoint.

For remote access, keep Relay on loopback and put an external identity-aware
gateway in front of it. The gateway must authenticate each caller with OIDC or
validated tokens and apply a deny-by-default identity ACL, or use mTLS while
validating client certificates, mapping them to identities, and applying the
same ACL. TLS termination with only a server certificate encrypts transport but
does not authenticate the caller. Without such a gateway, use loopback only:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --web --mcp streamable-http
```

Configure the gateway to protect every path, including Web, `/api/**`,
`/api/v1/control/**`, and `/mcp`, and to deny requests when authentication or
ACL evaluation fails. Remote repository commands then use the authenticated
gateway URL, never the Relay backend port directly:

```bash
relay-knowledge --remote https://knowledge.example.com repo status my-repo --format json
relay-knowledge --remote https://knowledge.example.com repo update my-repo --format json
relay-knowledge --remote https://knowledge.example.com repo query my-repo "service startup" --format json
```

Automation can instead set:

```bash
export RELAY_KNOWLEDGE_REMOTE_BASE_URL=https://knowledge.example.com
```

Remote mode supports repository index/update, scope preview, status, query,
feature flags, impact, report, and software projection. `repo update` submits
`POST /api/v1/code/repositories/{alias}/update`; omitted `base_ref` and
`head_ref` default to the latest published clean base and `HEAD`. The response
may still represent a queued task.

The following maintenance operations must run on the service host:

- `repo index --reset`
- `repo index-worker`
- shard repair
- backup
- migration
- rollback
- uninstall

An internal network, firewall source rule, remote-client flag, Origin filter,
scope allow-list, QoS policy, or audit log is defense in depth, not caller
authentication. If a gateway must run on another host, bind Relay only to a
dedicated private gateway-facing address, permit only that gateway address in
the firewall, and enable the non-loopback exposure flag. The gateway still must
perform identity authentication and ACL enforcement; do not expose the backend
to a whole private network.

## 9.6 Operate Updates and Workers

Inspect, pause, or resume the silent-update operator:

```bash
relay-knowledge service operator status --format json
relay-knowledge service operator pause --format json
relay-knowledge service operator resume --format json
```

Silent updates must be configurable, observable, and reversible. They can
refresh graph data and derived indexes only within authorized scopes and must
expose fresh, stale, paused, degraded, and failure states.

The split-worker preview runs at most one durable code-index task:

```bash
relay-knowledge service worker run --format json
relay-knowledge service worker run --task-id <id> --format json
```

The command can write completion or failure only after claiming a task under an
attempt-scoped lease. An expired lease, mismatched attempt, or unclaimed task
cannot produce a successful result. Do not call this command in a loop as a
substitute for the platform service manager.

### Fenced publication and background recovery

A code-index worker does not publish a fresh repository when code facts alone
finish. Full and incremental tasks remain behind a fence until software
projection also succeeds. In a single SQLite store, scope freshness, software
status, checkpoint completion, and the publication receipt become visible in
one transaction. In partitioned mode, the new shard route remains `staged` and
owned by the durable task's `staged_task_id`; active-only reads continue using
the previous active scope. One control transaction then activates the route,
mirrors repository status, and records the receipt.

Task `succeeded` is a later fenced transaction and requires that receipt plus
the matching fresh scope and, when the target has a checkpoint, its completed
checkpoint; a mode without one does not fabricate it. If the service crashes
before control activation, recovery resumes
the staged shard. If it crashes after activation but before task completion, a
reclaimed attempt reuses the task-scoped receipt and converges without
republishing; the expired attempt still cannot report success. Operators should
inspect task, checkpoint, and repository status and allow the reconciler to
recover the lease rather than deleting lock files, catalog rows, or shard data.

## 9.7 Upgrade, Roll Back, or Uninstall

Use this upgrade sequence:

```text
preflight doctor
  -> stop ad hoc CLI writers
  -> stop the service through the platform manager
  -> confirm that every writer has stopped
  -> create a stopped-service, transaction-consistent runtime backup
  -> run lifecycle upgrade from the verified new release binary
  -> start the service through the platform manager
     -> the first synchronous open runs schema/index migration and any required shadow rebuild
     -> the service becomes available only after open completes
  -> post-upgrade doctor
```

Plan and validate the upgrade:

```bash
relay-knowledge setup doctor --format json
relay-knowledge service plan upgrade --target-version 1.2.3 --install-dir /opt/relay-knowledge --format json
relay-knowledge service lifecycle upgrade --dry-run --target-version 1.2.3 --install-dir /opt/relay-knowledge --format json
relay-knowledge service doctor --format json
```

Backups must be transaction-consistent and include every path in
`runtime_state_paths`. With `partitioned_sqlite`, back up the primary database
and shard directory together; a primary-only backup makes repository code facts
unavailable. A lifecycle checkpoint covers the binary and service definition,
not the runtime database. `--execute` requires the stop-service step to succeed
but does not independently prove that no unmanaged writer remains.

Use the stopped-service backup and restore procedure in the Chinese
[SRE operations runbook, Section 16.6](../../zh/01-user-guide/16-sre-operations-runbook.md)
as the authoritative runtime-data procedure. Do not mix manual binary copying
or a separate definition rewrite into the lifecycle upgrade path.

To uninstall the service while keeping runtime data:

```bash
relay-knowledge service plan uninstall --format json
relay-knowledge service lifecycle uninstall --dry-run --format json
relay-knowledge service lifecycle uninstall --execute --format json
```

Run the same dry-run/execute pair on macOS or from an elevated PowerShell
session on Windows. Deleting runtime data, logs, caches, dead letters, or shard
directories requires explicit user confirmation; lifecycle uninstall removes
the platform registration and definition but preserves the installed binary and
runtime state.

`service lifecycle rollback --dry-run` shows how the checkpointed binary and
service definition will be restored; `--execute` attempts those file and
service-manager steps. The lifecycle checkpoint is not a database snapshot. If
schema or runtime data must also be restored, keep the service stopped and use
the stopped-service runtime backup before executing lifecycle rollback. Without
both required backups, do not claim a complete rollback.

For an explicit `--install-dir`, install refuses to overwrite an existing
binary. Windows install creates the service and writes its environment as
separate steps; an environment-write failure removes the service created by
that attempt. Upgrade checkpoints an existing binary and service definition in
attempt-scoped files and publishes the checkpoint atomically before it becomes
the current rollback source. If no prior backup exists, failure rollback or an
explicit rollback deletes only files actually copied or written by that
attempt; a definition-only upgrade does not delete the active binary.

Windows upgrade refreshes SCM `BinaryPathName` and environment before startup;
macOS upgrade unloads and reloads the launchd plist. Uninstall failure rollback
and rollback from an uninstall checkpoint restore the deleted service
definition before registering the service again. A lifecycle report marks
rollback complete only when every selected rollback step succeeds. If restore,
definition rewrite, unregister, or service-registration rollback fails,
dependent delete, reload, or start steps do not continue. A failure before any
file or service-manager state changes does not stop, restore, or restart an
existing service.

External service-manager and doctor processes have execution timeouts and
drain stdout/stderr while waiting. Output collection remains bounded after exit
or timeout, preventing inherited pipes or large-output helpers from hanging the
execution report. A forward-only migration must be disclosed in change notes;
replacing the old binary alone is not a completed rollback.

## 9.8 Diagnose the Resident Service

Use this diagnostic order:

```bash
relay-knowledge status --format json
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge audit query --limit 50 --format json
```

For HTTP diagnostics:

```bash
curl http://127.0.0.1:8791/api/health
curl http://127.0.0.1:8791/api/service/status
curl http://127.0.0.1:8791/api/v1/control/storage/topology
```

Common symptoms:

- Web is unavailable after startup: check `RELAY_KNOWLEDGE_HTTP_BIND`, the
  systemd/launchd/Windows Service state, and service logs.
- A non-loopback bind is rejected: keep Relay on loopback behind a same-host
  identity-aware gateway. For a separately hosted gateway, use a dedicated
  private backend address, allow only that gateway through the firewall, and
  then set `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`; Origin and scope
  restrictions still do not authenticate callers.
- `single_sqlite` cannot open the runtime: look for an active
  `partitioned_sqlite` shard catalog and follow the rollback plan.
- `repo status` stays `running`: inspect the active task lease, checkpoint, and
  dead letter; do not kill processes or bypass the lease.
- `health` reports `storage_busy` or stale diagnostics: the short-budget probe
  degraded, but the service is not necessarily unavailable. Continue with
  service status, index lag, and queue depth.

See [Chapter 13: Operations and Troubleshooting](13-operations-and-troubleshooting.md)
for more diagnostic procedures.
