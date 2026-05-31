import type { HealthResponse, IndexStatus, ProjectStatusResponse } from "./api/contracts";

export type OperationId =
  | "retrieve"
  | "ingest"
  | "graph"
  | "code"
  | "indexes"
  | "provider"
  | "worker"
  | "proposal"
  | "audit"
  | "service";
export type Freshness = "allow-stale" | "wait-until-fresh" | "graph-only";
export type CodeAction = "register" | "index" | "update" | "query" | "impact" | "status";
export type CodeQueryKind =
  | "hybrid"
  | "symbol"
  | "definition"
  | "references"
  | "callers"
  | "callees"
  | "imports"
  | "sbom";
export type IndexKind = IndexStatus["kind"];
export type WorkerKind = "embedding" | "ocr" | "vision" | "extractor";
export type ProposalAction = "list" | "show" | "accept" | "reject" | "supersede";

export type OperationSnapshot = {
  id: string;
  name: string;
  command: string;
  payload: Record<string, unknown>;
  createdAt: string;
};

export type AppState = {
  selectedOperation: OperationId;
  staged: OperationSnapshot[];
  retrieve: {
    query: string;
    sourceScope: string;
    freshness: Freshness;
    limit: number;
  };
  ingest: {
    sourceScope: string;
    content: string;
    entityLabels: string;
  };
  graph: {
    sourceScope: string;
  };
  code: {
    action: CodeAction;
    alias: string;
    rootPath: string;
    pathFilter: string;
    languageFilter: string;
    refSelector: string;
    baseRef: string;
    headRef: string;
    query: string;
    queryKind: CodeQueryKind;
    limit: number;
    freshness: Freshness;
  };
  indexes: {
    kinds: IndexKind[];
  };
  provider: {
    probeInput: string;
  };
  worker: {
    action: "status" | "run-once";
    kind: WorkerKind;
  };
  proposal: {
    action: ProposalAction;
    proposalId: string;
    state: "proposed" | "accepted" | "rejected" | "superseded";
    actor: string;
    reason: string;
    limit: number;
  };
  audit: {
    operation: string;
    limit: number;
  };
  service: {
    mcpTransport: "configured" | "streamable-http";
    allowedScopes: string;
  };
};

export const OPERATIONS: Array<{ id: OperationId; label: string }> = [
  { id: "retrieve", label: "Retrieve" },
  { id: "ingest", label: "Ingest" },
  { id: "graph", label: "Graph" },
  { id: "code", label: "Code" },
  { id: "indexes", label: "Indexes" },
  { id: "provider", label: "Provider" },
  { id: "worker", label: "Workers" },
  { id: "proposal", label: "Proposals" },
  { id: "audit", label: "Audit" },
  { id: "service", label: "Service" }
];

export const INDEX_KINDS: IndexKind[] = ["bm25", "semantic", "vector"];

export const appState: AppState = {
  selectedOperation: "retrieve",
  staged: [],
  retrieve: {
    query: "SQLite graph state",
    sourceScope: "docs",
    freshness: "allow-stale",
    limit: 8
  },
  ingest: {
    sourceScope: "docs",
    content: "Rust async services isolate blocking SQLite work",
    entityLabels: "Rust, SQLite"
  },
  graph: {
    sourceScope: "docs"
  },
  code: {
    action: "query",
    alias: "core",
    rootPath: "/path/to/repo",
    pathFilter: "src",
    languageFilter: "rust",
    refSelector: "HEAD",
    baseRef: "main",
    headRef: "HEAD",
    query: "retry_policy",
    queryKind: "hybrid",
    limit: 10,
    freshness: "allow-stale"
  },
  indexes: {
    kinds: ["bm25", "semantic", "vector"]
  },
  provider: {
    probeInput: "relay-knowledge provider probe"
  },
  worker: {
    action: "status",
    kind: "extractor"
  },
  proposal: {
    action: "list",
    proposalId: "proposal:extractor:example",
    state: "proposed",
    actor: "web-user",
    reason: "Reviewed in Web workspace",
    limit: 25
  },
  audit: {
    operation: "",
    limit: 50
  },
  service: {
    mcpTransport: "streamable-http",
    allowedScopes: "docs"
  }
};

export function currentOperationSnapshot(
  status: ProjectStatusResponse,
  health: HealthResponse
): OperationSnapshot {
  const createdAt = operationSnapshotTime();
  const metadata = operationMetadata(status, health);
  const snapshot = operationCommandAndPayload(metadata);

  return {
    id: `${Date.now()}-${appState.selectedOperation}`,
    createdAt,
    ...snapshot
  };
}

export function retrieveOperationSnapshot(
  status: ProjectStatusResponse,
  health: HealthResponse,
  retrieve: AppState["retrieve"]
): OperationSnapshot {
  return {
    id: `${Date.now()}-retrieve`,
    createdAt: operationSnapshotTime(),
    ...retrieveSnapshot(operationMetadata(status, health), retrieve)
  };
}

export function maxIndexLag(indexes: IndexStatus[], graphVersion: number): number {
  return Math.max(0, ...indexes.map((index) => graphVersion - index.indexed_graph_version));
}

