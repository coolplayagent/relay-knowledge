import type {
  HealthResponse,
  ProjectStatusResponse,
  ServiceStatusResponse,
  WebOperationExecuteResponse,
  WebOperationSnapshot
} from "./contracts";

const PROJECT_STATUS_PATH = "/api/project/status";
const HEALTH_PATH = "/api/health";
const SERVICE_STATUS_PATH = "/api/service/status";
const OPERATION_EXECUTE_PATH = "/api/web/operations/execute";
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
