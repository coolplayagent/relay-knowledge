import type {
  HealthResponse,
  IndexStatus,
  ProjectStatusResponse,
  ServiceStatusResponse
} from "./api/contracts";
import {
  executeWebOperation,
  loadHealth,
  loadProjectStatus,
  loadServiceStatus
} from "./api/client.js";
import { providersSection } from "./providers.js";
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
  type Freshness,
  type ProposalAction,
  type WorkerKind
} from "./operations.js";

type Diagnostics = {
  status: ProjectStatusResponse;
  health: HealthResponse;
  service: ServiceStatusResponse | null;
};

type Tone = "good" | "warn" | "bad";

let currentDiagnostics: Diagnostics | null = null;
let activeOperationRunId = 0;
let operationRun:
  | { state: "idle" }
  | { state: "running"; snapshotName: string }
  | { state: "success"; snapshotName: string; result: unknown; diagnosticsError?: string }
  | { state: "error"; snapshotName: string; message: string } = { state: "idle" };

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
  container.append(sidebar(), main);

  return container;
}

function shell(
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null
): HTMLElement {
  const container = element("div", "shell");
  container.append(sidebar(), content(status, health, service));

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
    navLink("Providers", "#providers"),
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

function content(
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null
): HTMLElement {
  const main = element("main", "content");
  main.append(
    toolbar(status, health),
    statusSection(status, health),
    readinessSection(status, health, service),
    providersSection(status, health),
    operationsSection(status, health),
    indexesSection(health.indexes, health.metadata.graph_version),
    runtimeSection(status, service)
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
      degraded_file_count: health.graph.code_parse_status_counts.failed
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
    case "provider":
      form.append(
        inputControl("Probe input", appState.provider.probeInput, (value) => {
          appState.provider.probeInput = value;
        })
      );
      break;
    case "worker":
      form.append(workerControls());
      break;
    case "proposal":
      form.append(proposalControls());
      break;
    case "audit":
      form.append(
        inputControl("Operation", appState.audit.operation, (value) => {
          appState.audit.operation = value;
        }),
        numberControl("Limit", appState.audit.limit, (value) => {
          appState.audit.limit = positiveInt(value, 50);
        })
      );
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

function workerControls(): HTMLElement {
  const group = element("div", "field-grid");
  group.append(
    selectControl(
      "Action",
      appState.worker.action,
      [
        ["status", "status"],
        ["run-once", "run-once"]
      ],
      (value) => {
        appState.worker.action = value as AppState["worker"]["action"];
        updatePreview();
      }
    ),
    selectControl(
      "Kind",
      appState.worker.kind,
      [
        ["embedding", "embedding"],
        ["ocr", "ocr"],
        ["vision", "vision"],
        ["extractor", "extractor"]
      ],
      (value) => {
        appState.worker.kind = value as WorkerKind;
        updatePreview();
      }
    )
  );

  return group;
}

function proposalControls(): HTMLElement {
  const group = element("div", "field-grid");
  group.append(
    selectControl(
      "Action",
      appState.proposal.action,
      [
        ["list", "list"],
        ["show", "show"],
        ["accept", "accept"],
        ["reject", "reject"],
        ["supersede", "supersede"]
      ],
      (value) => {
        appState.proposal.action = value as ProposalAction;
        rerenderFromState();
      }
    )
  );
  if (appState.proposal.action === "list") {
    group.append(
      selectControl(
        "State",
        appState.proposal.state,
        [
          ["proposed", "proposed"],
          ["accepted", "accepted"],
          ["rejected", "rejected"],
          ["superseded", "superseded"]
        ],
        (value) => {
          appState.proposal.state = value as AppState["proposal"]["state"];
          updatePreview();
        }
      ),
      numberControl("Limit", appState.proposal.limit, (value) => {
        appState.proposal.limit = positiveInt(value, 25);
      })
    );
  } else {
    group.append(
      inputControl("Proposal", appState.proposal.proposalId, (value) => {
        appState.proposal.proposalId = value;
      })
    );
    if (appState.proposal.action !== "show") {
      group.append(
        inputControl("Actor", appState.proposal.actor, (value) => {
          appState.proposal.actor = value;
        }),
        inputControl("Reason", appState.proposal.reason, (value) => {
          appState.proposal.reason = value;
        })
      );
    }
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
    previewActions(status, health),
    operationResultPanel()
  );

  return preview;
}

function previewActions(status: ProjectStatusResponse, health: HealthResponse): HTMLElement {
  const actions = element("div", "preview-actions");
  const snapshot = currentOperationSnapshot(status, health);
  const runnable = isExecutableWebOperation(snapshot.payload.operation);
  const run = document.createElement("button");
  run.type = "button";
  run.className = "button primary";
  run.dataset.testid = "run-operation";
  run.disabled = operationRun.state === "running" || !runnable;
  run.append(icon("run-icon"), document.createTextNode("Run"));
  run.addEventListener("click", () => {
    if (runnable) {
      void runCurrentOperation(status, health);
    }
  });

  const stage = document.createElement("button");
  stage.type = "button";
  stage.className = "button";
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
    operationRun = { state: "idle" };
    rerenderFromState();
  });
  actions.append(run, stage, clear);

  return actions;
}

async function runCurrentOperation(status: ProjectStatusResponse, health: HealthResponse) {
  const runId = activeOperationRunId + 1;
  activeOperationRunId = runId;
  const snapshot = currentOperationSnapshot(status, health);
  operationRun = { state: "running", snapshotName: snapshot.name };
  rerenderFromState();

  try {
    const response = await executeWebOperation(snapshot);
    if (runId !== activeOperationRunId) {
      return;
    }
    operationRun = { state: "success", snapshotName: snapshot.name, result: response };
    rerenderFromState();
    await refreshDiagnosticsAfterOperation(response, snapshot.name, runId);
  } catch (error) {
    if (runId !== activeOperationRunId) {
      return;
    }
    operationRun = {
      state: "error",
      snapshotName: snapshot.name,
      message: errorMessage(error)
    };
  }
  rerenderFromState();
}

async function refreshDiagnosticsAfterOperation(result: unknown, snapshotName: string, runId: number) {
  try {
    const [nextStatus, nextHealth, nextService] = await Promise.all([
      loadProjectStatus(),
      loadHealth(),
      loadServiceStatus().catch(() => null)
    ]);
    if (runId !== activeOperationRunId) {
      return;
    }
    currentDiagnostics = { status: nextStatus, health: nextHealth, service: nextService };
  } catch (error) {
    if (runId !== activeOperationRunId) {
      return;
    }
    operationRun = {
      state: "success",
      snapshotName,
      result,
      diagnosticsError: errorMessage(error)
    };
  }
}

function operationResultPanel(): HTMLElement {
  const panel = element("div", "operation-result");
  panel.dataset.state = operationRun.state;
  if (operationRun.state === "idle") {
    panel.append(textElement("div", "muted-line", "No operation has run in this session."));
  } else if (operationRun.state === "running") {
    panel.append(
      textElement("div", "result-heading", operationRun.snapshotName),
      textElement("div", "muted-line", "Running")
    );
  } else if (operationRun.state === "success") {
    panel.append(
      textElement("div", "result-heading", operationRun.snapshotName),
      preBlock("Result", JSON.stringify(operationRun.result, null, 2), "result-preview")
    );
    if (operationRun.diagnosticsError) {
      panel.append(textElement("div", "warning-message", operationRun.diagnosticsError));
    }
  } else {
    panel.append(
      textElement("div", "result-heading", operationRun.snapshotName),
      textElement("div", "error-message", operationRun.message)
    );
  }

  return panel;
}

function isExecutableWebOperation(operation: unknown): boolean {
  return (
    operation === "retrieve.context" ||
    operation === "graph.ingest" ||
    operation === "graph.inspect" ||
    operation === "index.refresh" ||
    operation === "provider.embedding.probe" ||
    operation === "worker.status" ||
    operation === "worker.run-once" ||
    operation === "proposal.list" ||
    operation === "proposal.show" ||
    operation === "proposal.accept" ||
    operation === "proposal.reject" ||
    operation === "proposal.supersede" ||
    operation === "audit.query" ||
    operation === "service.doctor" ||
    operation === "service.run.streamable_http" ||
    (typeof operation === "string" && operation.startsWith("code.repo."))
  );
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
