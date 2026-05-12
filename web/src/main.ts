import type { HealthResponse, IndexStatus, ProjectStatusResponse } from "./api/contracts";
import { loadHealth, loadProjectStatus } from "./api/client.js";
import {
  INDEX_KINDS,
  OPERATIONS,
  appState,
  codeActionOptions,
  codeQueryKindOptions,
  currentOperationSnapshot,
  freshnessOptions,
  maxIndexLag,
  positiveInt,
  uniqueKinds,
  type AppState,
  type CodeAction,
  type CodeQueryKind,
  type Freshness
} from "./operations.js";

type Diagnostics = {
  status: ProjectStatusResponse;
  health: HealthResponse;
};

type Tone = "good" | "warn" | "bad";

let currentDiagnostics: Diagnostics | null = null;

async function renderApp() {
  const root = document.getElementById("root");
  if (!root) {
    return;
  }

  root.replaceChildren(loadingShell());
  try {
    const [status, health] = await Promise.all([loadProjectStatus(), loadHealth()]);
    currentDiagnostics = { status, health };
    root.replaceChildren(shell(status, health));
  } catch (error) {
    root.replaceChildren(errorShell(error));
  }
}

function rerenderFromState() {
  const root = document.getElementById("root");
  if (!root || !currentDiagnostics) {
    return;
  }

  root.replaceChildren(shell(currentDiagnostics.status, currentDiagnostics.health));
}

function loadingShell(): HTMLElement {
  const container = element("div", "shell");
  const main = element("main", "content");
  main.append(sectionShell("status", "Status", textElement("div", "muted-line", "Loading")));
  container.append(sidebar(), main);

  return container;
}

function shell(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const container = element("div", "shell");
  container.append(sidebar(), content(status, health));

  return container;
}

function sidebar(): HTMLElement {
  const aside = element("aside", "sidebar");
  aside.setAttribute("aria-label", "Navigation");
  const nav = element("nav", "nav-list");
  nav.setAttribute("aria-label", "Primary");
  nav.append(
    navLink("Status", "#status"),
    navLink("Readiness", "#readiness"),
    navLink("Operations", "#operations"),
    navLink("Indexes", "#indexes"),
    navLink("Runtime", "#runtime")
  );
  aside.append(textElement("div", "brand", "relay-knowledge"), nav);

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
    toolbar(status, health),
    statusSection(status, health),
    readinessSection(status, health),
    operationsSection(status, health),
    indexesSection(health.indexes, health.metadata.graph_version),
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
    refreshButton()
  );
  bar.append(titles, actions);

  return bar;
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

function statusPill(text: string, tone: "good" | "warn" | "bad"): HTMLElement {
  return textElement("span", `status-pill ${tone}`, text);
}

function statusSection(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const lag = maxIndexLag(health.indexes, status.metadata.graph_version);
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
    metricItem("Code files", health.graph.code_file_count),
    metricItem("Symbols", health.graph.code_symbol_count),
    metricItem("References", health.graph.code_reference_count)
  );
  section.append(statusLine, metrics);

  return section;
}

function metricItem(label: string, value: number): HTMLElement {
  const item = element("div", "metric-item");
  item.append(textElement("dt", undefined, label), textElement("dd", undefined, String(value)));

  return item;
}

