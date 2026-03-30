const DEFAULT_API_BASE_URL = "http://127.0.0.1:8787";

export const apiBaseUrl =
  import.meta.env.VITE_EDEN_API_BASE_URL?.replace(/\/$/, "") ?? DEFAULT_API_BASE_URL;

export interface HealthReport {
  status: string;
  service: string;
  version: string;
  now: string;
  api: {
    status: string;
    bind_addr: string;
    db_path: string;
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
  const response = await fetch(`${apiBaseUrl}${path}`, init);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return (await response.json()) as T;
}

export function fetchHealthReport(signal?: AbortSignal) {
  return fetchJson<HealthReport>("/health/report", { signal });
}
