import type { HealthResponse, ProjectStatusResponse, ServiceStatusResponse } from "./api/contracts";
import {
  executeWebOperation,
  loadHealth,
  loadProjectStatus,
  loadServiceStatus
} from "./api/client.js";
import {
  INDEX_KINDS,
  OPERATIONS,
  appState,
  codeActionOptions,
  codeQueryKindOptions,
  currentOperationSnapshot,
  freshnessOptions,
  positiveInt,
  uniqueKinds,
  type AppState,
  type CodeAction,
  type CodeQueryKind,
  type Freshness,
  type ProposalAction,
  type WorkerKind
} from "./operations.js";
import { element, icon, sectionShell, textElement } from "./ui.js";

type DiagnosticsSnapshot = {
  status: ProjectStatusResponse;
  health: HealthResponse;
  service: ServiceStatusResponse | null;
};

type OperationsCallbacks = {
  rerender: () => void;
  setDiagnostics: (diagnostics: DiagnosticsSnapshot) => void;
  errorMessage: (error: unknown) => string;
};

let activeOperationRunId = 0;
let currentOperationDiagnostics: Pick<DiagnosticsSnapshot, "status" | "health"> | null = null;
let operationRun:
  | { state: "idle" }
  | { state: "running"; snapshotName: string }
  | { state: "success"; snapshotName: string; result: unknown; diagnosticsError?: string }
  | { state: "error"; snapshotName: string; message: string } = { state: "idle" };

export function operationsSection(
  status: ProjectStatusResponse,
  health: HealthResponse,
  callbacks: OperationsCallbacks
): HTMLElement {
  currentOperationDiagnostics = { status, health };
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
      callbacks.rerender();
    });
    tabs.append(tab);
  }

  const body = element("div", "operation-layout");
  body.append(operationForm(callbacks), operationPreview(status, health, callbacks), stagedOperations());
  section.append(tabs, body);

  return section;
}

function operationForm(callbacks: OperationsCallbacks): HTMLElement {
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
      form.append(codeActionControls(callbacks));
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
      form.append(proposalControls(callbacks));
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

function codeActionControls(callbacks: OperationsCallbacks): HTMLElement {
  const group = element("div", "field-grid");
  group.append(
    selectControl("Action", appState.code.action, codeActionOptions(), (value) => {
      appState.code.action = value as CodeAction;
      callbacks.rerender();
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

function proposalControls(callbacks: OperationsCallbacks): HTMLElement {
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
        callbacks.rerender();
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

function operationPreview(
  status: ProjectStatusResponse,
  health: HealthResponse,
  callbacks: OperationsCallbacks
): HTMLElement {
  const snapshot = currentOperationSnapshot(status, health);
  const preview = element("div", "operation-preview");
  preview.append(
    textElement("div", "panel-title", snapshot.name),
    preBlock("Command", snapshot.command, "command-preview"),
    preBlock("Request", JSON.stringify(snapshot.payload, null, 2), "payload-preview"),
    previewActions(status, health, callbacks),
    operationResultPanel()
  );

  return preview;
}

function previewActions(
  status: ProjectStatusResponse,
  health: HealthResponse,
  callbacks: OperationsCallbacks
): HTMLElement {
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
      void runCurrentOperation(status, health, callbacks);
    }
  });

  const stage = document.createElement("button");
  stage.type = "button";
  stage.className = "button";
  stage.dataset.testid = "stage-operation";
  stage.append(icon("plus-icon"), document.createTextNode("Stage"));
  stage.addEventListener("click", () => {
    appState.staged = [currentOperationSnapshot(status, health), ...appState.staged].slice(0, 6);
    callbacks.rerender();
  });

  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "button";
  clear.append(icon("clear-icon"), document.createTextNode("Clear"));
  clear.addEventListener("click", () => {
    appState.staged = [];
    operationRun = { state: "idle" };
    callbacks.rerender();
  });
  actions.append(run, stage, clear);

  return actions;
}

async function runCurrentOperation(
  status: ProjectStatusResponse,
  health: HealthResponse,
  callbacks: OperationsCallbacks
) {
  const runId = activeOperationRunId + 1;
  activeOperationRunId = runId;
  const snapshot = currentOperationSnapshot(status, health);
  operationRun = { state: "running", snapshotName: snapshot.name };
  callbacks.rerender();

  try {
    const response = await executeWebOperation(snapshot);
    if (runId !== activeOperationRunId) {
      return;
    }
    operationRun = { state: "success", snapshotName: snapshot.name, result: response };
    callbacks.rerender();
    await refreshDiagnosticsAfterOperation(response, snapshot.name, runId, callbacks);
  } catch (error) {
    if (runId !== activeOperationRunId) {
      return;
    }
    operationRun = {
      state: "error",
      snapshotName: snapshot.name,
      message: callbacks.errorMessage(error)
    };
  }
  callbacks.rerender();
}

async function refreshDiagnosticsAfterOperation(
  result: unknown,
  snapshotName: string,
  runId: number,
  callbacks: OperationsCallbacks
) {
  try {
    const [status, health, service] = await Promise.all([
      loadProjectStatus(),
      loadHealth(),
      loadServiceStatus().catch(() => null)
    ]);
    if (runId !== activeOperationRunId) {
      return;
    }
    callbacks.setDiagnostics({ status, health, service });
  } catch (error) {
    if (runId !== activeOperationRunId) {
      return;
    }
    operationRun = {
      state: "success",
      snapshotName,
      result,
      diagnosticsError: callbacks.errorMessage(error)
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
  if (!currentOperationDiagnostics) {
    return;
  }
  const snapshot = currentOperationSnapshot(
    currentOperationDiagnostics.status,
    currentOperationDiagnostics.health
  );
  const command = document.querySelector(".command-preview");
  const payload = document.querySelector(".payload-preview");
  if (command) {
    command.textContent = snapshot.command;
  }
  if (payload) {
    payload.textContent = JSON.stringify(snapshot.payload, null, 2);
  }
}
