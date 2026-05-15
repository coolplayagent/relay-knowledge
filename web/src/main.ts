import type {
  HealthResponse,
  IndexStatus,
  ProjectStatusResponse,
  ServiceStatusResponse
} from "./api/contracts";
import { loadHealth, loadProjectStatus, loadServiceStatus } from "./api/client.js";
import { graphCanvasSection } from "./graph_canvas.js";
import { operationsSection } from "./operations_panel.js";
import { providersSection } from "./providers.js";
import { maxIndexLag } from "./operations.js";
import { currentTheme, initializeTheme, toggleTheme } from "./theme.js";
import { element, icon, sectionShell, statusPill, textElement, type Tone } from "./ui.js";

type Diagnostics = {
  status: ProjectStatusResponse;
  health: HealthResponse;
  service: ServiceStatusResponse | null;
};

type PageId = "status" | "readiness" | "graph" | "providers" | "operations" | "indexes" | "runtime";

type PageLink = {
  id: PageId;
  label: string;
};

const PAGES: PageLink[] = [
  { id: "status", label: "Status" },
  { id: "readiness", label: "Readiness" },
  { id: "graph", label: "Graph" },
  { id: "providers", label: "Providers" },
  { id: "operations", label: "Operations" },
  { id: "indexes", label: "Indexes" },
  { id: "runtime", label: "Runtime" }
];

let currentDiagnostics: Diagnostics | null = null;
let activePage: PageId = pageFromLocation();

async function renderApp() {
  const root = document.getElementById("root");
  if (!root) {
    return;
  }

  root.replaceChildren(loadingShell());
  try {
    const [status, health, service] = await Promise.all([
      loadProjectStatus(),
      loadHealth(),
      loadServiceStatus().catch(() => null)
    ]);
    currentDiagnostics = { status, health, service };
    root.replaceChildren(shell(status, health, service));
  } catch (error) {
    root.replaceChildren(errorShell(error));
  }
}

function rerenderFromState() {
  const root = document.getElementById("root");
  if (!root || !currentDiagnostics) {
    return;
  }

  root.replaceChildren(
    shell(currentDiagnostics.status, currentDiagnostics.health, currentDiagnostics.service)
  );
}

function loadingShell(): HTMLElement {
  const container = element("div", "shell");
  const main = element("main", "content");
  main.append(sectionShell("status", "Status", textElement("div", "muted-line", "Loading")));
  container.append(sidebar(activePage), main);

  return container;
}

function shell(
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null
): HTMLElement {
  const container = element("div", "shell");
  container.append(sidebar(activePage), content(status, health, service));

  return container;
}

function sidebar(selectedPage: PageId): HTMLElement {
  const aside = element("aside", "sidebar");
  aside.setAttribute("aria-label", "Navigation");
  const nav = element("nav", "nav-list");
  nav.setAttribute("aria-label", "Primary");
  nav.append(...PAGES.map((page) => navLink(page, selectedPage)));
  aside.append(textElement("div", "brand", "relay-knowledge"), nav);

  return aside;
}

function navLink(page: PageLink, selectedPage: PageId): HTMLAnchorElement {
  const link = document.createElement("a");
  link.href = `#${page.id}`;
  link.textContent = page.label;
  if (page.id === selectedPage) {
    link.className = "active";
    link.setAttribute("aria-current", "page");
  }
  link.addEventListener("click", (event) => {
    event.preventDefault();
    setActivePage(page.id, true);
  });

  return link;
}

function content(
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null
): HTMLElement {
  const main = element("main", "content");
  main.dataset.page = activePage;
  main.append(toolbar(status, health), pageContent(activePage, status, health, service));

  return main;
}

function errorShell(error: unknown): HTMLElement {
  const container = element("div", "shell");
  const main = element("main", "content");
  const section = sectionShell("status", "Status");
  section.append(textElement("div", "error-message", errorMessage(error)));
  main.append(section);
  container.append(sidebar(activePage), main);

  return container;
}

