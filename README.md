# relay-knowledge

基于图数据库的知识图谱。

## Development

This is a Rust project. Install Rust through `rustup`, then run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run
```

Optional local pre-commit checks:

```bash
pre-commit install
pre-commit run --all-files
```

Setup helpers are also available:

```bash
./setup.sh
```

On Windows, run `setup.bat` from Command Prompt.