function readinessSection(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const section = sectionShell("readiness", "GraphRAG readiness");
  const grid = element("div", "readiness-grid");
  const graph = health.graph;
  const graphVersion = health.metadata.graph_version;
  const bm25 = health.indexes.find((index) => index.kind === "bm25");
  const semantic = health.indexes.find((index) => index.kind === "semantic");
  const vector = health.indexes.find((index) => index.kind === "vector");
  const hasEvidence = graph.entity_count > 0 || graph.evidence_count > 0;
  const hasCodeGraph = graph.code_file_count > 0 || graph.code_symbol_count > 0;

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
      `${graph.code_file_count} files / ${graph.code_symbol_count} symbols`
    ),
    readinessItem(
      "Runtime budgets",
      health.healthy ? "ready" : "degraded",
      health.healthy ? "good" : "warn",
      `${status.runtime.qos_max_in_flight_requests} in-flight / ${status.runtime.qos_max_queue_depth} queue`
    ),
    readinessItem(
      "Refresh recovery",
      health.index_refresh.dead_letter_count > 0 ? "failed" : "ready",
      health.index_refresh.dead_letter_count > 0 ? "bad" : "good",
      `${health.index_refresh.queue_depth} queued / ${health.index_refresh.dead_letter_count} dead-letter`
    )
  );
  section.append(grid);

  return section;
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

function operationsSection(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const section = sectionShell("operations", "Operations");
  const tabs = element("div", "operation-tabs");
  tabs.setAttribute("role", "tablist");
  for (const operation of OPERATIONS) {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = operation.id === appState.selectedOperation ? "tab active" : "tab";
    tab.setAttribute("role", "tab");
    tab.setAttribute("aria-selected", String(operation.id === appState.selectedOperation));
    tab.textContent = operation.label;
    tab.addEventListener("click", () => {
      appState.selectedOperation = operation.id;
      rerenderFromState();
    });
    tabs.append(tab);
  }

  const body = element("div", "operation-layout");
  body.append(operationForm(), operationPreview(status, health), stagedOperations());
  section.append(tabs, body);

  return section;
}

function operationForm(): HTMLElement {
  const form = element("form", "operation-form");
  form.addEventListener("submit", (event) => event.preventDefault());

  switch (appState.selectedOperation) {
    case "retrieve":
      form.append(
        inputControl("Query", appState.retrieve.query, (value) => {
          appState.retrieve.query = value;
        }),
        inputControl("Scope", appState.retrieve.sourceScope, (value) => {
          appState.retrieve.sourceScope = value;
        }),
        selectControl("Freshness", appState.retrieve.freshness, freshnessOptions(), (value) => {
          appState.retrieve.freshness = value as Freshness;
        }),
        numberControl("Limit", appState.retrieve.limit, (value) => {
          appState.retrieve.limit = positiveInt(value, 8);
        })
      );
      break;
    case "ingest":
      form.append(
        inputControl("Source", appState.ingest.sourceScope, (value) => {
          appState.ingest.sourceScope = value;
        }),
        textareaControl("Content", appState.ingest.content, (value) => {
          appState.ingest.content = value;
        }),
        inputControl("Entities", appState.ingest.entityLabels, (value) => {
          appState.ingest.entityLabels = value;
        })
      );
      break;
    case "graph":
      form.append(
        inputControl("Scope", appState.graph.sourceScope, (value) => {
          appState.graph.sourceScope = value;
        })
      );
      break;
    case "code":
      form.append(codeActionControls());
      break;
    case "indexes":
      form.append(indexKindControls());
      break;
    case "service":
      form.append(
        selectControl(
          "MCP",
          appState.service.mcpTransport,
          [
            ["streamable-http", "streamable-http"],
            ["configured", "configured"]
          ],
          (value) => {
            appState.service.mcpTransport = value as AppState["service"]["mcpTransport"];
          }
        ),
        inputControl("Allowed scopes", appState.service.allowedScopes, (value) => {
          appState.service.allowedScopes = value;
        })
      );
      break;
  }

  return form;
}

