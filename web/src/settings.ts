import type {
  HealthResponse,
  ProjectStatusResponse,
  ServiceStatusResponse,
  WebOperationSnapshot
} from "./api/contracts";
import {
  executeWebOperation,
  loadHealth,
  loadProjectStatus,
  loadServiceStatus
} from "./api/client.js";
import { modelProviderSettingsPanel } from "./model_provider_settings.js";
import { element, icon, sectionShell, statusPill, textElement, type Tone } from "./ui.js";

type BackendMode = "local" | "external" | "disabled";
type ProviderKind = "openai_compatible" | "echo";

type SettingsState = {
  agent: {
    mcpEnabled: boolean;
    endpoint: string;
    allowedOrigins: string;
    allowedScopes: string;
    allowUnspecifiedScope: boolean;
    allowRemoteClients: boolean;
    maxLimit: number;
    maxContextBytes: number;
    maxRuntimeMs: number;
    auditSinkEnabled: boolean;
    auditQueueDepth: number;
  };
  model: {
    semanticBackend: BackendMode;
    vectorBackend: BackendMode;
    provider: ProviderKind;
    baseUrl: string;
    apiKey: string;
    runtimeKeyConfigured: boolean;
    textModel: string;
    imageModel: string;
    dimension: number;
    batchSize: number;
    timeoutMs: number;
    maxConcurrency: number;
  };
};

type SettingsCallbacks = {
  rerender: () => void;
  setDiagnostics: (diagnostics: {
    status: ProjectStatusResponse;
    health: HealthResponse;
    service: ServiceStatusResponse | null;
  }) => void;
  errorMessage: (error: unknown) => string;
};

type ProbeState =
  | { state: "idle" }
  | { state: "running" }
  | { state: "success"; result: unknown; diagnosticsError?: string }
  | { state: "error"; message: string };

const BACKEND_OPTIONS: Array<[BackendMode, string]> = [
  ["local", "local"],
  ["external", "external"],
  ["disabled", "disabled"]
];
const PROVIDER_OPTIONS: Array<[ProviderKind, string]> = [
  ["openai_compatible", "openai_compatible"],
  ["echo", "echo"]
];

let settingsState: SettingsState | null = null;
let settingsStateSourceKey = "";
let settingsDirty = false;
let probeState: ProbeState = { state: "idle" };
let copyMessage = "";
let activeProbeRunId = 0;

export function settingsSection(
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null,
  callbacks: SettingsCallbacks
): HTMLElement {
  const sourceKey = settingsSourceKey(status, service);
  if (!settingsState || (!settingsDirty && settingsStateSourceKey !== sourceKey)) {
    settingsState = settingsStateFromRuntime(status, service);
    settingsStateSourceKey = sourceKey;
  }

  const state = settingsState;
  const section = sectionShell("settings", "Settings");
  const layout = element("div", "settings-layout");
  const left = element("div", "settings-stack");
  left.append(
    settingsForm(state, status, health, service, callbacks),
    modelProviderSettingsPanel({
      rerender: callbacks.rerender,
      errorMessage: callbacks.errorMessage
    })
  );
  layout.append(
    left,
    settingsOutput(state, status, health, callbacks)
  );
  section.append(layout);

  return section;
}

function settingsStateFromRuntime(
  status: ProjectStatusResponse,
  service: ServiceStatusResponse | null
): SettingsState {
  const protocols = service?.agent_protocols;
  const policy = protocols?.policy;

  return {
    agent: {
      mcpEnabled: protocols?.mcp_streamable_http_enabled ?? false,
      endpoint: protocols?.mcp_endpoint ?? "/mcp",
      allowedOrigins: protocols?.mcp_allowed_origins?.join(", ") ?? "",
      allowedScopes: service?.operator.allowed_scopes.join(", ") ?? "docs",
      allowUnspecifiedScope: policy?.allow_unspecified_scope ?? false,
      allowRemoteClients: policy?.allow_remote_clients ?? false,
      maxLimit: policy?.max_limit ?? 10,
      maxContextBytes: policy?.max_context_bytes ?? 65536,
      maxRuntimeMs: policy?.max_runtime_ms ?? status.runtime.http_request_timeout_ms,
      auditSinkEnabled: protocols?.audit_sink_enabled ?? service?.audit_sink.durable ?? false,
      auditQueueDepth: protocols?.audit_queue_depth ?? 1024
    },
    model: {
      semanticBackend: status.runtime.semantic_backend_mode,
      vectorBackend: status.runtime.vector_backend_mode,
      provider: status.runtime.embedding_provider ?? "openai_compatible",
      baseUrl: status.runtime.embedding_base_url ?? "https://api.example.com/v1",
      apiKey: "",
      runtimeKeyConfigured: status.runtime.embedding_api_key_configured,
      textModel: status.runtime.text_embedding_model,
      imageModel: status.runtime.image_embedding_model,
      dimension: status.runtime.embedding_dimension,
      batchSize: status.runtime.embedding_batch_size ?? 16,
      timeoutMs: status.runtime.embedding_timeout_ms ?? 10000,
      maxConcurrency: status.runtime.embedding_max_concurrency ?? 2
    }
  };
}

