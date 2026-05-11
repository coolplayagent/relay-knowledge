import type { HealthResponse, IndexStatus, ProjectStatusResponse } from "./api/contracts";
import { loadHealth, loadProjectStatus } from "./api/client.js";

async function renderApp() {
  const root = document.getElementById("root");
  if (!root) {
    return;
  }

  try {
    const [status, health] = await Promise.all([loadProjectStatus(), loadHealth()]);
    root.replaceChildren(shell(status, health));
  } catch (error) {
    root.replaceChildren(errorShell(error));
  }
}

function shell(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const container = element("div", "shell");
  container.append(sidebar(), content(status, health));

  return container;
}

function sidebar(): HTMLElement {
  const aside = element("aside", "sidebar");
  aside.setAttribute("aria-label", "Navigation");
  aside.append(
    textElement("div", "brand", "relay-knowledge"),
    navLink("Status", "#status"),
    navLink("Indexes", "#indexes"),
    navLink("Runtime", "#runtime")
  );

  return aside;
}

function navLink(label: string, href: string): HTMLAnchorElement {
  const link = document.createElement("a");
  link.href = href;
  link.textContent = label;

  return link;
}

function content(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const main = element("main", "content");
  main.append(
    toolbar(status),
    statusSection(health),
    indexesSection(health.indexes),
    runtimeSection(status)
  );

  return main;
}

function errorShell(error: unknown): HTMLElement {
  const container = element("div", "shell");
  const main = element("main", "content");
  const section = sectionShell("status", "Status");
  section.append(textElement("div", "error-message", errorMessage(error)));
  main.append(section);
  container.append(sidebar(), main);

  return container;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Diagnostics unavailable";
}

function toolbar(status: ProjectStatusResponse): HTMLElement {
  const bar = element("div", "toolbar");
  const titles = element("div");
  titles.append(
    textElement("div", "title", status.project_name),
    textElement("div", "subtitle", `Graph version ${status.metadata.graph_version}`)
  );

  const button = document.createElement("button");
  button.type = "button";
  button.setAttribute("aria-label", "Refresh diagnostics");
  button.append(icon("refresh-icon"), document.createTextNode("Refresh"));
  button.addEventListener("click", () => void renderApp());
  bar.append(titles, button);

  return bar;
}

function statusSection(health: HealthResponse): HTMLElement {
  const section = sectionShell("status", "Status");
  const line = element("div", "status-line");
  line.append(
    icon("health-icon"),
    textElement("span", undefined, health.healthy ? "healthy" : "degraded"),
    textElement("span", undefined, `entities ${health.graph.entity_count}`),
    textElement("span", undefined, `evidence ${health.graph.evidence_count}`)
  );
  section.append(line);

  return section;
}

function indexesSection(indexes: IndexStatus[]): HTMLElement {
  const section = sectionShell("indexes", "Indexes");
  const table = document.createElement("table");
  table.append(tableHead(), tableBody(indexes));
  section.append(table);

  return section;
}

function tableHead(): HTMLTableSectionElement {
  const head = document.createElement("thead");
  const row = document.createElement("tr");
  for (const label of ["Kind", "State", "Index version", "Graph version"]) {
    row.append(textElement("th", undefined, label));
  }
  head.append(row);

  return head;
}

function tableBody(indexes: IndexStatus[]): HTMLTableSectionElement {
  const body = document.createElement("tbody");
  for (const index of indexes) {
    const row = document.createElement("tr");
    row.append(
      textElement("td", undefined, index.kind),
      textElement("td", undefined, index.state),
      textElement("td", undefined, String(index.index_version)),
      textElement("td", undefined, String(index.indexed_graph_version))
    );
    body.append(row);
  }

  return body;
}

function runtimeSection(status: ProjectStatusResponse): HTMLElement {
  const section = sectionShell("runtime", "Runtime");
  const list = element("dl", "runtime-list");
  list.append(
    runtimeItem("HTTP bind", status.runtime.http_bind),
    runtimeItem("Data", status.runtime.data_dir),
    runtimeItem("Cache", status.runtime.cache_dir),
    runtimeItem("QoS connections", String(status.runtime.qos_max_connections))
  );
  section.append(list);

  return section;
}

function runtimeItem(label: string, value: string): HTMLElement {
  const item = document.createElement("div");
  item.append(textElement("dt", undefined, label), textElement("dd", undefined, value));

  return item;
}

function sectionShell(id: string, title: string): HTMLElement {
  const section = element("section", "section");
  section.id = id;
  section.append(textElement("div", "section-title", title));

  return section;
}

function icon(className: string): HTMLSpanElement {
  const span = element("span", className);
  span.setAttribute("aria-hidden", "true");

  return span;
}

function textElement<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string | undefined,
  text: string
): HTMLElementTagNameMap[K] {
  const node = element(tag, className);
  node.textContent = text;

  return node;
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }

  return node;
}

void renderApp();
