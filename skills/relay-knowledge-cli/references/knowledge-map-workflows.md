# Knowledge Map Workflows

Use `relay-knowledge map` when a user asks where project knowledge lives, how
agents should navigate repository knowledge, how to add or update a knowledge
source, or how AGENTS.md should reference shared knowledge navigation.

The shared contract is `.knowledge/knowledge-map.yaml`. Treat it as a versioned
repository contract. Prefer CLI CRUD commands over direct YAML edits so
`map_version`, route references, and history stay consistent.

## Agent Decision Rules

- Before reading or changing the contract, run `relay-knowledge map validate
  --format json`.
- For read-only navigation, run `relay-knowledge map show --format json` or
  `relay-knowledge map route <topic> --format json`.
- Before adding a source, check whether the target topic or source already
  exists with `map show`.
- A topic can contain multiple sources. Add each source with a distinct
  `--id`; the topic route keeps the ordered `source_order` list.
- Use `map source add`, `map source update`, or `map source remove` for changes.
- After every mutation, run `relay-knowledge map validate --format json`.
- Do not copy the full YAML into AGENTS.md. AGENTS.md should only reference
  `.knowledge/knowledge-map.yaml`.
- Edit YAML directly only when the CLI is unavailable and the user explicitly
  asks for manual repair.

## POSIX Examples

Initialize the contract:

```bash
relay-knowledge map init --format json
relay-knowledge map agent-snippet --format text
relay-knowledge map validate --format json
```

Add a source:

```bash
relay-knowledge map source add \
  --id cli-reference \
  --topic cli \
  --kind doc \
  --uri docs/zh/01-user-guide/03-cli-command-reference.md \
  --scope docs \
  --description "CLI command reference" \
  --format json
relay-knowledge map validate --format json
```

Update and route:

```bash
relay-knowledge map source update \
  --id cli-reference \
  --description "User-facing CLI command reference" \
  --format json
relay-knowledge map route cli --format json
```

Remove a source:

```bash
relay-knowledge map source remove --id cli-reference --format json
relay-knowledge map validate --format json
```

## PowerShell Examples

```powershell
relay-knowledge map init --format json
relay-knowledge map source add --id cli-reference --topic cli --kind doc --uri docs/zh/01-user-guide/03-cli-command-reference.md --scope docs --description "CLI command reference" --format json
relay-knowledge map validate --format json
```

## cmd.exe Examples

```cmd
relay-knowledge map init --format json
relay-knowledge map source add --id cli-reference --topic cli --kind doc --uri docs/zh/01-user-guide/03-cli-command-reference.md --scope docs --description "CLI command reference" --format json
relay-knowledge map validate --format json
```