function settingsForm(
  state: SettingsState,
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null,
  callbacks: SettingsCallbacks
): HTMLElement {
  const form = element("form", "settings-panel");
  form.addEventListener("submit", (event) => event.preventDefault());
  form.append(
    textElement("div", "panel-title", "Agent interoperability"),
    agentStatusRow(state, service),
    checkboxControl("MCP Streamable HTTP", state.agent.mcpEnabled, (value) => {
      state.agent.mcpEnabled = value;
    }),
    inputControl("MCP endpoint", state.agent.endpoint, (value) => {
      state.agent.endpoint = value;
    }),
    inputControl("Allowed scopes", state.agent.allowedScopes, (value) => {
      state.agent.allowedScopes = value;
    }),
    inputControl("Allowed origins", state.agent.allowedOrigins, (value) => {
      state.agent.allowedOrigins = value;
    }),
    checkboxControl("Unspecified scope", state.agent.allowUnspecifiedScope, (value) => {
      state.agent.allowUnspecifiedScope = value;
    }),
    checkboxControl("Remote clients", state.agent.allowRemoteClients, (value) => {
      state.agent.allowRemoteClients = value;
    }),
    numberControl("Max tool limit", state.agent.maxLimit, (value) => {
      state.agent.maxLimit = positiveInt(value, 10);
    }),
    numberControl("Max context bytes", state.agent.maxContextBytes, (value) => {
      state.agent.maxContextBytes = positiveInt(value, 65536);
    }),
    numberControl("Request timeout ms", state.agent.maxRuntimeMs, (value) => {
      state.agent.maxRuntimeMs = positiveInt(value, status.runtime.http_request_timeout_ms);
    }),
    checkboxControl("Audit sink", state.agent.auditSinkEnabled, (value) => {
      state.agent.auditSinkEnabled = value;
    }),
    numberControl("Audit queue depth", state.agent.auditQueueDepth, (value) => {
      state.agent.auditQueueDepth = positiveInt(value, 1024);
    }),
    textElement("div", "panel-title", "Retrieval defaults"),
    modelStatusRow(state),
    selectControl("Semantic backend", state.model.semanticBackend, BACKEND_OPTIONS, (value) => {
      state.model.semanticBackend = value as BackendMode;
    }),
    selectControl("Vector backend", state.model.vectorBackend, BACKEND_OPTIONS, (value) => {
      state.model.vectorBackend = value as BackendMode;
    }),
    selectControl("Provider", state.model.provider, PROVIDER_OPTIONS, (value) => {
      state.model.provider = value as ProviderKind;
    }),
    inputControl("Base URL", state.model.baseUrl, (value) => {
      state.model.baseUrl = value;
    }),
    passwordControl("API key", state.model.apiKey, (value) => {
      state.model.apiKey = value;
    }),
    inputControl("Text model", state.model.textModel, (value) => {
      state.model.textModel = value;
    }),
    inputControl("Image model", state.model.imageModel, (value) => {
      state.model.imageModel = value;
    }),
    numberControl("Dimension", state.model.dimension, (value) => {
      state.model.dimension = positiveInt(value, status.runtime.embedding_dimension);
    }),
    numberControl("Batch size", state.model.batchSize, (value) => {
      state.model.batchSize = positiveInt(value, 16);
    }),
    numberControl("Timeout ms", state.model.timeoutMs, (value) => {
      state.model.timeoutMs = positiveInt(value, 10000);
    }),
    numberControl("Max concurrency", state.model.maxConcurrency, (value) => {
      state.model.maxConcurrency = positiveInt(value, 2);
    }),
    settingsActions(state, status, health, service, callbacks)
  );

  return form;
}

