#!/usr/bin/env sh
set -eu

DEFAULT_PORT="8791"
COMMAND="${1:-}"

usage() {
  echo "Usage: ./run.sh start|restart|stop|status [--port <n>] [--daemon] [--force]"
}

if [ -z "$COMMAND" ] || [ "$COMMAND" = "-h" ] || [ "$COMMAND" = "--help" ]; then
  usage
  exit 0
fi
shift || true

PORT="$DEFAULT_PORT"
DAEMON="false"
FORCE="false"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --port)
      PORT="${2:-}"
      if [ -z "$PORT" ]; then
        echo "[Error] missing value for --port"
        exit 2
      fi
      shift 2
      ;;
    --port=*)
      PORT="${1#--port=}"
      shift
      ;;
    --daemon)
      DAEMON="true"
      shift
      ;;
    --force)
      FORCE="true"
      shift
      ;;
    *)
      echo "[Error] unexpected argument: $1"
      usage
      exit 2
      ;;
  esac
done

case "$PORT" in
  ''|*[!0-9]*)
    echo "[Error] --port must be a positive integer"
    exit 2
    ;;
esac

if [ "$PORT" -lt 1 ] || [ "$PORT" -gt 65535 ]; then
  echo "[Error] --port must be between 1 and 65535"
  exit 2
fi

APP_HOME="${RELAY_KNOWLEDGE_HOME:-}"
if [ -n "$APP_HOME" ]; then
  RUNTIME_DIR="${RELAY_KNOWLEDGE_RUNTIME_DIR:-$APP_HOME/run}"
  LOG_DIR="${RELAY_KNOWLEDGE_LOG_DIR:-$APP_HOME/logs}"
else
  STATE_BASE="${XDG_STATE_HOME:-$HOME/.local/state}"
  RUNTIME_DIR="${RELAY_KNOWLEDGE_RUNTIME_DIR:-$STATE_BASE/relay-knowledge/run}"
  LOG_DIR="${RELAY_KNOWLEDGE_LOG_DIR:-$STATE_BASE/relay-knowledge/logs}"
fi

PID_FILE="$RUNTIME_DIR/web-$PORT.pid"
LOG_FILE="$LOG_DIR/web-$PORT.log"
BIN="./target/release/relay-knowledge"
WEB_INDEX="./web/dist/index.html"

pid_alive() {
  [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" >/dev/null 2>&1
}

require_built_artifacts() {
  if [ ! -x "$BIN" ]; then
    echo "[Error] missing $BIN. Run ./build.sh first."
    exit 1
  fi
  if [ ! -f "$WEB_INDEX" ]; then
    echo "[Error] missing $WEB_INDEX. Run ./build.sh first."
    exit 1
  fi
}

stop_service() {
  if ! pid_alive; then
    rm -f "$PID_FILE"
    echo "relay-knowledge web service is not running on port $PORT."
    return
  fi

  PID="$(cat "$PID_FILE")"
  echo "Stopping relay-knowledge web service on port $PORT (pid $PID)..."
  kill "$PID" >/dev/null 2>&1 || true

  COUNT=0
  while kill -0 "$PID" >/dev/null 2>&1 && [ "$COUNT" -lt 30 ]; do
    COUNT=$((COUNT + 1))
    sleep 1
  done

  if kill -0 "$PID" >/dev/null 2>&1; then
    if [ "$FORCE" = "true" ]; then
      echo "Force killing pid $PID..."
      kill -KILL "$PID" >/dev/null 2>&1 || true
    else
      echo "[Error] service did not stop within 30s. Re-run with --force."
      exit 1
    fi
  fi

  rm -f "$PID_FILE"
  echo "Stopped."
}

start_service() {
  require_built_artifacts
  mkdir -p "$RUNTIME_DIR" "$LOG_DIR"

  if pid_alive; then
    if [ "$FORCE" = "true" ]; then
      stop_service
    else
      echo "[Error] service already running on port $PORT (pid $(cat "$PID_FILE")). Use --force to replace it."
      exit 1
    fi
  else
    rm -f "$PID_FILE"
  fi

  export RELAY_KNOWLEDGE_HTTP_BIND="127.0.0.1:$PORT"
  if [ "$DAEMON" = "true" ]; then
    echo "Starting relay-knowledge web service on http://127.0.0.1:$PORT ..."
    nohup "$BIN" service run --web --mcp streamable-http </dev/null >"$LOG_FILE" 2>&1 &
    PID="$!"
    echo "$PID" >"$PID_FILE"
    sleep 1
    if ! kill -0 "$PID" >/dev/null 2>&1; then
      rm -f "$PID_FILE"
      echo "[Error] service failed to start. See $LOG_FILE."
      exit 1
    fi
    echo "Started pid $PID. Log: $LOG_FILE"
  else
    echo "Starting relay-knowledge web service on http://127.0.0.1:$PORT ..."
    exec "$BIN" service run --web --mcp streamable-http
  fi
}

case "$COMMAND" in
  start)
    start_service
    ;;
  restart)
    stop_service
    start_service
    ;;
  stop)
    stop_service
    ;;
  status)
    if pid_alive; then
      echo "relay-knowledge web service is running on http://127.0.0.1:$PORT (pid $(cat "$PID_FILE"))."
      echo "Log: $LOG_FILE"
    else
      rm -f "$PID_FILE"
      echo "relay-knowledge web service is not running on port $PORT."
    fi
    ;;
  *)
    echo "[Error] unknown command: $COMMAND"
    usage
    exit 2
    ;;
esac
