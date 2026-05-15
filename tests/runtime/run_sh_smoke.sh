#!/usr/bin/env sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

if [ ! -x "./target/release/relay-knowledge" ]; then
  echo "[Error] missing ./target/release/relay-knowledge. Run cargo build --release first."
  exit 1
fi

if [ ! -f "./web/dist/index.html" ]; then
  echo "[Error] missing ./web/dist/index.html. Run npm run build --prefix web first."
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "[Error] python3 is required for runtime smoke setup and probes."
  exit 1
fi

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/relay-knowledge-runtime-smoke.XXXXXX")"
APP_HOME="$TMP_ROOT/home"
PORT="$(
  python3 - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"

cleanup() {
  RELAY_KNOWLEDGE_HOME="$APP_HOME" ./run.sh stop --port "$PORT" --force >/dev/null 2>&1 || true
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT INT TERM

mkdir -p "$APP_HOME/data"
python3 - "$APP_HOME/data/relay-knowledge.sqlite" <<'PY'
import sqlite3
import sys

connection = sqlite3.connect(sys.argv[1])
connection.executescript(
    """
    CREATE TABLE graph_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        graph_version INTEGER NOT NULL
    );
    INSERT INTO graph_state (id, graph_version) VALUES (1, 1);

    CREATE TABLE evidence (
        id TEXT PRIMARY KEY,
        source_scope TEXT NOT NULL,
        content TEXT NOT NULL,
        created_graph_version INTEGER NOT NULL
    );
    INSERT INTO evidence (id, source_scope, content, created_graph_version)
    VALUES ('ev-runtime-smoke', 'docs', 'Runtime smoke preserves legacy storage data', 1);

    CREATE TABLE graph_mutations (
        graph_version INTEGER PRIMARY KEY,
        evidence_count INTEGER NOT NULL,
        entity_count INTEGER NOT NULL
    );
    INSERT INTO graph_mutations (graph_version, evidence_count, entity_count)
    VALUES (1, 1, 0);

    CREATE TABLE index_refresh_tasks (
        task_id TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        source_scope TEXT NOT NULL,
        modality TEXT NOT NULL,
        target_graph_version INTEGER NOT NULL,
        state TEXT NOT NULL,
        attempt_count INTEGER NOT NULL,
        next_retry_at_ms INTEGER NOT NULL,
        input_fingerprint TEXT NOT NULL,
        cursor_before INTEGER NOT NULL,
        cursor_after INTEGER,
        last_error_kind TEXT,
        last_error_message TEXT
    );
    INSERT INTO index_refresh_tasks (
        task_id, kind, source_scope, modality, target_graph_version, state,
        attempt_count, next_retry_at_ms, input_fingerprint, cursor_before,
        cursor_after, last_error_kind, last_error_message
    )
    VALUES (
        'bm25:graph:text', 'bm25', 'graph', 'text', 1, 'queued',
        0, 0, 'legacy-fingerprint', 0, NULL, NULL, NULL
    );
    """
)
connection.close()
PY

if ! RELAY_KNOWLEDGE_HOME="$APP_HOME" ./run.sh start --port "$PORT" --daemon; then
  cat "$APP_HOME/logs/web-$PORT.log" >&2 || true
  exit 1
fi

python3 - "$PORT" "$APP_HOME/data/relay-knowledge.sqlite" <<'PY'
import json
import sqlite3
import sys
import time
import urllib.request

port = sys.argv[1]
database_path = sys.argv[2]


def get_json(path: str) -> dict:
    with urllib.request.urlopen(f"http://127.0.0.1:{port}{path}", timeout=2) as response:
        return json.loads(response.read().decode("utf-8"))


deadline = time.time() + 30
last_error = None
while time.time() < deadline:
    try:
        health = get_json("/api/health")
        project = get_json("/api/project/status")
        service = get_json("/api/service/status")
        if (
            "healthy" in health
            and project["project_name"] == "relay-knowledge"
            and project["metadata"]["graph_version"] == 1
            and service["metadata"]["graph_version"] == 1
        ):
            break
    except Exception as error:  # noqa: BLE001 - printed below for smoke diagnostics.
        last_error = error
    time.sleep(0.5)
else:
    raise SystemExit(f"runtime smoke probes failed: {last_error!r}")

connection = sqlite3.connect(database_path)
evidence_count = connection.execute("SELECT COUNT(*) FROM evidence").fetchone()[0]
task_columns = {
    row[1] for row in connection.execute("PRAGMA table_info(index_refresh_tasks)").fetchall()
}
connection.close()

if evidence_count != 1:
    raise SystemExit(f"expected preserved evidence row, got {evidence_count}")
if "lease_owner" not in task_columns:
    raise SystemExit("expected migrated index_refresh_tasks.lease_owner column")
PY
