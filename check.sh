#!/usr/bin/env sh
set -eu

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

echo "Running Rust formatting check..."
cargo fmt --all -- --check

echo "Running Rust clippy..."
cargo clippy --all-targets --all-features -- -D warnings

echo "Running Rust tests..."
cargo test --all-targets --all-features

echo "Running skill metadata gate..."
manifest_version="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
python3 tools/release/update_skill_metadata_version.py --self-test --check \
  skills/relay-knowledge-cli/SKILL.md "$manifest_version"

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "Installing cargo-llvm-cov..."
  cargo install cargo-llvm-cov --locked
fi

echo "Running coverage gate..."
cargo llvm-cov --all-targets --all-features --fail-under-lines 90

if command_exists npm; then
  echo "Building Web assets..."
  npm run build --prefix web
  if command_exists python3; then
    echo "Running run.sh runtime smoke gate..."
    cargo build --release
    sh tests/runtime/run_sh_smoke.sh
  else
    echo "[Warning] python3 not found. Skipping run.sh runtime smoke gate."
  fi
else
  echo "[Warning] npm not found. Skipping Web build and run.sh runtime smoke gate."
fi

if command_exists uv; then
  echo "Running browser integration gate..."
  uv sync --extra dev --no-default-groups
  if [ -n "${CI:-}" ]; then
    uv run --extra dev python -m playwright install --with-deps chromium
  elif ! uv run --extra dev python -m playwright install --with-deps chromium; then
    echo "[Warning] Playwright system dependency install failed; retrying local Chromium install without system packages."
    uv run --extra dev python -m playwright install chromium
  fi
  uv run --extra dev pytest tests/browser
else
  echo "[Warning] uv not found. Skipping browser integration gate."
fi

echo "All checks completed."