function pageContent(
  page: PageId,
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null
): HTMLElement {
  switch (page) {
    case "status":
      return statusSection(status, health);
    case "readiness":
      return readinessSection(status, health, service);
    case "graph":
      return graphCanvasSection();
    case "providers":
      return providersSection(status, health);
    case "operations":
      return operationsSection(status, health, {
        rerender: rerenderFromState,
        setDiagnostics: (diagnostics) => {
          currentDiagnostics = diagnostics;
        },
        errorMessage
      });
    case "indexes":
      return indexesSection(health.indexes, health.metadata.graph_version);
    case "runtime":
      return runtimeSection(status, service);
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Diagnostics unavailable";
}

function setActivePage(page: PageId, updateLocation: boolean) {
  if (updateLocation && window.location.hash !== `#${page}`) {
    window.history.pushState(null, "", `#${page}`);
  }
  if (activePage !== page) {
    activePage = page;
    rerenderFromState();
  }
  document.querySelector(".content")?.scrollTo({ top: 0 });
}

function syncActivePageFromLocation() {
  const page = pageFromLocation();
  if (page !== activePage) {
    activePage = page;
    rerenderFromState();
  }
}

function pageFromLocation(): PageId {
  const candidate = window.location.hash.replace("#", "");

  return PAGES.some((page) => page.id === candidate) ? (candidate as PageId) : "status";
}

function toolbar(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const bar = element("div", "toolbar");
  const titles = element("div");
  titles.append(
    textElement("h1", "title", status.project_name),
    textElement("div", "subtitle", `Graph version ${status.metadata.graph_version}`)
  );

  const actions = element("div", "toolbar-actions");
  actions.append(
    statusPill(health.healthy ? "healthy" : "degraded", health.healthy ? "good" : "warn"),
    themeButton(),
    refreshButton()
  );
  bar.append(titles, actions);

  return bar;
}

function themeButton(): HTMLButtonElement {
  const button = document.createElement("button");
  const theme = currentTheme();
  const nextTheme = theme === "dark" ? "day" : "night";
  button.type = "button";
  button.className = "button";
  button.dataset.testid = "theme-toggle";
  button.setAttribute("aria-label", `Switch to ${nextTheme} theme`);
  button.append(
    icon(theme === "dark" ? "sun-icon" : "moon-icon"),
    document.createTextNode(theme === "dark" ? "Day" : "Night")
  );
  button.addEventListener("click", () => {
    toggleTheme();
    rerenderFromState();
  });

  return button;
}

function refreshButton(): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "button";
  button.setAttribute("aria-label", "Refresh diagnostics");
  button.append(icon("refresh-icon"), document.createTextNode("Refresh"));
  button.addEventListener("click", () => void renderApp());

  return button;
}

function statusSection(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const lag = maxIndexLag(health.indexes, status.metadata.graph_version);
  const codeTotals = codeRepositoryTotals(health);
  const section = sectionShell("status", "Status");
  const statusLine = element("div", "status-line");
  statusLine.append(
    icon(health.healthy ? "health-icon" : "warn-icon"),
    textElement("span", undefined, health.healthy ? "healthy" : "degraded"),
    textElement("span", undefined, `index lag ${lag}`),
    textElement("span", undefined, `queue ${health.index_refresh.queue_depth}`),
    textElement("span", undefined, `mutations ${health.graph.mutation_count}`)
  );

  const metrics = element("div", "metric-grid");
  metrics.append(
    metricItem("Entities", health.graph.entity_count),
    metricItem("Evidence", health.graph.evidence_count),
    metricItem("Relations", health.graph.relation_count),
    metricItem("Claims", health.graph.claim_count),
    metricItem("Events", health.graph.event_count),
    metricItem("Code files", codeTotals.indexed_file_count),
    metricItem("Symbols", codeTotals.symbol_count),
    metricItem("References", codeTotals.reference_count)
  );
  section.append(statusLine, metrics);

  return section;
}

function metricItem(label: string, value: number): HTMLElement {
  const item = element("div", "metric-item");
  item.append(textElement("dt", undefined, label), textElement("dd", undefined, String(value)));

  return item;
}

