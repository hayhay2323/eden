import type {
  CaseWorkflowState,
  ContextStatus,
  OperatorWorkItem,
  OperationalGraphNodeResponse,
  OperationalHistoryRecord,
  OperationalNavigation,
  OperationalNeighborhood,
  OperationalSnapshot,
  RuntimeTaskRecord,
  IntentDirection,
  IntentKind,
  WorldIntentReflectionQuery,
} from "./types";

const DEFAULT_API_BASE_URL = "http://127.0.0.1:8787";

export const apiBaseUrl =
  import.meta.env.VITE_EDEN_API_BASE_URL?.replace(/\/$/, "") ?? DEFAULT_API_BASE_URL;

function getApiKey(): string | null {
  return import.meta.env.VITE_EDEN_API_KEY ?? localStorage.getItem("eden_api_key");
}

function authHeaders(): Record<string, string> {
  const key = getApiKey();
  return key ? { "x-api-key": key } : {};
}

export interface HealthReport {
  status: string;
  service: string;
  version: string;
  now: string;
  api: {
    status: string;
    bind_addr: string;
    db_path: string;
    runtime_tasks_path: string;
    runtime_task_count: number;
    persistence_enabled: boolean;
    query_auth_enabled: boolean;
    cors_mode: string;
    allowed_origins: string[];
    revocation_path: string;
    revoked_token_count: number;
  };
  runtimes: Array<{
    status: string;
    market: string;
    debounce_ms: number;
    rest_refresh_secs: number;
    metrics_every_ticks: number;
    db_path: string;
    runtime_log_path?: string | null;
    artifacts: Array<{
      kind: string;
      status: string;
      path: string;
      exists: boolean;
      size_bytes?: number | null;
      modified_at?: string | null;
      age_secs?: number | null;
    }>;
    issue_summary: {
      warning_count: number;
      error_count: number;
      last_issue_codes?: string[];
    };
    recent_runtime_events: unknown[];
  }>;
}

async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = { ...authHeaders(), ...init?.headers };
  const response = await fetch(`${apiBaseUrl}${path}`, { ...init, headers });

  if (response.status === 401) {
    const currentKey = localStorage.getItem("eden_api_key");
    const key = prompt("Eden API Key:");
    if (key && key !== currentKey) {
      localStorage.setItem("eden_api_key", key);
      return fetchJson<T>(path, init);
    }
    throw new Error("Authentication failed");
  }

  if (!response.ok) {
    let detail = "";
    try {
      const payload = await response.clone().json();
      if (
        payload &&
        typeof payload === "object" &&
        "error" in payload &&
        typeof payload.error === "string"
      ) {
        detail = payload.error;
      }
    } catch {
      try {
        const text = await response.text();
        if (text.trim().length > 0) {
          detail = text.trim();
        }
      } catch {
        // Ignore body parsing errors and fall back to status text.
      }
    }

    throw new Error(
      detail.length > 0
        ? `${response.status} ${response.statusText}: ${detail}`
        : `${response.status} ${response.statusText}`,
    );
  }
  try {
    return (await response.json()) as T;
  } catch (e) {
    console.error("[eden] JSON parse failed:", e);
    throw e;
  }
}

async function postJson<T>(
  path: string,
  body: unknown,
  init?: Omit<RequestInit, "body" | "method">,
): Promise<T> {
  return fetchJson<T>(path, {
    ...init,
    method: "POST",
    headers: {
      "content-type": "application/json",
      ...init?.headers,
    },
    body: JSON.stringify(body),
  });
}

export function fetchHealthReport(signal?: AbortSignal) {
  return fetchJson<HealthReport>("/health/report", { signal });
}

export function fetchOperationalSnapshot(market: string, signal?: AbortSignal) {
  return fetchJson<OperationalSnapshot>(
    `/api/ontology/${market}/operational-snapshot`,
    { signal },
  );
}

export function fetchOperationalNavigation(
  market: string,
  kind: string,
  id: string,
  signal?: AbortSignal,
) {
  return fetchJson<OperationalNavigation>(
    `/api/ontology/${market}/navigation/${kind}/${encodeURIComponent(id)}`,
    { signal },
  );
}

