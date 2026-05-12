from __future__ import annotations

import contextlib
import functools
import http.server
import json
import socketserver
import threading
from pathlib import Path

from playwright.sync_api import Page, expect


def test_web_diagnostics_render_browser_contract(page: Page) -> None:
    web_dist = Path(__file__).resolve().parents[2] / "web" / "dist"
    assert web_dist.exists(), "run `npm --prefix web run build` before browser tests"

    with serve_directory(web_dist) as base_url:
        page.goto(base_url)

        expect(page.get_by_role("main").get_by_text("relay-knowledge", exact=True)).to_be_visible()
        expect(page.get_by_text("Graph version 7")).to_be_visible()
        expect(page.get_by_text("degraded")).to_be_visible()
        expect(page.get_by_text("code files 12")).to_be_visible()
        expect(page.get_by_role("cell", name="bm25")).to_be_visible()
        expect(page.get_by_text("127.0.0.1:9900")).to_be_visible()


@contextlib.contextmanager
def serve_directory(directory: Path):
    handler = functools.partial(DiagnosticsHandler, directory=directory)
    with socketserver.TCPServer(("127.0.0.1", 0), handler) as server:
        thread = threading.Thread(target=server.serve_forever)
        thread.daemon = True
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_address[1]}"
        finally:
            server.shutdown()
            thread.join(timeout=5)


class DiagnosticsHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self) -> None:
        path = self.path.split("?", 1)[0]
        if path == "/api/project/status":
            self.write_json(PROJECT_STATUS_RESPONSE)
        elif path == "/api/health":
            self.write_json(HEALTH_RESPONSE)
        else:
            super().do_GET()

    def write_json(self, payload: dict) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


RUNTIME = {
    "config_dir": "/srv/relay/config",
    "data_dir": "/srv/relay/data",
    "state_dir": "/srv/relay/state",
    "cache_dir": "/srv/relay/cache",
    "log_dir": "/srv/relay/logs",
    "temp_dir": "/tmp/relay-knowledge",
    "runtime_dir": "/srv/relay/run",
    "service_dir": "/srv/relay/service",
    "http_bind": "127.0.0.1:9900",
    "http_request_timeout_ms": 30000,
    "http_graceful_shutdown_timeout_ms": 10000,
    "http_max_request_body_bytes": 1048576,
    "http_proxy_configured": False,
    "http_no_proxy_rules": 0,
    "http_ssl_verify": True,
    "qos_max_connections": 1024,
    "qos_max_in_flight_requests": 256,
    "qos_max_queue_depth": 512,
}

PROJECT_STATUS_RESPONSE = {
    "project_name": "relay-knowledge",
    "metadata": {
        "trace_id": "trace-web-live",
        "request_id": "req-web-live",
        "graph_version": 7,
        "stale": True,
    },
    "runtime": RUNTIME,
}

HEALTH_RESPONSE = {
    "metadata": {
        "trace_id": "trace-health-live",
        "request_id": "req-health-live",
        "graph_version": 7,
        "indexed_graph_version": 6,
        "stale": True,
    },
    "healthy": False,
    "graph": {
        "graph_version": 7,
        "entity_count": 3,
        "evidence_count": 5,
        "mutation_count": 4,
        "code_file_count": 12,
        "code_symbol_count": 48,
        "code_reference_count": 125,
        "code_chunk_count": 37,
        "code_parse_status_counts": {
            "parsed": 10,
            "partial": 1,
            "text_only": 1,
            "failed": 0,
        },
    },
    "indexes": [
        {
            "kind": "bm25",
            "index_version": 3,
            "indexed_graph_version": 6,
            "state": "stale",
        },
        {
            "kind": "semantic",
            "index_version": 3,
            "indexed_graph_version": 7,
            "state": "fresh",
        },
        {
            "kind": "vector",
            "index_version": 3,
            "indexed_graph_version": 7,
            "state": "fresh",
        },
    ],
    "runtime": RUNTIME,
}
