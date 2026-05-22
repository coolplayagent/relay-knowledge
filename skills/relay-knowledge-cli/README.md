# Relay Knowledge CLI Skill

This ClawHub-compatible skill teaches LLM agents to operate `relay-knowledge`
through the local CLI. It is for local knowledge graph ingestion, hybrid
GraphRAG queries, code repository indexing, code graph search, multi-repository
queries, impact analysis, setup diagnostics, installation checks, and upgrade
checks.

## Package Contents

- `SKILL.md`: agent instructions and skill metadata.
- `agents/openai.yaml`: UI metadata for OpenAI-compatible agent surfaces.
- `references/cli-workflows.md`: detailed CLI workflows and safety defaults.
- `assets/linux-x86_64/relay-knowledge`: Linux x64 release binary in generated
  release packages.
- `assets/windows-x86_64/relay-knowledge.exe`: Windows x64 release binary in
  generated release packages.

## Runtime Selection

Resolve `relay-knowledge` before running workflow commands. Compare the
published binary on `PATH` with the bundled asset binary for the current
platform by running `version --format json` for each usable candidate. Use the
newest semver version. If versions are equal, prefer the `PATH` binary so
user-managed installs remain authoritative.

If the current operating system or CPU architecture has no bundled asset,
install `relay-knowledge` from a published channel first, such as a verified
GitHub Release archive or `cargo install relay-knowledge` from crates.io.

## Protocol Boundary

This skill is intentionally CLI-only. It does not configure MCP, call MCP
tools, manage ACP sessions, or replace protocol-level agent access. Use the
project MCP/ACP documentation for those integrations.
