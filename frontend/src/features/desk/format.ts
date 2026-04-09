import type { CaseContract, LiveLineageMetric, RecommendationContract } from "@/lib/api/types";

export function pct(v: number | null | undefined): string {
  if (v == null) return "-";
  return `${(Number(v) * 100).toFixed(1)}%`;
}

export function signed(v: number | null | undefined): string {
  if (v == null) return "-";
  const n = Number(v);
  return `${n >= 0 ? "+" : ""}${n.toFixed(4)}`;
}

export function numCls(v: number | null | undefined): string {
  if (v == null) return "";
  return Number(v) > 0 ? "text-positive" : Number(v) < 0 ? "text-negative" : "";
}

export function actionCls(action: string): string {
  const a = action.toLowerCase();
  if (a.includes("enter") || a.includes("add") || a.includes("buy")) return "action--enter";
  if (a.includes("monitor") || a.includes("watch") || a.includes("observe")) return "action--monitor";
  if (a.includes("review")) return "action--review";
  if (a.includes("trim") || a.includes("exit") || a.includes("sell")) return "action--trim";
  return "action--ignore";
}

export const LENS: Record<string, string> = {
  structural: "結構性",
  cross_market_arbitrage: "跨市場套利",
  sector_rotation: "板塊輪動",
  momentum_continuation: "動量延續",
  pre_market_positioning: "盤前佈局",
  graph_neighbors: "圖譜聯動",
  pre_market_gap: "跳空缺口",
};

export const BIAS: Record<string, string> = {
  bullish: "看多",
  bearish: "看空",
  neutral: "中性",
};

function normalizeFamily(value: string | null | undefined): string {
  return (value ?? "")
    .toLowerCase()
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function normalizedFamilyKey(value: string | null | undefined): string {
  return normalizeFamily(value);
}

export function supportedFamilyKeys(lineage: LiveLineageMetric[] | null | undefined): Set<string> {
  const supported = new Set<string>();
  for (const item of lineage ?? []) {
    if (!item.horizon || item.horizon === "50t") continue;
    if ((item.mean_return ?? 0) <= 0) continue;
    supported.add(normalizeFamily(item.template));
  }
  return supported;
}

function caseFamilyKey(item: CaseContract): string {
  return normalizeFamily(item.thesis_family);
}

function recommendationFamilyKey(item: RecommendationContract): string {
  return normalizeFamily(item.recommendation.thesis_family);
}

function familySupported(family: string, supported: Set<string>): boolean {
  if (!family) return false;
  if (supported.has(family)) return true;
  for (const candidate of supported) {
    if (candidate.includes(family) || family.includes(candidate)) return true;
  }
  return false;
}

export function isFamilySupported(family: string, supported: Set<string>): boolean {
  return familySupported(family, supported);
}

function caseActionRank(action: string): number {
  const a = action.toLowerCase();
  if (a.includes("enter")) return 0;
  if (a.includes("review")) return 1;
  if (a.includes("monitor") || a.includes("watch")) return 2;
  if (a.includes("observe")) return 3;
  return 4;
}

export function compareCasesByOperationalPriority(
  left: CaseContract,
  right: CaseContract,
  supportedFamilies: Set<string>,
): number {
  const leftSupported = familySupported(caseFamilyKey(left), supportedFamilies);
  const rightSupported = familySupported(caseFamilyKey(right), supportedFamilies);
  if (leftSupported !== rightSupported) return leftSupported ? -1 : 1;

  const leftBlocked = Boolean(left.multi_horizon_gate_reason);
  const rightBlocked = Boolean(right.multi_horizon_gate_reason);
  if (leftBlocked !== rightBlocked) return leftBlocked ? 1 : -1;

  const leftActionRank = caseActionRank(left.action);
  const rightActionRank = caseActionRank(right.action);
  if (leftActionRank !== rightActionRank) return leftActionRank - rightActionRank;

  if ((right.confidence ?? 0) !== (left.confidence ?? 0)) {
    return (right.confidence ?? 0) - (left.confidence ?? 0);
  }
  return left.title.localeCompare(right.title);
}

export function compareRecommendationsByOperationalPriority(
  left: RecommendationContract,
  right: RecommendationContract,
  supportedFamilies: Set<string>,
): number {
  const leftSupported = familySupported(recommendationFamilyKey(left), supportedFamilies);
  const rightSupported = familySupported(recommendationFamilyKey(right), supportedFamilies);
  if (leftSupported !== rightSupported) return leftSupported ? -1 : 1;

  const leftAlpha = left.recommendation.expected_net_alpha ?? Number.NEGATIVE_INFINITY;
  const rightAlpha = right.recommendation.expected_net_alpha ?? Number.NEGATIVE_INFINITY;
  if (rightAlpha !== leftAlpha) return rightAlpha - leftAlpha;

  const leftConfidence = left.recommendation.confidence ?? 0;
  const rightConfidence = right.recommendation.confidence ?? 0;
  if (rightConfidence !== leftConfidence) return rightConfidence - leftConfidence;

  return left.recommendation.symbol.localeCompare(right.recommendation.symbol);
}
