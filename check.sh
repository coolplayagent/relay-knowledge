#!/usr/bin/env sh
set -eu

QUALITY_PROFILE="standard"
case "${1:-}" in
  ""|--standard)
    ;;
  --deep)
    QUALITY_PROFILE="deep"
    ;;
  --help|-h)
    echo "Usage: ./check.sh [--standard|--deep]"
    echo "  --standard  Run deterministic stable-toolchain repository gates (default)."
    echo "  --deep      Also run benchmark, Miri, and AddressSanitizer gates."
    exit 0
    ;;
  *)
    echo "[Error] Unknown check profile: $1" >&2
    echo "Usage: ./check.sh [--standard|--deep]" >&2
    exit 2
    ;;
esac

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

sanitizer_target=""
if [ "$QUALITY_PROFILE" = "deep" ]; then
  if ! command_exists rustup || ! rustup run nightly rustc --version >/dev/null 2>&1; then
    echo "[Error] The deep profile requires the nightly Rust toolchain." >&2
    echo "Install it with: rustup toolchain install nightly --profile minimal --component miri,rust-src" >&2
    exit 1
  fi

  nightly_components="$(rustup component list --toolchain nightly --installed)"
  if ! printf '%s\n' "$nightly_components" | grep -Eq '^miri(-[[:alnum:]_.-]+)?$'; then
    echo "[Error] The deep profile requires the nightly Miri component." >&2
    echo "Install it with: rustup component add miri --toolchain nightly" >&2
    exit 1
  fi

  if ! printf '%s\n' "$nightly_components" | grep -Eq '^rust-src$'; then
    echo "[Error] The deep profile requires nightly rust-src for instrumented standard-library builds." >&2
    echo "Install it with: rustup component add rust-src --toolchain nightly" >&2
    exit 1
  fi

  case "$(uname -s):$(uname -m)" in
    Linux:x86_64)
      sanitizer_target="x86_64-unknown-linux-gnu"
      ;;
    Linux:aarch64|Linux:arm64)
      sanitizer_target="aarch64-unknown-linux-gnu"
      ;;
    Darwin:x86_64)
      sanitizer_target="x86_64-apple-darwin"
      ;;
    Darwin:aarch64|Darwin:arm64)
      sanitizer_target="aarch64-apple-darwin"
      ;;
    *)
      echo "[Error] AddressSanitizer is not configured for $(uname -s) $(uname -m)." >&2
      exit 1
      ;;
  esac
fi

echo "Running documentation structure and link check..."
python3 tools/docs/check_docs.py --self-test
python3 tools/docs/check_docs.py

echo "Running Rust formatting check..."
cargo fmt --all -- --check

echo "Running Rust type and build check..."
cargo check --all-targets --all-features

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

if [ "$QUALITY_PROFILE" = "deep" ]; then
  echo "Running deterministic benchmark gate..."
  cargo test --test benchmarks --all-features -- --nocapture

  echo "Preparing Miri..."
  cargo +nightly miri setup

  echo "Running Miri against FFI-free core domain invariants..."
  MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-symbolic-alignment-check -Zmiri-deterministic-concurrency" \
    cargo +nightly miri test --lib --all-features domain::core::

  echo "Running AddressSanitizer on $sanitizer_target..."
  ASAN_OPTIONS="detect_leaks=1" \
    RUSTFLAGS="-Zsanitizer=address" \
    RUSTDOCFLAGS="-Zsanitizer=address" \
    cargo +nightly test -Zbuild-std --target "$sanitizer_target" \
      --lib --bins --all-features
fi

echo "All checks completed."
