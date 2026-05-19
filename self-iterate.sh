#!/usr/bin/env sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
HARNESS_DIR="$ROOT_DIR/tools/self_iteration"
BUILD_ARGS=""
TARGET_DIR="debug"
if [ "${RELAY_KNOWLEDGE_SELF_ITERATION_RELEASE:-0}" = "1" ]; then
  BUILD_ARGS="--release"
  TARGET_DIR="release"
fi
HARNESS_BIN="$HARNESS_DIR/target/$TARGET_DIR/relay-knowledge-self-iterate"

if [ "${1:-}" = "once" ] || [ "${1:-}" = "loop" ] || [ "${1:-}" = "evaluate" ] || [ "${1:-}" = "chart" ]; then
  MODE="$1"
  shift
else
  MODE="loop"
fi

if [ ! -x "$HARNESS_BIN" ] || [ -n "$(find "$HARNESS_DIR/src" "$HARNESS_DIR/Cargo.toml" -newer "$HARNESS_BIN" -print -quit 2>/dev/null)" ]; then
  cargo build $BUILD_ARGS --manifest-path "$HARNESS_DIR/Cargo.toml" --bin relay-knowledge-self-iterate
fi

exec "$HARNESS_BIN" "$MODE" --workspace "$ROOT_DIR" --yolo "$@"