function readinessSection(
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null
): HTMLElement {
  const section = sectionShell("readiness", "GraphRAG readiness");
  const grid = element("div", "readiness-grid");
  const graph = health.graph;
  const codeTotals = codeRepositoryTotals(health);
  const graphVersion = health.metadata.graph_version;
  const bm25 = health.indexes.find((index) => index.kind === "bm25");
  const semantic = health.indexes.find((index) => index.kind === "semantic");
  const vector = health.indexes.find((index) => index.kind === "vector");
  const hasEvidence = graph.entity_count > 0 || graph.evidence_count > 0;
  const hasCodeGraph = codeTotals.indexed_file_count > 0 || codeTotals.symbol_count > 0;
  const staleSummary = staleReasonSummary(health);

  grid.append(
    readinessItem(
      "Evidence graph",
      hasEvidence ? "active" : "empty",
      hasEvidence ? "good" : "warn",
      `${graph.entity_count} entities / ${graph.evidence_count} evidence`
    ),
    readinessItem(
      "BM25 read model",
      bm25?.state ?? "missing",
      indexReadinessTone(bm25, graphVersion),
      indexReadinessDetail(bm25, graphVersion)
    ),
    readinessItem(
      "Semantic cursor",
      semantic?.state ?? "missing",
      indexReadinessTone(semantic, graphVersion),
      indexReadinessDetail(semantic, graphVersion)
    ),
    readinessItem(
      "Vector cursor",
      vector?.state ?? "missing",
      indexReadinessTone(vector, graphVersion),
      indexReadinessDetail(vector, graphVersion)
    ),
    readinessItem(
      "Code graph",
      hasCodeGraph ? "indexed" : "empty",
      hasCodeGraph ? "good" : "warn",
      `${codeTotals.indexed_file_count} files / ${codeTotals.symbol_count} symbols`
    ),
    readinessItem(
      "Runtime budgets",
      health.healthy ? "ready" : "degraded",
      health.healthy ? "good" : "warn",
      `${status.runtime.qos_max_in_flight_requests} in-flight / ${status.runtime.qos_max_queue_depth} queue`
    ),
    readinessItem(
      "Service manager",
      service?.mode ?? "unknown",
      serviceTone(service),
      service
        ? `${service.workers.length} workers / ${service.proposal_backlog} proposals`
        : "service status endpoint unavailable"
    ),
    readinessItem(
      "Refresh recovery",
      health.index_refresh.dead_letter_count > 0 ? "failed" : "ready",
      health.index_refresh.dead_letter_count > 0 ? "bad" : "good",
      `${health.index_refresh.queue_depth} queued / ${health.index_refresh.dead_letter_count} dead-letter`
    ),
    readinessItem(
      "Stale reasons",
      staleSummary.value,
      staleSummary.tone,
      staleSummary.detail
    )
  );
  section.append(grid);

  return section;
}

function codeRepositoryTotals(health: HealthResponse): HealthResponse["repository_code_totals"] {
  return (
    health.repository_code_totals ?? {
      repository_count: 0,
      indexed_file_count: health.graph.code_file_count,
      symbol_count: health.graph.code_symbol_count,
      reference_count: health.graph.code_reference_count,
      chunk_count: health.graph.code_chunk_count,
      degraded_file_count: health.graph.code_parse_status_counts.failed,
      parse_status_counts: health.graph.code_parse_status_counts
    }
  );
}

function serviceTone(service: ServiceStatusResponse | null): Tone {
  if (!service || service.operator.state === "failed") {
    return "bad";
  }
  if (
    service.operator.state === "paused" ||
    service.operator.state === "degraded" ||
    service.proposal_backlog > 0
  ) {
    return "warn";
  }

  return "good";
}

function readinessItem(label: string, value: string, tone: Tone, detail: string): HTMLElement {
  const item = element("div", "readiness-item");
  const heading = element("div", "readiness-heading");
  heading.append(textElement("span", "readiness-label", label), statusPill(value, tone));
  item.append(heading, textElement("div", "readiness-detail", detail));

  return item;
}

function indexReadinessTone(index: IndexStatus | undefined, graphVersion: number): Tone {
  if (!index || index.state === "failed") {
    return "bad";
  }
  if (index.state === "stale" || index.state === "paused" || index.indexed_graph_version < graphVersion) {
    return "warn";
  }

  return "good";
}