export function uniqueKinds(kinds: IndexKind[]): IndexKind[] {
  return INDEX_KINDS.filter((kind) => kinds.includes(kind));
}

export function positiveInt(value: string, fallback: number): number {
  const parsed = Number.parseInt(value, 10);

  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

export function freshnessOptions(): Array<[Freshness, string]> {
  return [
    ["allow-stale", "allow-stale"],
    ["wait-until-fresh", "wait-until-fresh"],
    ["graph-only", "graph-only"]
  ];
}

export function codeActionOptions(): Array<[CodeAction, string]> {
  return [
    ["query", "query"],
    ["register", "register"],
    ["index", "index"],
    ["update", "update"],
    ["impact", "impact"],
    ["status", "status"]
  ];
}

export function codeQueryKindOptions(): Array<[CodeQueryKind, string]> {
  return [
    ["hybrid", "hybrid"],
    ["symbol", "symbol"],
    ["definition", "definition"],
    ["references", "references"],
    ["callers", "callers"],
    ["callees", "callees"],
    ["imports", "imports"],
    ["sbom", "sbom"]
  ];
}

function operationSnapshotTime(): string {
  return new Date().toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit"
  });
}

function operationMetadata(
  status: ProjectStatusResponse,
  health: HealthResponse
): Record<string, unknown> {
  return {
    request_id: status.metadata.request_id,
    graph_version: status.metadata.graph_version,
    indexed_graph_version: health.metadata.indexed_graph_version ?? null
  };
}

function operationCommandAndPayload(metadata: Record<string, unknown>): {
  name: string;
  command: string;
  payload: Record<string, unknown>;
} {
  switch (appState.selectedOperation) {
    case "retrieve":
      return retrieveSnapshot(metadata);
    case "ingest":
      return ingestSnapshot(metadata);
    case "graph":
      return graphSnapshot(metadata);
    case "code":
      return codeSnapshot(metadata);
    case "indexes":
      return indexesSnapshot(metadata);
    case "provider":
      return providerSnapshot(metadata);
    case "worker":
      return workerSnapshot(metadata);
    case "proposal":
      return proposalSnapshot(metadata);
    case "audit":
      return auditSnapshot(metadata);
    case "service":
      return serviceSnapshot(metadata);
  }
}

function retrieveSnapshot(metadata: Record<string, unknown>, retrieve = appState.retrieve) {
  const command = shellCommand([
    "relay-knowledge",
    "query",
    retrieve.query,
    "--source",
    retrieve.sourceScope,
    "--freshness",
    retrieve.freshness,
    "--limit",
    String(retrieve.limit),
    "--format",
    "streaming-json"
  ]);
  const payload = {
    operation: "retrieve.context",
    query: retrieve.query,
    source_scope: retrieve.sourceScope,
    freshness: retrieve.freshness,
    limit: retrieve.limit,
    metadata
  };

  return { name: "Retrieve context", command, payload };
}

function ingestSnapshot(metadata: Record<string, unknown>) {
  const labels = commaList(appState.ingest.entityLabels);
  const command = shellCommand([
    "relay-knowledge",
    "ingest",
    "--source",
    appState.ingest.sourceScope,
    "--content",
    appState.ingest.content,
    ...labels.flatMap((label) => ["--entity", label]),
    "--format",
    "streaming-json"
  ]);
  const payload = {
    operation: "graph.ingest",
    source_scope: appState.ingest.sourceScope,
    content: appState.ingest.content,
    entity_labels: labels,
    metadata
  };

  return { name: "Ingest evidence", command, payload };
}

function graphSnapshot(metadata: Record<string, unknown>) {
  const parts = ["relay-knowledge", "graph", "inspect"];
  if (appState.graph.sourceScope.trim().length > 0) {
    parts.push("--source", appState.graph.sourceScope);
  }
  parts.push("--format", "json");

  return {
    name: "Inspect graph",
    command: shellCommand(parts),
    payload: {
      operation: "graph.inspect",
      source_scope: appState.graph.sourceScope || null,
      metadata
    }
  };
}

