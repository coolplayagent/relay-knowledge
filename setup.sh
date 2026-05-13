#!/usr/bin/env sh
set -eu

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

load_cargo_env() {
  if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
  fi
}

echo "Checking Rust toolchain..."
if ! command_exists rustup; then
  echo "rustup not found, installing Rust toolchain..."
  if ! command_exists curl; then
    echo "[Error] curl not found. Install Rust from https://rustup.rs/ and rerun this script."
    exit 1
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  load_cargo_env
fi

if ! command_exists rustup; then
  echo "[Error] rustup is unavailable after installation."
  exit 1
fi

rustup toolchain install stable --profile minimal --component rustfmt --component clippy
rustup component add rustfmt clippy
load_cargo_env

if ! command_exists cargo; then
  echo "[Error] cargo not found. Ensure Rust is installed and ~/.cargo/bin is on PATH."
  exit 1
fi

echo "Checking pre-commit..."
PYTHON_BIN=""
if command_exists python3; then
  PYTHON_BIN="python3"
elif command_exists python; then
  PYTHON_BIN="python"
fi

PRE_COMMIT_MODE=""
if command_exists pre-commit; then
  PRE_COMMIT_MODE="executable"
elif [ -n "$PYTHON_BIN" ] && "$PYTHON_BIN" -m pre_commit --version >/dev/null 2>&1; then
  PRE_COMMIT_MODE="python-module"
elif [ -n "$PYTHON_BIN" ]; then
  echo "pre-commit not found, installing pre-commit..."
  if "$PYTHON_BIN" -m pip install --user pre-commit; then
    PRE_COMMIT_MODE="python-module"
  else
    echo "[Warning] pre-commit install failed. Run ./check.sh before committing."
  fi
else
  echo "[Warning] Python not found. Skipping pre-commit installation."
fi

run_pre_commit() {
  if [ "$PRE_COMMIT_MODE" = "python-module" ]; then
    "$PYTHON_BIN" -m pre_commit "$@"
    return
  fi
  pre-commit "$@"
}

if [ -n "$PRE_COMMIT_MODE" ]; then
  echo "Installing git hooks..."
  if run_pre_commit install; then
    echo "Git hooks install successful."
  else
    echo "[Warning] Git hooks install failed. Run ./check.sh before committing."
  fi
fi

if command_exists npm; then
  echo "npm found."
else
  echo "[Warning] npm not found. Install Node.js/npm before running ./build.sh for Web assets."
fi

if command_exists uv; then
  echo "uv found."
else
  echo "[Warning] uv not found. Browser integration tests in ./check.sh will be skipped."
fi

echo "Environment setup completed. Use ./build.sh to build and ./check.sh to verify."