function agentStatusRow(
  state: SettingsState,
  service: ServiceStatusResponse | null
): HTMLElement {
  const row = element("div", "settings-status-row");
  row.append(
    statusPill(state.agent.mcpEnabled ? "mcp enabled" : "mcp disabled", state.agent.mcpEnabled ? "good" : "warn"),
    statusPill(
      state.agent.allowRemoteClients ? "remote clients" : "local clients",
      state.agent.allowRemoteClients ? "warn" : "good"
    ),
    statusPill(
      `${service?.agent_protocols.allowed_origin_count ?? 0} origins`,
      service && service.agent_protocols.allowed_origin_count > 0 ? "good" : "warn"
    )
  );

  return row;
}

function modelStatusRow(state: SettingsState): HTMLElement {
  const row = element("div", "settings-status-row");
  const external = externalModelSelected(state);
  row.append(
    statusPill(`semantic ${state.model.semanticBackend}`, backendTone(state.model.semanticBackend)),
    statusPill(`vector ${state.model.vectorBackend}`, backendTone(state.model.vectorBackend)),
    statusPill(
      state.model.runtimeKeyConfigured || state.model.apiKey.trim().length > 0
        ? "key configured"
        : "key missing",
      external && !state.model.runtimeKeyConfigured && state.model.apiKey.trim().length === 0
        ? "bad"
        : "good"
    )
  );

  return row;
}

function backendTone(mode: BackendMode): Tone {
  if (mode === "external" || mode === "local") {
    return "good";
  }

  return "warn";
}

function settingsActions(
  state: SettingsState,
  status: ProjectStatusResponse,
  health: HealthResponse,
  service: ServiceStatusResponse | null,
  callbacks: SettingsCallbacks
): HTMLElement {
  const actions = element("div", "settings-actions");
  const reset = document.createElement("button");
  reset.type = "button";
  reset.className = "button";
  reset.dataset.testid = "reset-settings-runtime";
  reset.append(icon("refresh-icon"), document.createTextNode("Reset"));
  reset.addEventListener("click", () => {
    activeProbeRunId += 1;
    settingsState = settingsStateFromRuntime(status, service);
    settingsStateSourceKey = settingsSourceKey(status, service);
    settingsDirty = false;
    copyMessage = "";
    probeState = { state: "idle" };
    callbacks.rerender();
  });

  const probe = document.createElement("button");
  probe.type = "button";
  probe.className = "button primary";
  probe.dataset.testid = "probe-settings-provider";
  probe.disabled = probeState.state === "running" || !externalModelSelected(state);
  probe.append(icon("run-icon"), document.createTextNode("Probe"));
  probe.addEventListener("click", () => {
    void runProviderProbe(status, health, callbacks);
  });

  actions.append(reset, probe);

  return actions;
}

function settingsOutput(
  state: SettingsState,
  status: ProjectStatusResponse,
  health: HealthResponse,
  callbacks: SettingsCallbacks
): HTMLElement {
  const output = element("div", "settings-output");
  const config = generatedConfig(state);
  output.append(
    textElement("div", "panel-title", "Generated configuration"),
    preBlock("Environment", config, "settings-config-preview"),
    preBlock("Command", serviceCommand(state), "settings-command-preview"),
    outputActions(config, callbacks),
    probeResultPanel()
  );
  output.dataset.graphVersion = String(status.metadata.graph_version);
  output.dataset.indexedGraphVersion = String(health.metadata.indexed_graph_version ?? 0);

  return output;
}

function outputActions(config: string, callbacks: SettingsCallbacks): HTMLElement {
  const actions = element("div", "settings-actions");
  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "button";
  copy.dataset.testid = "copy-settings-config";
  copy.append(icon("copy-icon"), document.createTextNode("Copy"));
  copy.addEventListener("click", () => {
    void copyConfig(settingsState ? generatedConfig(settingsState) : config, callbacks);
  });
  actions.append(copy);
  if (copyMessage) {
    actions.append(textElement("span", "muted-line", copyMessage));
  }

  return actions;
}

async function copyConfig(config: string, callbacks: SettingsCallbacks) {
  try {
    if (!navigator.clipboard) {
      throw new Error("clipboard unavailable");
    }
    await navigator.clipboard.writeText(config);
    copyMessage = "Copied";
  } catch {
    copyMessage = "Copy unavailable";
  }
  callbacks.rerender();
}