function codeActionControls(): HTMLElement {
  const group = element("div", "field-grid");
  group.append(
    selectControl("Action", appState.code.action, codeActionOptions(), (value) => {
      appState.code.action = value as CodeAction;
      rerenderFromState();
    }),
    inputControl("Alias", appState.code.alias, (value) => {
      appState.code.alias = value;
    })
  );

  if (appState.code.action === "register") {
    group.append(
      inputControl("Root path", appState.code.rootPath, (value) => {
        appState.code.rootPath = value;
      }),
      inputControl("Path filter", appState.code.pathFilter, (value) => {
        appState.code.pathFilter = value;
      }),
      inputControl("Language", appState.code.languageFilter, (value) => {
        appState.code.languageFilter = value;
      })
    );
  } else if (appState.code.action === "index") {
    group.append(
      inputControl("Ref", appState.code.refSelector, (value) => {
        appState.code.refSelector = value;
      })
    );
  } else if (appState.code.action === "update") {
    group.append(
      inputControl("Base", appState.code.baseRef, (value) => {
        appState.code.baseRef = value;
      }),
      inputControl("Head", appState.code.headRef, (value) => {
        appState.code.headRef = value;
      })
    );
  } else if (appState.code.action === "impact") {
    group.append(
      inputControl("Base", appState.code.baseRef, (value) => {
        appState.code.baseRef = value;
      }),
      inputControl("Head", appState.code.headRef, (value) => {
        appState.code.headRef = value;
      }),
      numberControl("Limit", appState.code.limit, (value) => {
        appState.code.limit = positiveInt(value, 10);
      })
    );
  } else if (appState.code.action === "query") {
    group.append(
      inputControl("Query", appState.code.query, (value) => {
        appState.code.query = value;
      }),
      selectControl("Kind", appState.code.queryKind, codeQueryKindOptions(), (value) => {
        appState.code.queryKind = value as CodeQueryKind;
      }),
      inputControl("Ref", appState.code.refSelector, (value) => {
        appState.code.refSelector = value;
      }),
      inputControl("Path filter", appState.code.pathFilter, (value) => {
        appState.code.pathFilter = value;
      }),
      inputControl("Language", appState.code.languageFilter, (value) => {
        appState.code.languageFilter = value;
      }),
      selectControl("Freshness", appState.code.freshness, freshnessOptions(), (value) => {
        appState.code.freshness = value as Freshness;
      }),
      numberControl("Limit", appState.code.limit, (value) => {
        appState.code.limit = positiveInt(value, 10);
      })
    );
  }

  return group;
}

function indexKindControls(): HTMLElement {
  const group = element("fieldset", "checkbox-group");
  group.append(textElement("legend", undefined, "Kinds"));
  for (const kind of INDEX_KINDS) {
    const label = element("label", "checkbox-row");
    const input = document.createElement("input");
    input.type = "checkbox";
    input.name = `index-${kind}`;
    input.checked = appState.indexes.kinds.includes(kind);
    input.addEventListener("change", () => {
      appState.indexes.kinds = input.checked
        ? uniqueKinds([...appState.indexes.kinds, kind])
        : appState.indexes.kinds.filter((item) => item !== kind);
      updatePreview();
    });
    label.append(input, textElement("span", undefined, kind));
    group.append(label);
  }

  return group;
}

function operationPreview(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const snapshot = currentOperationSnapshot(status, health);
  const preview = element("div", "operation-preview");
  preview.append(
    textElement("div", "panel-title", snapshot.name),
    preBlock("Command", snapshot.command, "command-preview"),
    preBlock("Request", JSON.stringify(snapshot.payload, null, 2), "payload-preview"),
    previewActions(status, health)
  );

  return preview;
}

function previewActions(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const actions = element("div", "preview-actions");
  const stage = document.createElement("button");
  stage.type = "button";
  stage.className = "button primary";
  stage.dataset.testid = "stage-operation";
  stage.append(icon("plus-icon"), document.createTextNode("Stage"));
  stage.addEventListener("click", () => {
    appState.staged = [currentOperationSnapshot(status, health), ...appState.staged].slice(0, 6);
    rerenderFromState();
  });

  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "button";
  clear.append(icon("clear-icon"), document.createTextNode("Clear"));
  clear.addEventListener("click", () => {
    appState.staged = [];
    rerenderFromState();
  });
  actions.append(stage, clear);

  return actions;
}

