const API_BASE = process.env.NEXT_PUBLIC_EDEN_API_URL || "http://localhost:8787";
const API_KEY = process.env.NEXT_PUBLIC_EDEN_API_KEY || "";

async function edenFetch<T>(path: string, params?: Record<string, string>): Promise<T> {
  const url = new URL(`/api${path}`, API_BASE);
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v) url.searchParams.set(k, v);
    }
  }
  const res = await fetch(url.toString(), {
    headers: {
      Authorization: `Bearer ${API_KEY}`,
    },
    cache: "no-store",
  });
  if (!res.ok) {
    throw new Error(`Eden API ${path}: ${res.status} ${res.statusText}`);
  }
  return res.json();
}

// ── Types ──

export interface PolymarketPrior {
  slug: string;
  label: string;
  question: string;
  probability: string;
  bias: string;
  active: boolean;
  closed: boolean;
  category: string | null;
}

export interface PolymarketSnapshot {
  fetched_at: string;
  priors: PolymarketPrior[];
}

export interface LineageOutcome {
  label: string;
  total: number;
  resolved: number;
  hits: number;
  hit_rate: string;
  mean_return: string;
  mean_net_return: string;
  mean_mfe: string;
  mean_mae: string;
  follow_through_rate: string;
  invalidation_rate: string;
  structure_retention_rate: string;
}

export interface LineageStats {
  based_on: [string, number][];
  blocked_by: [string, number][];
  promoted_by: [string, number][];
  falsified_by: [string, number][];
  promoted_outcomes: LineageOutcome[];
  blocked_outcomes: LineageOutcome[];
  falsified_outcomes: LineageOutcome[];
}

export interface LineageSnapshot {
  snapshot_id: string;
  tick_number: number;
  recorded_at: string;
  window_size: number;
  stats: LineageStats;
}

export interface CausalFlip {
  scope_key: string;
  tick: number;
  prev_leader: string;
  new_leader: string;
  style: string;
  gap: string;
}

export interface CausalTimeline {
  scope_key: string;
  points: { tick: number; leader: string; confidence: string }[];
  flips: CausalFlip[];
}

// ── Fetchers ──

export function fetchPolymarket(): Promise<PolymarketSnapshot> {
  return edenFetch("/polymarket");
}

export function fetchLineage(limit?: number, top?: number): Promise<LineageStats> {
  return edenFetch("/lineage", {
    limit: String(limit || 120),
    top: String(top || 10),
  });
}

export function fetchLineageHistory(limit?: number): Promise<LineageSnapshot[]> {
  return edenFetch("/lineage/history", { limit: String(limit || 20) });
}

export function fetchCausalFlips(limit?: number): Promise<CausalFlip[]> {
  return edenFetch("/causal/flips", { limit: String(limit || 50) });
}

export function fetchCausalTimeline(scopeKey: string, limit?: number): Promise<CausalTimeline> {
  return edenFetch(`/causal/timeline/${encodeURIComponent(scopeKey)}`, {
    limit: String(limit || 120),
  });
}
