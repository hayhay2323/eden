import type {
  CaseContract,
  LiveLineageMetric,
  LiveSuccessPattern,
  LiveTemporalBar,
  MacroEventContract,
  OperationalSnapshot,
  RecommendationContract,
  SymbolStateContract,
  WorkflowContract,
} from "@/lib/api/types";

import { numCls, pct, signed } from "../format";
import { Stat } from "./shared";

function barsForSymbol(
  snapshot: OperationalSnapshot | null | undefined,
  symbol: string | null | undefined,
): LiveTemporalBar[] {
  if (!snapshot || !symbol) return [];
  return (snapshot.temporal_bars ?? []).filter((item) => item.symbol === symbol).slice(0, 4);
}

function normalizeFamily(value: string | null | undefined): string {
  return (value ?? "")
    .toLowerCase()
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function lineageForFamily(
  snapshot: OperationalSnapshot | null | undefined,
  family: string | null | undefined,
): LiveLineageMetric[] {
  const normalized = normalizeFamily(family);
  if (!snapshot || !normalized) return [];
  return (snapshot.lineage ?? []).filter((item) => {
    const candidate = normalizeFamily(item.template);
    return candidate === normalized || candidate.includes(normalized) || normalized.includes(candidate);
  });
}

function successPatternBySignature(
  snapshot: OperationalSnapshot | null | undefined,
  signature: string | null | undefined,
): LiveSuccessPattern | null {
  if (!snapshot || !signature) return null;
  return (snapshot.success_patterns ?? []).find((item) => item.signature === signature) ?? null;
}

function successPatternsForSymbol(
  snapshot: OperationalSnapshot | null | undefined,
  symbol: string | null | undefined,
): LiveSuccessPattern[] {
  if (!snapshot || !symbol) return [];
  const signatures = new Set(
    (snapshot.cases ?? [])
      .filter((item) => item.symbol === symbol)
      .map((item) => item.matched_success_pattern_signature)
      .filter((item): item is string => Boolean(item)),
  );
  return (snapshot.success_patterns ?? []).filter((item) => signatures.has(item.signature));
}

function bestCaseForSymbol(
  snapshot: OperationalSnapshot | null | undefined,
  symbol: string | null | undefined,
): CaseContract | null {
  if (!snapshot || !symbol) return null;
  return (
    snapshot.cases
      ?.filter((item) => item.symbol === symbol)
      .sort((left, right) => (right.confidence ?? 0) - (left.confidence ?? 0))[0] ?? null
  );
}

function linkedCaseForRecommendation(
  snapshot: OperationalSnapshot | null | undefined,
  recommendation: RecommendationContract,
): CaseContract | null {
  if (!snapshot) return null;
  return (
    snapshot.cases?.find(
      (item) =>
        item.id === recommendation.related_case_id ||
        item.setup_id === recommendation.related_setup_id,
    ) ?? null
  );
}

function ReasoningCard({
  caseItem,
  title = "Reasoning",
}: {
  caseItem: CaseContract;
  title?: string;
}) {
  return (
    <div className="eden-card">
      <div className="eden-card__title eden-workbench__stat-block">{title}</div>
      <Stat label="Driver" value={caseItem.driver_class ?? caseItem.tension_driver ?? "-"} />
      <Stat label="Lifecycle" value={caseItem.lifecycle_phase ?? "-"} />
      <Stat
        label="Peer Confirm"
        value={pct(caseItem.peer_confirmation_ratio)}
        cls={numCls(caseItem.peer_confirmation_ratio)}
      />
      <Stat
        label="Isolation"
        value={pct(caseItem.isolation_score)}
        cls={numCls(caseItem.isolation_score)}
      />
      <Stat
        label="Margin"
        value={pct(caseItem.competition_margin)}
        cls={numCls(caseItem.competition_margin)}
      />
      <Stat
        label="Peers"
        value={
          caseItem.peer_active_count != null || caseItem.peer_silent_count != null
            ? `${caseItem.peer_active_count ?? 0} active / ${caseItem.peer_silent_count ?? 0} silent`
            : "-"
        }
      />
      {caseItem.causal_narrative && (
        <div className="eden-note-item">
          {caseItem.causal_narrative}
        </div>
      )}
    </div>
  );
}

function TemporalBarsCard({ bars }: { bars: LiveTemporalBar[] }) {
  if (bars.length === 0) return null;
  return (
    <div className="eden-card">
      <div className="eden-card__title eden-workbench__stat-block">Temporal Bars</div>
      {bars.map((bar) => (
        <div key={`${bar.symbol}-${bar.horizon}-${bar.bucket_started_at}`} className="eden-note-item">
          <span className="mono">{bar.horizon}</span>{" "}
          <span className={numCls(bar.composite_close)}>{signed(bar.composite_close)}</span>{" "}
          <span className={numCls(bar.capital_flow_delta)}>flow {signed(bar.capital_flow_delta)}</span>{" "}
          <span className="text-dim">ev {bar.event_count} · p {bar.signal_persistence}</span>
        </div>
      ))}
    </div>
  );
}

function HorizonSupportCard({
  family,
  lineage,
}: {
  family: string | null | undefined;
  lineage: LiveLineageMetric[];
}) {
  if (!family && lineage.length === 0) return null;
  return (
    <div className="eden-card">
      <div className="eden-card__title eden-workbench__stat-block">Horizon Support</div>
      <Stat label="Family" value={family ?? "-"} />
      {lineage.length > 0 ? (
        lineage.map((item) => (
          <div key={`${item.template}-${item.horizon ?? "na"}`} className="eden-note-item">
            <span className="mono">{item.horizon ?? "n/a"}</span>{" "}
            <span className={numCls(item.hit_rate)}>{signed(item.hit_rate)}</span>{" "}
            <span className={numCls(item.mean_return)}>ret {signed(item.mean_return)}</span>{" "}
            <span className="text-dim">n {item.resolved}</span>
          </div>
        ))
      ) : (
        <div className="eden-note-item eden-note-item--danger">
          No positive 5m / 30m / session lineage support yet.
        </div>
      )}
    </div>
  );
}

function SuccessPatternMatchCard({
  patterns,
}: {
  patterns: LiveSuccessPattern[];
}) {
  if (patterns.length === 0) return null;
  return (
    <div className="eden-card">
      <div className="eden-card__title eden-workbench__stat-block">Matched Patterns</div>
      {patterns.map((item) => (
        <div key={item.signature} className="eden-note-item">
          <span className="mono">{item.signature}</span>{" "}
          <span className={numCls(item.mean_net_return)}>edge {signed(item.mean_net_return)}</span>{" "}
          <span className="text-dim">n {item.samples}</span>
        </div>
      ))}
    </div>
  );
}

export function SymbolDetail({
  sym,
  snapshot,
}: {
  sym: SymbolStateContract;
  snapshot?: OperationalSnapshot | null;
}) {
  const signal = sym.state.signal;
  const structure = sym.state.structure;
  const bars = barsForSymbol(snapshot, sym.symbol);
  const matchedPatterns = successPatternsForSymbol(snapshot, sym.symbol);
  const linkedCase = bestCaseForSymbol(snapshot, sym.symbol);
  return (
    <div className="eden-grid eden-grid--2">
      <div className="eden-card">
        <div className="eden-card__title eden-workbench__stat-block">Summary</div>
        <Stat label="Sector" value={sym.sector ?? "-"} />
        <Stat label="Structure" value={sym.summary.structure_action ?? "-"} />
        <Stat label="Status" value={sym.summary.structure_status ?? "-"} />
        <Stat
          label="Composite"
          value={signed(sym.summary.signal_composite)}
          cls={numCls(sym.summary.signal_composite)}
        />
        <Stat label="Depth" value={sym.summary.has_depth ? "yes" : "no"} />
      </div>
      <div className="eden-card">
        <div className="eden-card__title eden-workbench__stat-block">Signal</div>
        <Stat
          label="Composite"
          value={signed(signal?.composite)}
          cls={numCls(signal?.composite)}
        />
        <Stat
          label="Flow"
          value={signed(signal?.capital_flow_direction)}
          cls={numCls(signal?.capital_flow_direction)}
        />
        <Stat
          label="Momentum"
          value={signed(signal?.price_momentum)}
          cls={numCls(signal?.price_momentum)}
        />
        <Stat label="Structure" value={structure?.title ?? "-"} />
      </div>
      <TemporalBarsCard bars={bars} />
      {linkedCase && <ReasoningCard caseItem={linkedCase} title="Active Case Reasoning" />}
      <SuccessPatternMatchCard patterns={matchedPatterns} />
    </div>
  );
}

export function CaseDetail({
  caseItem,
  snapshot,
}: {
  caseItem: CaseContract;
  snapshot?: OperationalSnapshot | null;
}) {
  const bars = barsForSymbol(snapshot, caseItem.symbol);
  const family = caseItem.thesis_family;
  const lineage = lineageForFamily(snapshot, family);
  const matchedPattern = successPatternBySignature(
    snapshot,
    caseItem.matched_success_pattern_signature,
  );
  return (
    <div className="eden-grid eden-grid--2">
      <div className="eden-card">
        <div className="eden-card__title eden-workbench__stat-block">Case</div>
        <Stat label="Symbol" value={caseItem.symbol} />
        <Stat label="Action" value={caseItem.action} />
        <Stat label="Stage" value={caseItem.workflow_state} />
        <Stat label="Confidence" value={pct(caseItem.confidence)} />
        <Stat label="Invalidation" value={caseItem.invalidation_rule ?? "-"} />
      </div>
      <div className="eden-card">
        <div className="eden-card__title eden-workbench__stat-block">Policy</div>
        <Stat label="Primary" value={caseItem.policy_primary ?? caseItem.governance_reason_code ?? "-"} />
        <Stat label="Execution" value={caseItem.execution_policy ?? "-"} />
        <Stat label="Alpha Horizon" value={caseItem.alpha_horizon ?? "-"} />
        {caseItem.multi_horizon_gate_reason && (
          <div className="eden-note-item eden-note-item--danger">
            {caseItem.multi_horizon_gate_reason}
          </div>
        )}
        {caseItem.policy_reason && (
          <div className="eden-note-item">
            {caseItem.policy_reason}
          </div>
        )}
      </div>
      <ReasoningCard caseItem={caseItem} />
      <TemporalBarsCard bars={bars} />
      <HorizonSupportCard family={family} lineage={lineage} />
      <SuccessPatternMatchCard patterns={matchedPattern ? [matchedPattern] : []} />
    </div>
  );
}

export function RecommendationDetail({
  rec,
  snapshot,
}: {
  rec: RecommendationContract;
  snapshot?: OperationalSnapshot | null;
}) {
  const recommendation = rec.recommendation;
  const family = recommendation.thesis_family;
  const lineage = lineageForFamily(snapshot, family);
  const linkedCase = linkedCaseForRecommendation(snapshot, rec);
  const matchedPattern = successPatternBySignature(
    snapshot,
    recommendation.matched_success_pattern_signature ?? rec.summary.matched_success_pattern_signature,
  );
  return (
    <div className="eden-grid eden-grid--2">
      <div className="eden-card">
        <div className="eden-card__title eden-workbench__stat-block">Recommendation</div>
        <Stat label="Symbol" value={recommendation.symbol} />
        <Stat label="Action" value={recommendation.best_action} />
        <Stat label="Bias" value={recommendation.bias} />
        <Stat label="Confidence" value={pct(recommendation.confidence)} />
        <Stat label="Why" value={recommendation.why} />
      </div>
      <div className="eden-card">
        <div className="eden-card__title eden-workbench__stat-block">Decision Horizon</div>
        <Stat label="Alpha Horizon" value={recommendation.alpha_horizon ?? "-"} />
        <Stat label="Execution" value={recommendation.execution_policy ?? "-"} />
        <Stat label="Governance" value={recommendation.governance_reason_code ?? "-"} />
        <Stat
          label="Pattern"
          value={
            recommendation.matched_success_pattern_signature ??
            rec.summary.matched_success_pattern_signature ??
            "-"
          }
        />
        <Stat
          label="Expected Alpha"
          value={signed(recommendation.expected_net_alpha)}
          cls={numCls(recommendation.expected_net_alpha)}
        />
        {recommendation.governance_reason && (
          <div className="eden-note-item">
            {recommendation.governance_reason}
          </div>
        )}
      </div>
      {linkedCase && <ReasoningCard caseItem={linkedCase} title="Linked Case Reasoning" />}
      <HorizonSupportCard family={family} lineage={lineage} />
      <SuccessPatternMatchCard patterns={matchedPattern ? [matchedPattern] : []} />
    </div>
  );
}

export function MacroEventDetail({ eventItem }: { eventItem: MacroEventContract }) {
  const summary = eventItem.summary;
  return (
    <div className="eden-card">
      <div className="eden-card__title eden-workbench__stat-block">Macro Event</div>
      <Stat label="Headline" value={summary?.headline ?? eventItem.event.headline} />
      <Stat label="Type" value={summary?.event_type ?? eventItem.event.event_type} />
      <Stat
        label="Confidence"
        value={pct(summary?.confidence ?? eventItem.event.confidence)}
      />
      <Stat
        label="Scope"
        value={summary?.primary_scope ?? eventItem.event.impact.primary_scope}
      />
    </div>
  );
}

export function WorkflowDetail({ workflow }: { workflow: WorkflowContract }) {
  const caseCount =
    workflow.case_ids?.length ??
    workflow.case_refs?.length ??
    workflow.relationships?.cases?.length ??
    0;
  const recommendationCount =
    workflow.recommendation_ids?.length ??
    workflow.recommendation_refs?.length ??
    workflow.relationships?.recommendations?.length ??
    0;

  return (
    <div className="eden-card">
      <div className="eden-card__title eden-workbench__stat-block">Workflow</div>
      <Stat label="Stage" value={workflow.stage} />
      <Stat label="Policy" value={workflow.execution_policy ?? "-"} />
      <Stat label="Owner" value={workflow.owner ?? "-"} />
      <Stat label="Cases" value={String(caseCount)} />
      <Stat label="Recommendations" value={String(recommendationCount)} />
    </div>
  );
}