function indexReadinessDetail(index: IndexStatus | undefined, graphVersion: number): string {
  if (!index) {
    return "index status unavailable";
  }
  const lag = Math.max(0, graphVersion - index.indexed_graph_version);

  return `version ${index.index_version} / lag ${lag}`;
}

function staleReasonSummary(health: HealthResponse): { value: string; tone: Tone; detail: string } {
  const reasons = health.index_refresh.stale_reasons ?? [];
  if (reasons.length === 0) {
    return { value: "clear", tone: "good", detail: "no stale or failed cursor reasons" };
  }
  const failed = reasons.find((reason) => reason.last_error || reason.reason.includes("failed"));
  const scoped = reasons.find((reason) => reason.source_scope);
  const first = failed ?? scoped ?? reasons[0];
  const scope = first.source_scope ? ` / ${first.source_scope}` : "";
  const detail = `${first.kind}${scope}: ${first.reason}`;

  return {
    value: `${reasons.length} reason${reasons.length === 1 ? "" : "s"}`,
    tone: failed ? "bad" : "warn",
    detail
  };
}

function indexesSection(indexes: IndexStatus[], graphVersion: number): HTMLElement {
  const section = sectionShell("indexes", "Indexes");
  const table = document.createElement("table");
  table.append(tableHead(["Kind", "State", "Index version", "Graph version", "Lag"]), tableBody(indexes, graphVersion));
  section.append(table);

  return section;
}

function tableHead(labels: string[]): HTMLTableSectionElement {
  const head = document.createElement("thead");
  const row = document.createElement("tr");
  for (const label of labels) {
    row.append(textElement("th", undefined, label));
  }
  head.append(row);

  return head;
}

function tableBody(indexes: IndexStatus[], graphVersion: number): HTMLTableSectionElement {
  const body = document.createElement("tbody");
  for (const index of indexes) {
    const row = document.createElement("tr");
    row.append(
      textElement("td", undefined, index.kind),
      tableState(index.state),
      textElement("td", undefined, String(index.index_version)),
      textElement("td", undefined, String(index.indexed_graph_version)),
      textElement("td", undefined, String(Math.max(0, graphVersion - index.indexed_graph_version)))
    );
    body.append(row);
  }

  return body;
}

function tableState(state: IndexStatus["state"]): HTMLTableCellElement {
  const cell = document.createElement("td");
  const tone = state === "fresh" ? "good" : state === "stale" || state === "paused" ? "warn" : "bad";
  cell.append(statusPill(state, tone));

  return cell;
}

function runtimeSection(
  status: ProjectStatusResponse,
  service: ServiceStatusResponse | null
): HTMLElement {
  const section = sectionShell("runtime", "Runtime");
  const list = element("dl", "runtime-list");
  list.append(
    runtimeItem("HTTP bind", status.runtime.http_bind),
    runtimeItem("Data", status.runtime.data_dir),
    runtimeItem("State", status.runtime.state_dir),
    runtimeItem("Cache", status.runtime.cache_dir),
    runtimeItem("Logs", status.runtime.log_dir),
    runtimeItem("QoS connections", String(status.runtime.qos_max_connections)),
    runtimeItem("In-flight", String(status.runtime.qos_max_in_flight_requests)),
    runtimeItem("Queue depth", String(status.runtime.qos_max_queue_depth))
  );
  if (service) {
    list.append(
      runtimeItem("Service mode", service.mode),
      runtimeItem("Definition", service.service_definition_path),
      runtimeItem("Worker families", String(service.workers.length)),
      runtimeItem("Proposal backlog", String(service.proposal_backlog)),
      runtimeItem("Audit events", String(service.audit_sink.event_count))
    );
  }
  section.append(list);

  return section;
}

function runtimeItem(label: string, value: string): HTMLElement {
  const item = document.createElement("div");
  item.append(textElement("dt", undefined, label), textElement("dd", undefined, value));

  return item;
}

initializeTheme();
window.addEventListener("popstate", syncActivePageFromLocation);
window.addEventListener("hashchange", syncActivePageFromLocation);
window.addEventListener("relay-knowledge:graph-rerender", rerenderFromState);
void renderApp();
