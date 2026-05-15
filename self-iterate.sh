#!/usr/bin/env sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

if [ "${1:-}" = "once" ] || [ "${1:-}" = "loop" ] || [ "${1:-}" = "evaluate" ] || [ "${1:-}" = "chart" ]; then
  MODE="$1"
  shift
else
  MODE="loop"
fi

exec python3 "$ROOT_DIR/tools/self_iteration/self_iterate.py" "$MODE" --yolo "$@"
