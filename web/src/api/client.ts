import type {
  GraphCanvasKind,
  GraphCanvasResponse,
  HealthResponse,
  ModelCatalogResult,
  ModelConnectivityProbeResult,
  ModelDiscoveryResult,
  ModelFallbackConfig,
  ModelProfileSaveRequest,
  ModelProfilesResponse,
  ProjectStatusResponse,
  ServiceStatusResponse,
  WebOperationExecuteResponse,
  WebOperationSnapshot
} from "./contracts";

const PROJECT_STATUS_PATH = "/api/project/status";
const HEALTH_PATH = "/api/health";
const SERVICE_STATUS_PATH = "/api/service/status";
const GRAPH_CANVAS_PATH = "/api/web/graph/canvas";
const OPERATION_EXECUTE_PATH = "/api/web/operations/execute";
const MODEL_PROFILES_PATH = "/api/configs/model/profiles";
const MODEL_FALLBACK_PATH = "/api/configs/model-fallback";
const MODEL_CATALOG_PATH = "/api/configs/model/catalog";
const MODEL_PROBE_PATH = "/api/configs/model:probe";
const MODEL_DISCOVER_PATH = "/api/configs/model:discover";
const REQUEST_TIMEOUT_MS = 10000;
const NO_CLIENT_TIMEOUT = null;

export async function loadProjectStatus(): Promise<ProjectStatusResponse> {
  return fetchJson<ProjectStatusResponse>(PROJECT_STATUS_PATH);
}

export async function loadHealth(): Promise<HealthResponse> {
  return fetchJson<HealthResponse>(HEALTH_PATH);
}

export async function loadServiceStatus(): Promise<ServiceStatusResponse> {
  return fetchJson<ServiceStatusResponse>(SERVICE_STATUS_PATH);
}

export async function loadGraphCanvas(params: {
  kind: GraphCanvasKind;
  sourceScope?: string;
  query?: string;
  limit: number;
}): Promise<GraphCanvasResponse> {
  const query = new URLSearchParams();
  query.set("kind", params.kind);
  query.set("limit", String(params.limit));
  if (params.sourceScope) {
    query.set("scope", params.sourceScope);
  }
  if (params.query) {
    query.set("query", params.query);
  }

  return fetchJson<GraphCanvasResponse>(`${GRAPH_CANVAS_PATH}?${query.toString()}`);
}

export async function executeWebOperation(
  snapshot: WebOperationSnapshot
): Promise<WebOperationExecuteResponse> {
  return fetchJson<WebOperationExecuteResponse>(
    OPERATION_EXECUTE_PATH,
    {
      method: "POST",
      body: JSON.stringify({ snapshot }),
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json"
      }
    },
    {
      timeoutMs: NO_CLIENT_TIMEOUT
    }
  );
}

export async function loadModelProfiles(): Promise<ModelProfilesResponse> {
  return fetchJson<ModelProfilesResponse>(MODEL_PROFILES_PATH);
}

export async function saveModelProfile(
  name: string,
  profile: ModelProfileSaveRequest
): Promise<ModelProfilesResponse> {
  return fetchJson<ModelProfilesResponse>(`${MODEL_PROFILES_PATH}/${encodeURIComponent(name)}`, {
    method: "PUT",
    body: JSON.stringify(profile),
    headers: {
      "Content-Type": "application/json"
    }
  });
}

export async function deleteModelProfile(name: string): Promise<ModelProfilesResponse> {
  return fetchJson<ModelProfilesResponse>(`${MODEL_PROFILES_PATH}/${encodeURIComponent(name)}`, {
    method: "DELETE"
  });
}

export async function loadModelFallbackConfig(): Promise<ModelFallbackConfig> {
  return fetchJson<ModelFallbackConfig>(MODEL_FALLBACK_PATH);
}

export async function saveModelFallbackConfig(
  config: ModelFallbackConfig
): Promise<ModelFallbackConfig> {
  return fetchJson<ModelFallbackConfig>(MODEL_FALLBACK_PATH, {
    method: "PUT",
    body: JSON.stringify(config),
    headers: {
      "Content-Type": "application/json"
    }
  });
}

export async function loadModelCatalog(refresh = false): Promise<ModelCatalogResult> {
  const suffix = refresh ? "?refresh=true" : "";
  return fetchJson<ModelCatalogResult>(`${MODEL_CATALOG_PATH}${suffix}`);
}

export async function refreshModelCatalog(): Promise<ModelCatalogResult> {
  return fetchJson<ModelCatalogResult>(`${MODEL_CATALOG_PATH}:refresh`, {
    method: "POST"
  });
}

export async function probeModelProfile(params: {
  profile_name?: string;
  override_config?: ModelProfileSaveRequest;
  timeout_ms?: number;
}): Promise<ModelConnectivityProbeResult> {
  return fetchJson<ModelConnectivityProbeResult>(
    MODEL_PROBE_PATH,
    {
      method: "POST",
      body: JSON.stringify(params),
      headers: {
        "Content-Type": "application/json"
      }
    },
    {
      timeoutMs: NO_CLIENT_TIMEOUT
    }
  );
}

export async function discoverModelProfile(params: {
  profile_name?: string;
  override_config?: ModelProfileSaveRequest;
  timeout_ms?: number;
}): Promise<ModelDiscoveryResult> {
  return fetchJson<ModelDiscoveryResult>(
    MODEL_DISCOVER_PATH,
    {
      method: "POST",
      body: JSON.stringify(params),
      headers: {
        "Content-Type": "application/json"
      }
    },
    {
      timeoutMs: NO_CLIENT_TIMEOUT
    }
  );
}

async function fetchJson<T>(
  path: string,
  init?: RequestInit,
  options: { timeoutMs?: number | null } = {}
): Promise<T> {
  const controller = new AbortController();
  const timeoutMs = options.timeoutMs === undefined ? REQUEST_TIMEOUT_MS : options.timeoutMs;
  const timeout =
    timeoutMs === null ? null : window.setTimeout(() => controller.abort(), timeoutMs);

  try {
    const response = await fetch(path, {
      ...init,
      headers: { Accept: "application/json", ...init?.headers },
      signal: controller.signal
    });
    if (!response.ok) {
      const details = await response.text();
      throw new Error(
        details ? `${path} returned ${response.status}: ${details}` : `${path} returned ${response.status}`
      );
    }

    return (await response.json()) as T;
  } finally {
    if (timeout !== null) {
      window.clearTimeout(timeout);
    }
  }
}
