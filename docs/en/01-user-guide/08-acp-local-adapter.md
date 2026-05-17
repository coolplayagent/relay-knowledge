# Chapter 8: ACP Local Adapter

[English](../../en/01-user-guide/08-acp-local-adapter.md) | [中文](../../zh/01-user-guide/08-acp-local-adapter.md)

The ACP local adapter is for same-process or local agent-client sessions. It exposes the same retrieval semantics as MCP, but is better suited to local interactions that need progress, cancellation, and context artifacts.

## 8.1 Boundary

The ACP adapter does not create independent business logic and does not bypass the unified API. Retrieval, authorization, QoS, audit, and context pack structure are shared with MCP, CLI, and Web.

MCP is usually the better entry point for other agent runtimes. ACP is better for local agent-client sessions. Neither protocol should access storage, indexing, or graph mutation implementations directly.

## 8.2 Session Capabilities

The local ACP session adapter supports:

- Initialize capability payload.
- Bounded local sessions.
- Progress updates.
- Cancellation.
- Context artifacts.
- Bounded in-process audit events.

Prompt turns are constrained by max runtime budgets. Timeout or cancellation returns an explicit status and does not keep consuming query hot-path resources in the background.

## 8.3 Scope and Identity

ACP uses the local adapter identity and carries untrusted client identity into audit metadata. It follows the same authorization boundary as MCP; source scopes in requests must satisfy runtime policy and are not upgraded by default just because the caller is local.

When an agent needs access to a knowledge scope, explicitly configure scope policy or first establish the alias through the code repository registration workflow.

## 8.4 Context Artifact

An ACP prompt response can include a context artifact. The artifact gives the caller the retrieved context pack, ranking, graph facts, source spans, budget, and truncation state instead of compressing retrieval details into an unauditable natural-language summary.

Callers should preserve source, freshness, degraded reason, and audit correlation from the artifact so later runs can reproduce or explain agent output.

## 8.5 Choosing MCP or ACP

Prefer MCP when:

- Integrating with an external agent runtime.
- You need the standard Streamable HTTP tool/resource/prompt surface.
- You need the Prometheus metrics endpoint.

Prefer ACP when:

- An agent client works with `relay-knowledge` locally.
- You need progress, cancellation, and context artifacts.
- You do not want to open an HTTP remote-access surface for the local session.
