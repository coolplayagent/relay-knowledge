# relay-knowledge

基于图数据库的知识图谱。

## Documentation

文档按用途归档在 [`docs/`](docs/README.md):

- `docs/research/`: 知识图谱、GraphRAG、arXiv 论文研究总结。
- `docs/specs/`: 能力规格、参考实现分析和后续接口规格。

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
