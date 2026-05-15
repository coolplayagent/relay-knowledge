import type { HealthResponse, ProjectStatusResponse } from "./api/contracts";
import { executeWebOperation } from "./api/client.js";
import { appState, retrieveOperationSnapshot } from "./operations.js";
import { element, icon, textElement } from "./ui.js";

export type HomeQueryCallbacks = {
  rerender: () => void;
  errorMessage: (error: unknown) => string;
};

type HomeQueryRun =
  | { state: "idle" }
  | { state: "running"; query: string }
  | { state: "success"; query: string; result: unknown }
  | { state: "error"; query: string; message: string };

let homeQueryText = "";
let activeHomeQueryRunId = 0;
let homeQueryRun: HomeQueryRun = { state: "idle" };

export function homeQueryEntry(
  status: ProjectStatusResponse,
  health: HealthResponse,
  callbacks: HomeQueryCallbacks
): HTMLElement {
  const entry = element("div", "home-query-entry");
  const form = element("form", "home-query-form");
  form.setAttribute("aria-label", "Query knowledge graph");

  const field = element("label", "home-query-field");
  field.append(textElement("span", undefined, "Query"));
  const input = document.createElement("input");
  input.name = "home-query";
  input.autocomplete = "off";
  input.placeholder = "Ask the graph";
  input.value = homeQueryText;

  const run = document.createElement("button");
  run.type = "submit";
  run.className = "button primary";
  run.dataset.testid = "home-query-run";
  run.disabled = !canRunHomeQuery();
  run.append(icon("run-icon"), document.createTextNode("Query"));

  input.addEventListener("input", () => {
    homeQueryText = input.value;
    run.disabled = !canRunHomeQuery();
  });
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void runHomeQuery(status, health, callbacks);
  });

  field.append(input);
  form.append(field, run);
  entry.append(form);
  const result = homeQueryResult();
  if (result) {
    entry.append(result);
  }

  return entry;
}

function canRunHomeQuery(): boolean {
  return homeQueryRun.state !== "running" && homeQueryText.trim().length > 0;
}

async function runHomeQuery(
  status: ProjectStatusResponse,
  health: HealthResponse,
  callbacks: HomeQueryCallbacks
) {
  const query = homeQueryText.trim();
  if (query.length === 0) {
    homeQueryRun = { state: "error", query, message: "Enter a query." };
    callbacks.rerender();
    return;
  }

  const runId = activeHomeQueryRunId + 1;
  activeHomeQueryRunId = runId;
  appState.retrieve.query = query;
  const retrieve = { ...appState.retrieve, query };
  const snapshot = retrieveOperationSnapshot(status, health, retrieve);
  homeQueryRun = { state: "running", query };
  callbacks.rerender();

  try {
    const response = await executeWebOperation(snapshot);
    if (runId !== activeHomeQueryRunId) {
      return;
    }
    homeQueryRun = { state: "success", query, result: response };
  } catch (error) {
    if (runId !== activeHomeQueryRunId) {
      return;
    }
    homeQueryRun = { state: "error", query, message: callbacks.errorMessage(error) };
  }
  callbacks.rerender();
}

function homeQueryResult(): HTMLElement | null {
  if (homeQueryRun.state === "idle") {
    return null;
  }

  const panel = element("div", "home-query-result");
  panel.dataset.state = homeQueryRun.state;
  if (homeQueryRun.state === "running") {
    panel.append(
      textElement("div", "result-heading", "Retrieve context"),
      textElement("div", "muted-line", `Running "${homeQueryRun.query}"`)
    );
  } else if (homeQueryRun.state === "success") {
    panel.append(
      textElement("div", "result-heading", "Retrieve context"),
      resultPreview(JSON.stringify(homeQueryRun.result, null, 2))
    );
  } else {
    panel.append(
      textElement("div", "result-heading", "Retrieve context"),
      textElement("div", "error-message", homeQueryRun.message)
    );
  }

  return panel;
}

function resultPreview(value: string): HTMLElement {
  const group = element("div", "pre-group");
  group.append(textElement("div", "pre-label", "Result"));
  const pre = document.createElement("pre");
  pre.className = "home-query-preview";
  pre.textContent = value;
  group.append(pre);

  return group;
}