export function fetchOperationalNeighborhood(
  market: string,
  kind: string,
  id: string,
  signal?: AbortSignal,
) {
  return fetchJson<OperationalNeighborhood>(
    `/api/ontology/${market}/neighborhood/${kind}/${encodeURIComponent(id)}`,
    { signal },
  );
}

export function fetchOperationalHistory(
  endpoint: string,
  signal?: AbortSignal,
  limit = 20,
) {
  const path = endpoint.includes("?")
    ? `${endpoint}&limit=${limit}`
    : `${endpoint}?limit=${limit}`;
  return fetchJson<OperationalHistoryRecord[]>(path, { signal });
}

export function fetchOperationalGraphNode(
  endpoint: string,
  signal?: AbortSignal,
  limit = 24,
) {
  const path = endpoint.includes("?")
    ? `${endpoint}&limit=${limit}`
    : `${endpoint}?limit=${limit}`;
  return fetchJson<OperationalGraphNodeResponse>(path, { signal });
}

export function buildOperationalGraphNodeEndpoint(market: string, nodeId: string) {
  return `/api/ontology/${market}/graph/node/${encodeURIComponent(nodeId)}`;
}

export function fetchLiveSnapshot(market: string, signal?: AbortSignal) {
  if (market === "us") {
    return fetchJson<unknown>("/api/us/live", { signal });
  }
  return fetchJson<unknown>("/api/live", { signal });
}

export interface WorldReflectionRequest {
  kind?: IntentKind;
  direction?: IntentDirection;
  limit?: number;
}

export function fetchAgentWorldReflection(
  market: string,
  options: WorldReflectionRequest = {},
  signal?: AbortSignal,
) {
  const params = new URLSearchParams();
  if (options.kind) params.set("kind", options.kind);
  if (options.direction) params.set("direction", options.direction);
  if (options.limit != null) params.set("limit", String(options.limit));
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return fetchJson<WorldIntentReflectionQuery>(
    `/api/agent/${encodeURIComponent(market)}/world/reflection${suffix}`,
    { signal },
  );
}

export function postCaseTransition(
  market: string,
  setupId: string,
  body: {
    target_stage: string;
    actor?: string;
    note?: string;
  },
) {
  return postJson<CaseWorkflowState>(
    `/api/cases/${market}/${encodeURIComponent(setupId)}/transition`,
    body,
  );
}

export function postCaseAssign(
  market: string,
  setupId: string,
  body: {
    owner?: string | null;
    reviewer?: string | null;
    queue_pin?: string | null;
    actor?: string;
    note?: string;
  },
) {
  return postJson<CaseWorkflowState>(
    `/api/cases/${market}/${encodeURIComponent(setupId)}/assign`,
    body,
  );
}

export function fetchContextStatus(signal?: AbortSignal) {
  return fetchJson<ContextStatus>("/api/status/context", { signal });
}

export function fetchRuntimeTasks(
  filters: {
    market?: string;
    status?: string;
    kind?: string;
    owner?: string;
  } = {},
  signal?: AbortSignal,
) {
  const params = new URLSearchParams();
  if (filters.market) params.set("market", filters.market);
  if (filters.status) params.set("status", filters.status);
  if (filters.kind) params.set("kind", filters.kind);
  if (filters.owner) params.set("owner", filters.owner);
  const suffix = params.toString();
  return fetchJson<RuntimeTaskRecord[]>(
    suffix.length > 0 ? `/api/runtime/tasks?${suffix}` : "/api/runtime/tasks",
    { signal },
  );
}

export function fetchOperatorWorkItems(
  market: string,
  filters: {
    symbol?: string;
    action?: string;
  } = {},
  signal?: AbortSignal,
) {
  const params = new URLSearchParams();
  if (filters.symbol) params.set("symbol", filters.symbol);
  if (filters.action) params.set("action", filters.action);
  const suffix = params.toString();
  return fetchJson<OperatorWorkItem[]>(
    suffix.length > 0
      ? `/api/ontology/${market}/operator-work-items?${suffix}`
      : `/api/ontology/${market}/operator-work-items`,
    { signal },
  );
}

export function postCaseQueuePin(
  market: string,
  setupId: string,
  body: {
    pinned: boolean;
    label?: string;
    actor?: string;
    note?: string;
  },
) {
  return postJson<CaseWorkflowState>(
    `/api/cases/${market}/${encodeURIComponent(setupId)}/queue-pin`,
    body,
  );
}
