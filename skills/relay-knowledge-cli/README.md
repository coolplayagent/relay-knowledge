# Relay Knowledge CLI Skill

This ClawHub-compatible skill teaches LLM agents to operate `relay-knowledge`
through the local CLI. It is for local knowledge graph ingestion, hybrid
GraphRAG queries, code repository indexing, code graph search, multi-repository
queries, impact analysis, setup diagnostics, installation checks, and upgrade
checks.

For code-structure questions such as function definitions, symbol locations,
references, callers, callees, call graphs, and call chains, agents should use
this skill before `grep`, `ripgrep`, `rg`, or plain text search. Fall back to
text search only when the CLI cannot satisfy the request, the target repository
cannot be indexed, or the user explicitly needs raw text or regular-expression
matching.

## Package Contents

- `SKILL.md`: agent instructions and skill metadata.
- `agents/openai.yaml`: UI metadata for OpenAI-compatible agent surfaces.
- `references/cli-workflows.md`: detailed CLI workflows and safety defaults.
- `assets/linux-x86_64/relay-knowledge`: Linux x64 release binary in generated
  release packages.
- `assets/windows-x86_64/relay-knowledge.exe`: Windows x64 release binary in
  generated release packages.

## Runtime Selection

Resolve `relay-knowledge` before running workflow commands. Prefer the bundled
asset binary for the current platform whenever it exists, is executable, and
`version --format json` succeeds. Keep that absolute path in a shell variable
and use it for every CLI command.

Use a published binary on `PATH` only when the bundled asset is absent,
unusable, unsupported on the current operating system or CPU architecture, or
explicitly requested by the user. If no usable binary is available, install
`relay-knowledge` from a published channel first, such as a verified GitHub
Release archive or `cargo install relay-knowledge` from crates.io.

## Protocol Boundary

This skill is intentionally CLI-only. It does not configure MCP, call MCP
tools, manage ACP sessions, or replace protocol-level agent access. Use the
project MCP/ACP documentation for those integrations.