async function runProviderProbe(
  status: ProjectStatusResponse,
  health: HealthResponse,
  callbacks: SettingsCallbacks
) {
  const runId = activeProbeRunId + 1;
  activeProbeRunId = runId;
  probeState = { state: "running" };
  callbacks.rerender();

  try {
    const result = await executeWebOperation(providerProbeSnapshot(status, health));
    if (runId !== activeProbeRunId) {
      return;
    }
    probeState = { state: "success", result };
    callbacks.rerender();
    await refreshDiagnosticsAfterProbe(runId, callbacks);
  } catch (error) {
    if (runId !== activeProbeRunId) {
      return;
    }
    probeState = { state: "error", message: callbacks.errorMessage(error) };
    callbacks.rerender();
  }
}

function providerProbeSnapshot(
  status: ProjectStatusResponse,
  health: HealthResponse
): WebOperationSnapshot {
  return {
    id: `settings-provider-${Date.now()}`,
    name: "Probe embedding provider",
    command: "relay-knowledge provider probe --format json",
    createdAt: new Date().toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit"
    }),
    payload: {
      operation: "provider.embedding.probe",
      input: "web-settings",
      metadata: {
        request_id: status.metadata.request_id,
        graph_version: status.metadata.graph_version,
        indexed_graph_version: health.metadata.indexed_graph_version ?? null
      }
    }
  };
}

async function refreshDiagnosticsAfterProbe(runId: number, callbacks: SettingsCallbacks) {
  try {
    const [status, health, service] = await Promise.all([
      loadProjectStatus(),
      loadHealth(),
      loadServiceStatus().catch(() => null)
    ]);
    if (runId !== activeProbeRunId) {
      return;
    }
    callbacks.setDiagnostics({ status, health, service });
    callbacks.rerender();
  } catch (error) {
    if (runId !== activeProbeRunId || probeState.state !== "success") {
      return;
    }
    probeState = {
      ...probeState,
      diagnosticsError: callbacks.errorMessage(error)
    };
    callbacks.rerender();
  }
}

function probeResultPanel(): HTMLElement {
  const panel = element("div", "settings-result");
  panel.dataset.state = probeState.state;
  if (probeState.state === "idle") {
    panel.append(textElement("div", "muted-line", "Provider probe has not run."));
  } else if (probeState.state === "running") {
    panel.append(textElement("div", "result-heading", "Probe embedding provider"));
    panel.append(textElement("div", "muted-line", "Running"));
  } else if (probeState.state === "success") {
    panel.append(
      textElement("div", "result-heading", "Probe embedding provider"),
      preBlock("Result", JSON.stringify(probeState.result, null, 2), "settings-probe-preview")
    );
    if (probeState.diagnosticsError) {
      panel.append(textElement("div", "warning-message", probeState.diagnosticsError));
    }
  } else {
    panel.append(
      textElement("div", "result-heading", "Probe embedding provider"),
      textElement("div", "error-message", probeState.message)
    );
  }

  return panel;
}

function generatedConfig(state: SettingsState): string {
  return [
    ...agentEnvironment(state),
    "",
    ...modelEnvironment(state)
  ]
    .filter((line, index, lines) => line.length > 0 || lines[index - 1]?.length > 0)
    .map((line) => (line.length === 0 ? "" : `export ${line}`))
    .join("\n");
}

function agentEnvironment(state: SettingsState): string[] {
  const values = [
    envLine("RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED", boolValue(state.agent.mcpEnabled)),
    envLine("RELAY_KNOWLEDGE_MCP_ENDPOINT", state.agent.endpoint),
    envLine("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", state.agent.allowedScopes),
    envLine(
      "RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE",
      boolValue(state.agent.allowUnspecifiedScope)
    ),
    envLine("RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS", boolValue(state.agent.allowRemoteClients)),
    envLine("RELAY_KNOWLEDGE_MCP_MAX_LIMIT", String(state.agent.maxLimit)),
    envLine("RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES", String(state.agent.maxContextBytes)),
    envLine("RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS", String(state.agent.maxRuntimeMs)),
    envLine("RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED", boolValue(state.agent.auditSinkEnabled)),
    envLine("RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH", String(state.agent.auditQueueDepth))
  ];
  if (state.agent.allowedOrigins.trim().length > 0) {
    values.splice(3, 0, envLine("RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS", state.agent.allowedOrigins));
  }

  return values;
}

