import { useShellStore } from "@/state/shell-store";
import type {
  BackwardInvestigation,
  LiveLineageMetric,
  LiveSuccessPattern,
  OperationalSnapshot,
} from "@/lib/api/types";
import { numCls, signed } from "./format";

export function ReasoningPanel({ snap }: { snap: OperationalSnapshot }) {
  const investigations = snap.sidecars.backward_investigations ?? [];
  const topInvestigations = investigations.filter((b) => b.leading_cause).slice(0, 3);
  const lineage = snap.lineage ?? [];
  const successPatterns = snap.success_patterns ?? [];

  if (topInvestigations.length === 0 && lineage.length === 0 && successPatterns.length === 0) {
    return null;
  }

  return (
    <div className="eden-panel-block">
      <div className="eden-card__title eden-panel-heading">
        Evidence
      </div>
      {topInvestigations.length > 0 && (
        <div className="eden-grid eden-grid--3">
          {topInvestigations.map((b, i) => (
            <EvidenceCard key={i} investigation={b} />
          ))}
        </div>
      )}
      {lineage.length > 0 && (
        <div className="eden-card eden-panel-block eden-lineage-card">
          <div className="eden-card__header">
            <span className="eden-card__title">Lineage Horizons</span>
            <span className="eden-card__badge eden-card__badge--cyan">{lineage.length}</span>
          </div>
          <div className="eden-grid eden-grid--3">
            {lineage.slice(0, 6).map((item, index) => (
              <LineageMetricCard key={`${item.template}-${item.horizon ?? index}`} item={item} />
            ))}
          </div>
        </div>
      )}
      {successPatterns.length > 0 && (
        <div className="eden-card eden-panel-block eden-lineage-card">
          <div className="eden-card__header">
            <span className="eden-card__title">Success Patterns</span>
            <span className="eden-card__badge eden-card__badge--cyan">{successPatterns.length}</span>
          </div>
          <div className="eden-grid eden-grid--3">
            {successPatterns.slice(0, 6).map((item, index) => (
              <SuccessPatternCard key={`${item.signature}-${index}`} item={item} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export function EvidenceCard({ investigation }: { investigation: BackwardInvestigation }) {
  const cause = investigation.leading_cause;
  if (!cause) return null;

  const symbol = investigation.leaf_scope?.Symbol;
  const openObject = useShellStore((s) => s.openObject);

  return (
    <div
      className={`eden-card${symbol ? " eden-clickable" : ""}`}
      onClick={() => symbol && openObject({ kind: "symbol_state", id: symbol, label: symbol })}
    >
      <div className="eden-inline-row eden-inline-row--tight eden-evidence-card__header">
        {symbol && <span className="mono eden-evidence-card__symbol">{symbol}</span>}
        <span className="text-dim eden-evidence-card__label">{investigation.leaf_label}</span>
      </div>

      <div className="eden-body-copy eden-evidence-card__explanation">
        {cause.explanation}
      </div>

      <div className="eden-inline-row eden-inline-row--tight eden-evidence-card__meta">
        <span className="mono text-muted eden-section-meta">
          conviction {Number(cause.net_conviction).toFixed(2)}
        </span>
        {investigation.contest_state && (
          <span className="mono text-muted eden-section-meta">
            {investigation.contest_state}
          </span>
        )}
      </div>

      {cause.falsifier && (
        <div className="eden-note-item eden-note-item--danger eden-evidence-card__falsifier">
          {cause.falsifier}
        </div>
      )}

      {(cause.supporting_evidence ?? []).slice(0, 3).map((e, i) => (
        <div key={i} className="eden-evidence-card__row">
          <span className={`mono eden-evidence-card__weight ${numCls(e.weight)}`}>
            {signed(e.weight)}
          </span>
          <span className="text-muted">[{e.channel}]</span>
          <span className="text-dim">{e.statement}</span>
        </div>
      ))}
    </div>
  );
}

function LineageMetricCard({ item }: { item: LiveLineageMetric }) {
  return (
    <div className="eden-card">
      <div className="eden-inline-row eden-inline-row--tight eden-evidence-card__header">
        <span className="mono eden-evidence-card__symbol">{item.horizon ?? "n/a"}</span>
        <span className="text-dim eden-evidence-card__label">{item.template}</span>
      </div>
      <div className="eden-metric-strip">
        <div className="eden-metric-strip__cell">
          <div className="mono text-muted eden-metric-strip__label">HIT</div>
          <div className={`mono eden-metric-strip__value ${numCls(item.hit_rate)}`}>
            {signed(item.hit_rate)}
          </div>
        </div>
        <div className="eden-metric-strip__cell">
          <div className="mono text-muted eden-metric-strip__label">MEAN</div>
          <div className={`mono eden-metric-strip__value ${numCls(item.mean_return)}`}>
            {signed(item.mean_return)}
          </div>
        </div>
        <div className="eden-metric-strip__cell">
          <div className="mono text-muted eden-metric-strip__label">N</div>
          <div className="mono eden-metric-strip__value">{item.resolved}</div>
        </div>
      </div>
    </div>
  );
}

function SuccessPatternCard({ item }: { item: LiveSuccessPattern }) {
  return (
    <div className="eden-card">
      <div className="eden-inline-row eden-inline-row--tight eden-evidence-card__header">
        <span className="mono eden-evidence-card__symbol">
          {item.center_kind ? `${item.center_kind}:${item.role ?? "n/a"}` : "convergence"}
        </span>
        <span className="text-dim eden-evidence-card__label">{item.family}</span>
      </div>
      <div className="eden-body-copy eden-evidence-card__explanation">
        {item.signature}
      </div>
      {item.dominant_channels.length > 0 && (
        <div className="eden-inline-row eden-inline-row--tight eden-evidence-card__meta">
          <span className="text-muted">{item.dominant_channels.join(" · ")}</span>
        </div>
      )}
      <div className="eden-metric-strip">
        <div className="eden-metric-strip__cell">
          <div className="mono text-muted eden-metric-strip__label">EDGE</div>
          <div className={`mono eden-metric-strip__value ${numCls(item.mean_net_return)}`}>
            {signed(item.mean_net_return)}
          </div>
        </div>
        <div className="eden-metric-strip__cell">
          <div className="mono text-muted eden-metric-strip__label">STR</div>
          <div className={`mono eden-metric-strip__value ${numCls(item.mean_strength)}`}>
            {signed(item.mean_strength)}
          </div>
        </div>
        <div className="eden-metric-strip__cell">
          <div className="mono text-muted eden-metric-strip__label">N</div>
          <div className="mono eden-metric-strip__value">{item.samples}</div>
        </div>
      </div>
    </div>
  );
}