function codeSnapshot(metadata: Record<string, unknown>) {
  const state = appState.code;
  const parts =
    state.action === "register"
      ? ["relay-knowledge", "repo", state.action, state.rootPath]
      : ["relay-knowledge", "repo", state.action, state.alias];
  const payload: Record<string, unknown> = {
    operation: `code.repo.${state.action}`,
    alias: state.alias,
    metadata
  };

  if (state.action === "register") {
    appendOption(parts, "--alias", state.alias);
    appendOption(parts, "--path", state.pathFilter);
    appendOption(parts, "--language", state.languageFilter);
    payload.root_path = state.rootPath;
    payload.path_filters = commaList(state.pathFilter);
    payload.language_filters = commaList(state.languageFilter);
  } else if (state.action === "index") {
    appendOption(parts, "--ref", state.refSelector);
    payload.ref = state.refSelector;
  } else if (state.action === "update") {
    appendOption(parts, "--base", state.baseRef);
    appendOption(parts, "--head", state.headRef);
    payload.base_ref = state.baseRef;
    payload.head_ref = state.headRef;
  } else if (state.action === "impact") {
    appendOption(parts, "--base", state.baseRef);
    appendOption(parts, "--head", state.headRef);
    appendOption(parts, "--limit", String(state.limit));
    payload.base_ref = state.baseRef;
    payload.head_ref = state.headRef;
    payload.limit = state.limit;
  } else if (state.action === "query") {
    appendOption(parts, "--query", state.query);
    appendOption(parts, "--kind", state.queryKind);
    appendOption(parts, "--limit", String(state.limit));
    appendOption(parts, "--ref", state.refSelector);
    appendOption(parts, "--path", state.pathFilter);
    appendOption(parts, "--language", state.languageFilter);
    appendOption(parts, "--freshness", state.freshness);
    payload.query = state.query;
    payload.kind = state.queryKind;
    payload.limit = state.limit;
    payload.ref = state.refSelector;
    payload.path_filters = commaList(state.pathFilter);
    payload.language_filters = commaList(state.languageFilter);
    payload.freshness = state.freshness;
  }
  parts.push("--format", "json");

  return { name: `Code ${state.action}`, command: shellCommand(parts), payload };
}

function indexesSnapshot(metadata: Record<string, unknown>) {
  const kinds = appState.indexes.kinds;
  const command = shellCommand([
    "relay-knowledge",
    "index",
    "refresh",
    ...kinds.flatMap((kind) => ["--kind", kind]),
    "--format",
    "streaming-json"
  ]);

  return {
    name: "Refresh indexes",
    command,
    payload: {
      operation: "index.refresh",
      kinds,
      metadata
    }
  };
}

function providerSnapshot(metadata: Record<string, unknown>) {
  return {
    name: "Probe embedding provider",
    command: shellCommand(["relay-knowledge", "provider", "probe", "--format", "json"]),
    payload: {
      operation: "provider.embedding.probe",
      input: appState.provider.probeInput,
      metadata
    }
  };
}

function workerSnapshot(metadata: Record<string, unknown>) {
  const parts = ["relay-knowledge", "worker", appState.worker.action];
  appendOption(parts, "--kind", appState.worker.kind);
  parts.push("--format", "json");

  return {
    name: `Worker ${appState.worker.action}`,
    command: shellCommand(parts),
    payload: {
      operation: `worker.${appState.worker.action}`,
      kind: appState.worker.kind,
      metadata
    }
  };
}

function proposalSnapshot(metadata: Record<string, unknown>) {
  const state = appState.proposal;
  const parts = ["relay-knowledge", "proposal", state.action];
  if (state.action === "list") {
    appendOption(parts, "--state", state.state);
    appendOption(parts, "--limit", String(state.limit));
  } else {
    parts.push(state.proposalId);
    if (state.action !== "show") {
      appendOption(parts, "--by", state.actor);
      appendOption(parts, "--reason", state.reason);
    }
  }
  parts.push("--format", "json");

  return {
    name: `Proposal ${state.action}`,
    command: shellCommand(parts),
    payload: {
      operation: `proposal.${state.action}`,
      proposal_id: state.action === "list" ? null : state.proposalId,
      state: state.action === "list" ? state.state : null,
      actor: state.action === "show" || state.action === "list" ? null : state.actor,
      reason: state.action === "show" || state.action === "list" ? null : state.reason,
      limit: state.action === "list" ? state.limit : null,
      metadata
    }
  };
}

function auditSnapshot(metadata: Record<string, unknown>) {
  const parts = ["relay-knowledge", "audit", "query"];
  appendOption(parts, "--operation", appState.audit.operation);
  appendOption(parts, "--limit", String(appState.audit.limit));
  parts.push("--format", "json");

  return {
    name: "Audit query",
    command: shellCommand(parts),
    payload: {
      operation: "audit.query",
      filter_operation: appState.audit.operation || null,
      limit: appState.audit.limit,
      metadata
    }
  };
}

function serviceSnapshot(metadata: Record<string, unknown>) {
  const command =
    appState.service.mcpTransport === "streamable-http"
      ? shellCommand([
          `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=${appState.service.allowedScopes}`,
          "relay-knowledge",
          "service",
          "run",
          "--mcp",
          "streamable-http"
        ])
      : shellCommand(["relay-knowledge", "service", "doctor", "--format", "json"]);

  return {
    name: "Service runtime",
    command,
    payload: {
      operation:
        appState.service.mcpTransport === "streamable-http"
          ? "service.run.streamable_http"
          : "service.doctor",
      allowed_scopes: commaList(appState.service.allowedScopes),
      metadata
    }
  };
}

function appendOption(parts: string[], name: string, value: string) {
  if (value.trim().length > 0) {
    parts.push(name, value);
  }
}

function commaList(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function shellCommand(parts: string[]): string {
  return parts.map(shellArg).join(" ");
}

function shellArg(value: string): string {
  if (/^[A-Za-z0-9_./:=@-]+$/.test(value)) {
    return value;
  }

  return `'${value.replaceAll("'", "'\\''")}'`;
}