function stagedOperations(): HTMLElement {
  const panel = element("div", "staged-panel");
  panel.append(textElement("div", "panel-title", "Staged operations"));
  const list = element("ol", "staged-list");
  if (appState.staged.length === 0) {
    list.append(textElement("li", "muted-line", "None"));
  } else {
    for (const item of appState.staged) {
      const row = element("li", "staged-item");
      row.append(
        textElement("span", "staged-name", item.name),
        textElement("code", undefined, item.command),
        textElement("time", undefined, item.createdAt)
      );
      list.append(row);
    }
  }
  panel.append(list);

  return panel;
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

function runtimeSection(status: ProjectStatusResponse): HTMLElement {
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
  section.append(list);

  return section;
}

function runtimeItem(label: string, value: string): HTMLElement {
  const item = document.createElement("div");
  item.append(textElement("dt", undefined, label), textElement("dd", undefined, value));

  return item;
}

function sectionShell(id: string, title: string, child?: HTMLElement): HTMLElement {
  const section = element("section", "section");
  section.id = id;
  section.append(textElement("h2", "section-title", title));
  if (child) {
    section.append(child);
  }

  return section;
}

function inputControl(
  label: string,
  value: string,
  onInput: (value: string) => void
): HTMLElement {
  const control = fieldShell(label);
  const input = document.createElement("input");
  input.name = fieldName(label);
  input.value = value;
  input.addEventListener("input", () => {
    onInput(input.value);
    updatePreview();
  });
  control.append(input);

  return control;
}

function numberControl(
  label: string,
  value: number,
  onInput: (value: string) => void
): HTMLElement {
  const control = fieldShell(label);
  const input = document.createElement("input");
  input.type = "number";
  input.min = "1";
  input.name = fieldName(label);
  input.value = String(value);
  input.addEventListener("input", () => {
    onInput(input.value);
    updatePreview();
  });
  control.append(input);

  return control;
}

function textareaControl(
  label: string,
  value: string,
  onInput: (value: string) => void
): HTMLElement {
  const control = fieldShell(label);
  const input = document.createElement("textarea");
  input.rows = 4;
  input.name = fieldName(label);
  input.value = value;
  input.addEventListener("input", () => {
    onInput(input.value);
    updatePreview();
  });
  control.append(input);

  return control;
}

function selectControl(
  label: string,
  value: string,
  options: Array<[string, string]>,
  onChange: (value: string) => void
): HTMLElement {
  const control = fieldShell(label);
  const select = document.createElement("select");
  select.name = fieldName(label);
  for (const [optionValue, optionLabel] of options) {
    const option = document.createElement("option");
    option.value = optionValue;
    option.textContent = optionLabel;
    option.selected = optionValue === value;
    select.append(option);
  }
  select.addEventListener("change", () => {
    onChange(select.value);
    updatePreview();
  });
  control.append(select);

  return control;
}

function fieldShell(label: string): HTMLElement {
  const control = element("label", "field");
  control.append(textElement("span", undefined, label));

  return control;
}

function fieldName(label: string): string {
  return label.toLowerCase().replaceAll(" ", "-");
}

function preBlock(label: string, value: string, className: string): HTMLElement {
  const group = element("div", "pre-group");
  group.append(textElement("div", "pre-label", label));
  const pre = document.createElement("pre");
  pre.className = className;
  pre.textContent = value;
  group.append(pre);

  return group;
}

function updatePreview() {
  if (!currentDiagnostics) {
    return;
  }
  const snapshot = currentOperationSnapshot(currentDiagnostics.status, currentDiagnostics.health);
  const command = document.querySelector(".command-preview");
  const payload = document.querySelector(".payload-preview");
  if (command) {
    command.textContent = snapshot.command;
  }
  if (payload) {
    payload.textContent = JSON.stringify(snapshot.payload, null, 2);
  }
}

function icon(className: string): HTMLSpanElement {
  const span = element("span", `icon ${className}`);
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
