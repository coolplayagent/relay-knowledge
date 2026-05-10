# Repository Guidelines

## Project Structure & Module Organization

This repository is currently a minimal skeleton for `relay-knowledge`, a graph-database-based knowledge graph project. The root contains `README.md`, `LICENSE`, and `.gitignore`; no source tree or build manifest is present yet.

When adding implementation code, follow standard Rust layout unless the project defines a different structure:

- `Cargo.toml`: package/workspace manifest.
- `src/`: application and library source code.
- `tests/`: integration tests.
- `examples/`: runnable usage examples or demos.
- `docs/`: architecture notes, schema details, and operational guides.

Keep generated output, build products, and large temporary data out of version control.

## Build, Test, and Development Commands

No runnable build is configured yet. Once `Cargo.toml` is added, use the standard Cargo workflow:

- `cargo build`: compile the project.
- `cargo test`: run unit and integration tests.
- `cargo fmt --check`: verify formatting without rewriting files.
- `cargo clippy --all-targets --all-features -- -D warnings`: run lint checks and fail on warnings.
- `cargo run`: run the default binary, if one exists.

Document required services, such as graph databases or local containers, in `README.md` and commit example configuration files.

## Coding Style & Naming Conventions

Use idiomatic Rust conventions: four-space indentation, `snake_case` for functions/modules, `PascalCase` for types and traits, and `SCREAMING_SNAKE_CASE` for constants. Prefer small modules with clear ownership. Run `cargo fmt` before committing Rust code.

Configuration and documentation files should use descriptive names, for example `docs/graph-schema.md` or `examples/load_dataset.rs`.

## Testing Guidelines

Place unit tests next to the code they exercise using `#[cfg(test)]`. Put cross-module tests in `tests/`. Name tests after observable behavior, such as `creates_node_when_entity_is_new`.

For graph/database behavior, prefer deterministic fixtures and isolate tests from developer-local state. If tests require an external service, provide setup instructions and sensible defaults.

## Commit & Pull Request Guidelines

The current history only contains an initial commit, so no repository-specific convention is established. Use short, imperative subjects, for example `Add graph schema loader` or `Document local database setup`.

Pull requests should include a summary, reason for the change, test results, and configuration or migration notes. Link related issues when available. Include screenshots only for UI or documentation rendering changes.

## Security & Configuration Tips

Do not commit secrets, database credentials, private datasets, or local environment files. Commit safe templates such as `.env.example` when configuration is required. Prefer explicit environment variables and document them in `README.md`.
