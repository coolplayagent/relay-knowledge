import type { HealthResponse, ProjectStatusResponse } from "./contracts";

const PROJECT_STATUS_PATH = "/api/project/status";
const HEALTH_PATH = "/api/health";
const REQUEST_TIMEOUT_MS = 10000;

export async function loadProjectStatus(): Promise<ProjectStatusResponse> {
  return fetchJson<ProjectStatusResponse>(PROJECT_STATUS_PATH);
}

export async function loadHealth(): Promise<HealthResponse> {
  return fetchJson<HealthResponse>(HEALTH_PATH);
}

async function fetchJson<T>(path: string): Promise<T> {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);

  try {
    const response = await fetch(path, {
      headers: { Accept: "application/json" },
      signal: controller.signal
    });
    if (!response.ok) {
      throw new Error(`${path} returned ${response.status}`);
    }

    return (await response.json()) as T;
  } finally {
    window.clearTimeout(timeout);
  }
}
