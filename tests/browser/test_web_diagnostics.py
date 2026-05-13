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
        requests: list[str] = []
        page.on("request", lambda request: requests.append(request.url))
        page.goto(base_url)

        expect(page.get_by_role("main").get_by_text("relay-knowledge", exact=True)).to_be_visible()
        expect(page.get_by_text("Graph version 7")).to_be_visible()
        expect(page.get_by_text("degraded").first).to_be_visible()
        expect(page.get_by_text("Code files")).to_be_visible()
        expect(page.get_by_text("12", exact=True)).to_be_visible()
        expect(page.get_by_role("heading", name="GraphRAG readiness")).to_be_visible()
        expect(page.get_by_text("BM25 read model")).to_be_visible()
        expect(page.get_by_text("Semantic cursor")).to_be_visible()
        expect(page.get_by_text("version 3 / lag 1")).to_be_visible()
        expect(page.get_by_role("cell", name="bm25")).to_be_visible()
        expect(page.get_by_text("127.0.0.1:9900")).to_be_visible()
        expect(page.get_by_role("navigation", name="Primary")).to_be_visible()
        expect(page.locator("aside nav a")).to_have_count(5)
        assert page.locator("link[rel='icon']").get_attribute("href", timeout=5000).startswith(
            "data:image/svg+xml"
        )
        assert f"{base_url}/favicon.ico" not in requests

        first_nav_color = page.locator("aside nav a").first.evaluate(
            "node => getComputedStyle(node).color"
        )
        assert first_nav_color != "rgb(0, 0, 238)"

        page.get_by_label("Query").fill("graph backpressure")
        page.get_by_label("Freshness").select_option("wait-until-fresh")
        expect(page.locator(".command-preview")).to_contain_text(
            "relay-knowledge query 'graph backpressure'"
        )
        page.get_by_test_id("stage-operation").click()
        expect(page.locator(".staged-list").get_by_text("Retrieve context")).to_be_visible()

        page.get_by_role("tab", name="Ingest").click()
        page.get_by_label("Content").fill("Evidence changed through the Web workspace")
        page.get_by_test_id("stage-operation").click()
        expect(page.locator(".staged-list").get_by_text("Ingest evidence")).to_be_visible()

        page.get_by_role("tab", name="Code").click()
        page.get_by_label("Action").select_option("impact")
        expect(page.get_by_label("Base")).to_be_visible()
        expect(page.locator(".command-preview")).to_contain_text("repo impact core")

        page.set_viewport_size({"width": 390, "height": 844})
        expect(page.locator("aside nav a")).to_have_count(5)
        expect(page.get_by_text("Runtime budgets")).to_be_visible()
        mobile_link_display = page.locator("aside nav a").first.evaluate(
            "node => getComputedStyle(node).display"
        )
        assert mobile_link_display == "block"


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
        "relation_count": 2,
        "claim_count": 4,
        "event_count": 1,
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
    "index_cursors": [
        {
            "kind": "bm25",
            "source_scope": "docs",
            "modality": "text",
            "index_version": 3,
            "indexed_graph_version": 6,
            "state": "stale",
        },
        {
            "kind": "semantic",
            "source_scope": "docs",
            "modality": "text",
            "index_version": 3,
            "indexed_graph_version": 7,
            "state": "fresh",
        },
        {
            "kind": "vector",
            "source_scope": "docs",
            "modality": "text",
            "index_version": 3,
            "indexed_graph_version": 7,
            "state": "fresh",
        },
    ],
    "index_refresh": {
        "queue_depth": 2,
        "running_count": 0,
        "retrying_count": 1,
        "dead_letter_count": 0,
        "oldest_unfinished_age_ms": 1200,
        "index_lag_by_kind": [
            {"kind": "bm25", "lag_versions": 1},
            {"kind": "semantic", "lag_versions": 0},
            {"kind": "vector", "lag_versions": 0},
        ],
        "max_index_lag_versions": 1,
        "stale_index_count": 1,
    },
    "runtime": RUNTIME,
}