function modelEnvironment(state: SettingsState): string[] {
  const values = [
    envLine("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", state.model.semanticBackend),
    envLine("RELAY_KNOWLEDGE_VECTOR_BACKEND", state.model.vectorBackend)
  ];
  if (externalModelSelected(state)) {
    values.push(
      envLine("RELAY_KNOWLEDGE_LLM_PROVIDER", state.model.provider),
      envLine("RELAY_KNOWLEDGE_EMBEDDING_BASE_URL", state.model.baseUrl),
      envLine("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", state.model.textModel),
      envLine("RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL", state.model.imageModel),
      envLine("RELAY_KNOWLEDGE_EMBEDDING_DIMENSION", String(state.model.dimension)),
      envLine("RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE", String(state.model.batchSize)),
      envLine("RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS", String(state.model.timeoutMs)),
      envLine("RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY", String(state.model.maxConcurrency))
    );
    if (state.model.apiKey.trim().length > 0) {
      values.splice(4, 0, envLine("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", state.model.apiKey.trim()));
    }
  }

  return values;
}

function serviceCommand(state: SettingsState): string {
  const parts = ["relay-knowledge", "service", "run", "--web"];
  if (state.agent.mcpEnabled) {
    parts.push("--mcp", "streamable-http");
  }

  return parts.join(" ");
}

function envLine(name: string, value: string): string {
  return `${name}=${shellValue(value)}`;
}

function shellValue(value: string): string {
  if (/^[A-Za-z0-9_./:=,@-]+$/.test(value)) {
    return value;
  }

  return `'${value.replaceAll("'", "'\\''")}'`;
}

function boolValue(value: boolean): string {
  return value ? "true" : "false";
}

function externalModelSelected(state: SettingsState): boolean {
  return state.model.semanticBackend === "external" || state.model.vectorBackend === "external";
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
    markSettingsDirty();
    onInput(input.value);
    updateSettingsPreview();
  });
  control.append(input);

  return control;
}

function passwordControl(
  label: string,
  value: string,
  onInput: (value: string) => void
): HTMLElement {
  const control = fieldShell(label);
  const input = document.createElement("input");
  input.type = "password";
  input.name = fieldName(label);
  input.autocomplete = "new-password";
  input.placeholder = "leave blank to keep current key";
  input.value = value;
  input.addEventListener("input", () => {
    markSettingsDirty();
    onInput(input.value);
    updateSettingsPreview();
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
    markSettingsDirty();
    onInput(input.value);
    updateSettingsPreview();
  });
  control.append(input);

  return control;
}

function selectControl<T extends string>(
  label: string,
  value: T,
  options: Array<[T, string]>,
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
    markSettingsDirty();
    onChange(select.value);
    updateSettingsPreview();
  });
  control.append(select);

  return control;
}

function checkboxControl(
  label: string,
  checked: boolean,
  onChange: (checked: boolean) => void
): HTMLElement {
  const control = element("label", "checkbox-row settings-checkbox");
  const input = document.createElement("input");
  input.type = "checkbox";
  input.name = fieldName(label);
  input.checked = checked;
  input.addEventListener("change", () => {
    markSettingsDirty();
    onChange(input.checked);
    updateSettingsPreview();
  });
  control.append(input, textElement("span", undefined, label));

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

function positiveInt(value: string, fallback: number): number {
  const parsed = Number.parseInt(value, 10);

  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function updateSettingsPreview() {
  if (!settingsState) {
    return;
  }
  const config = document.querySelector(".settings-config-preview");
  const command = document.querySelector(".settings-command-preview");
  const probe = document.querySelector("[data-testid='probe-settings-provider']");
  if (config) {
    config.textContent = generatedConfig(settingsState);
  }
  if (command) {
    command.textContent = serviceCommand(settingsState);
  }
  if (probe instanceof HTMLButtonElement) {
    probe.disabled = probeState.state === "running" || !externalModelSelected(settingsState);
  }
}

function markSettingsDirty() {
  settingsDirty = true;
  copyMessage = "";
}

function settingsSourceKey(
  status: ProjectStatusResponse,
  service: ServiceStatusResponse | null
): string {
  return JSON.stringify({
    runtime: status.runtime,
    protocols: service?.agent_protocols ?? null,
    operatorScopes: service?.operator.allowed_scopes ?? []
  });
}
