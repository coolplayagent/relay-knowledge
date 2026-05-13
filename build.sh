#!/usr/bin/env sh
set -eu

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

PROFILE="release"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --debug)
      PROFILE="debug"
      shift
      ;;
    -h|--help)
      echo "Usage: ./build.sh [--debug]"
      exit 0
      ;;
    *)
      echo "[Error] unexpected argument: $1"
      exit 2
      ;;
  esac
done

if [ "$PROFILE" = "debug" ]; then
  echo "Building relay-knowledge CLI debug binary..."
  cargo build
else
  echo "Building relay-knowledge CLI release binary..."
  cargo build --release
fi

if command_exists npm; then
  echo "Building Web assets..."
  npm install --prefix web
  npm run build --prefix web
else
  echo "[Error] npm not found. Install Node.js/npm to build web/dist."
  exit 1
fi

echo "Build completed."
