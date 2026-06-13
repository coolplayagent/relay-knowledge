# Foundational Runtime

[English](../../en/03-architecture-specs/03-foundational-runtime.md) | [中文](../../zh/03-architecture-specs/03-foundational-runtime.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

The foundational runtime layer separates environment, paths, networking, QoS, and bootstrap boundaries from business logic. Its advantage is unique ownership for every external capability, diagnostic and redacted runtime configuration, and business services that do not know platform directories, environment variables, or network mechanics.

## 2. Environment Boundary

`env` is the only module that reads environment variables. It is responsible for:

- Loading, parsing, and validating all `RELAY_KNOWLEDGE_*` variables.
- Redacting secrets, tokens, headers, and endpoints in diagnostics.
- Distinguishing absent, empty, invalid, disabled, and explicitly configured values.
- Producing typed runtime configuration for CLI, Web, service doctor, and tests.

Business modules receive typed config and must not call `std::env`.

## 3. Path Boundary

`paths` is the only module that constructs runtime paths. Defaults follow platform conventions and keep configuration, data, logs, cache, temporary files, dead letters, and service definitions separate.

Installation directories, release extraction directories, current working directories, and repository roots do not store runtime state by default. When users configure paths explicitly, `paths` owns normalization, permission checks, and diagnostics.

Repository-local contracts such as `.knowledge/knowledge-map.yaml` are not runtime state. Process entry points may pass cwd as a bootstrap input, but repository-root discovery policy must stay inside the `paths` repository-root contract; application services receive an already resolved repository root.

## 4. Network and QoS Boundary

`net` owns all network capabilities; `net::http` owns HTTP clients and servers; `net::qos` owns admission and resource budgets. Application services request network capability by intent, source, tenant, priority, and budget, not by constructing sockets or clients.

QoS policy covers at least:

- Connection, request, and body limits.
- Per-source and per-tenant budgets.
- Timeouts, cancellation, and retry backoff.
- Overload responses and dropped-work observability.

## 5. Bootstrap Model

CLI, Web, and service mode share the same application services. Startup order is fixed:

1. Parse env.
2. Resolve paths.
3. Initialize net and QoS policy.
4. Open storage and index metadata.
5. Run startup reconcilers.
6. Accept CLI/API/MCP/Web work.

Any entry point that bypasses this order is an architecture defect.

## 6. Diagnostics

`status`, `health`, `service doctor`, Web diagnostics, and MCP resources read the same runtime snapshot. Diagnostics explain configuration sources, redacted values, directories, service state, QoS budgets, index freshness, and degraded reasons.

## 7. Acceptance Criteria

- Only `env` reads environment variables, only `paths` constructs runtime paths, and only `net` creates network capabilities.
- CLI, Web, service, and tests use the same typed runtime config.
- Configuration errors surface as stable startup or doctor errors, not as panics in business paths.

---

Navigation: Previous: [2. Engineering Hard Constraints](02-engineering-hard-constraints.md) | Next: [4. Source Scope Model](04-source-scope-model.md)
