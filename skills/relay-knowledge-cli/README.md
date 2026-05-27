# Relay Knowledge CLI Skill

This ClawHub-compatible skill teaches LLM agents to operate `relay-knowledge`
through the local CLI. It is for local knowledge graph ingestion, hybrid
GraphRAG queries, code repository indexing, code graph search, multi-repository
queries, feature flag graph queries, impact analysis, setup diagnostics,
installation checks, and upgrade checks.

For code-structure questions such as function definitions, symbol locations,
references, callers, callees, call graphs, and call chains, agents should use
this skill before `grep`, `ripgrep`, `rg`, or plain text search. Fall back to
text search only when the CLI cannot satisfy the request, the target repository
cannot be indexed, or the user explicitly needs raw text or regular-expression
matching.

For `repo query --kind` prompts, the supported code query kinds are `hybrid`,
`symbol`, `definition`, `references`, `callers`, `callees`, and `imports`.
Agents should choose one of these kinds first and treat `grep`/`rg` as fallback
tools, not the preferred path.

For feature flag, config gate, environment-variable gate, settings gate, or
guarded-code questions, agents should use `repo feature-flags`. Feature flags
are not a `repo query --kind` value.

## Package Contents

- `SKILL.md`: agent instructions and skill metadata.
- `agents/openai.yaml`: UI metadata for OpenAI-compatible agent surfaces.
- `references/cli-workflows.md`: detailed CLI workflows and safety defaults.
- `assets/linux-x86_64/relay-knowledge`: Linux x64 release binary in generated
  release packages, built and checked against the glibc 2.31 baseline.
- `assets/windows-x86_64/relay-knowledge.exe`: Windows x64 release binary in
  generated release packages.

Keep the `SKILL.md` frontmatter `description` at or below 1024 Unicode
characters. Local checks, pre-commit, PR CI, release packaging, and ClawHub
publish validation all run the shared skill metadata gate. Quote the
description when it contains YAML-sensitive punctuation such as `: `.

## Runtime Selection

Resolve `relay-knowledge` before running workflow commands. Prefer the bundled
asset binary for the current operating system, CPU, and active command runner
whenever it exists, is executable, and `version --format json` succeeds. Keep
that absolute path in a shell variable and use it for every CLI command.

Do not run the Windows bundled asset from POSIX shells such as bash, sh, zsh,
fish, or WSL bash unless the command intentionally crosses into a Windows shell
boundary. Windows `.exe` examples belong in PowerShell or cmd.exe command
blocks; POSIX examples must use `assets/linux-x86_64/relay-knowledge` or a
POSIX `PATH` install.

Use a published binary on `PATH` only when the bundled asset is absent,
unusable, unsupported on the current operating system or CPU architecture,
unsupported by the active shell boundary, incompatible with the host Linux glibc
version, or explicitly requested by the user. If no usable binary is available,
install `relay-knowledge` from a published channel first, such as a verified
GitHub Release archive or `cargo install relay-knowledge` from crates.io.

## Protocol Boundary

This skill is intentionally CLI-only. It does not configure MCP, call MCP
tools, manage ACP sessions, or replace protocol-level agent access. Use the
project MCP/ACP documentation for those integrations.
