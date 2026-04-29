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

function caseReasoningPriority(item: CaseContract): number {
  const driverScore =
    item.driver_class === "sector_wave" ? 40
      : item.driver_class === "company_specific" ? 34
      : item.driver_class === "liquidity_dislocation" ? 28
      : item.driver_class === "institutional" ? 24
      : item.driver_class === "capital_flow" ? 22
      : item.driver_class === "trade_flow" ? 20
      : item.driver_class === "microstructure" ? 18
      : item.driver_class === "mixed_structural" ? 10
      : 0;
  const peerScore = Math.round((item.peer_confirmation_ratio ?? 0) * 100);
  const marginScore = Math.round((item.competition_margin ?? 0) * 60);
  const lifecycleScore =
    item.lifecycle_phase === "growing" || item.lifecycle_phase === "Growing" ? 12
      : item.lifecycle_phase === "peaking" || item.lifecycle_phase === "Peaking" ? 4
      : item.lifecycle_phase === "new" || item.lifecycle_phase === "New" ? 2
      : item.lifecycle_phase === "fading" || item.lifecycle_phase === "Fading" ? -8
      : 0;
  return driverScore + peerScore + marginScore + lifecycleScore;
}

function recommendationReasoningPriority(item: RecommendationContract): number {
  const summary = item.summary;
  const driverScore =
    summary.driver_class === "sector_wave" ? 40
      : summary.driver_class === "company_specific" ? 34
      : summary.driver_class === "liquidity_dislocation" ? 28
      : summary.driver_class === "institutional" ? 24
      : summary.driver_class === "capital_flow" ? 22
      : summary.driver_class === "trade_flow" ? 20
      : summary.driver_class === "microstructure" ? 18
      : summary.driver_class === "mixed_structural" ? 10
      : 0;
  const peerScore = Math.round((summary.peer_confirmation_ratio ?? 0) * 100);
  const marginScore = Math.round((summary.competition_margin ?? 0) * 60);
  const lifecycleScore =
    summary.lifecycle_phase === "growing" || summary.lifecycle_phase === "Growing" ? 12
      : summary.lifecycle_phase === "peaking" || summary.lifecycle_phase === "Peaking" ? 4
      : summary.lifecycle_phase === "new" || summary.lifecycle_phase === "New" ? 2
      : summary.lifecycle_phase === "fading" || summary.lifecycle_phase === "Fading" ? -8
      : 0;
  return driverScore + peerScore + marginScore + lifecycleScore;
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

  const leftReasoning = caseReasoningPriority(left);
  const rightReasoning = caseReasoningPriority(right);
  if (rightReasoning !== leftReasoning) return rightReasoning - leftReasoning;

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

  const leftReasoning = recommendationReasoningPriority(left);
  const rightReasoning = recommendationReasoningPriority(right);
  if (rightReasoning !== leftReasoning) return rightReasoning - leftReasoning;

  const leftAlpha = left.recommendation.expected_net_alpha ?? Number.NEGATIVE_INFINITY;
  const rightAlpha = right.recommendation.expected_net_alpha ?? Number.NEGATIVE_INFINITY;
  if (rightAlpha !== leftAlpha) return rightAlpha - leftAlpha;

  const leftConfidence = left.recommendation.confidence ?? 0;
  const rightConfidence = right.recommendation.confidence ?? 0;
  if (rightConfidence !== leftConfidence) return rightConfidence - leftConfidence;

  return left.recommendation.symbol.localeCompare(right.recommendation.symbol);
}
