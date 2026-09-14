#!/usr/bin/env sh
set -eu

case "$*" in
  "run nightly rustc --version")
    if [ "${CHECK_SH_RUSTUP_CASE:-complete}" = "dated-only" ]; then
      exit 1
    fi
    echo "rustc 1.93.0-nightly (fixture)"
    ;;
  "component list --toolchain nightly --installed")
    echo "miri-x86_64-unknown-linux-gnu"
    if [ "${CHECK_SH_RUSTUP_CASE:-complete}" != "missing-rust-src" ]; then
      echo "rust-src"
    fi
    ;;
  *)
    echo "Unexpected rustup fixture invocation: $*" >&2
    exit 64
    ;;
esac
