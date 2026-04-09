import { useShellStore } from "@/state/shell-store";
import type { OperationalSnapshot, RecommendationContract } from "@/lib/api/types";
import { ToneBadge } from "@/features/workbench/primitives";
import {
  actionCls,
  BIAS,
  compareRecommendationsByOperationalPriority,
  LENS,
  numCls,
  pct,
  signed,
  supportedFamilyKeys,
} from "./format";

export function ActionsPanel({ snap }: { snap: OperationalSnapshot }) {
  const recs = snap.recommendations;
  if (recs.length === 0) return null;
  const supportedFamilies = supportedFamilyKeys(snap.lineage);

  const topRecs = [...recs]
    .sort((a, b) => compareRecommendationsByOperationalPriority(a, b, supportedFamilies))
    .slice(0, 6);

  return (
    <div className="eden-panel-block">
      <div className="eden-inline-row eden-inline-row--tight eden-panel-heading">
        <span className="eden-card__title">Actions</span>
        <span className="mono text-muted eden-section-meta">
          {recs.length} proposals
        </span>
      </div>
      <div className="eden-grid eden-grid--3">
        {topRecs.map((r) => (
          <ActionCard key={r.id} rec={r} />
        ))}
      </div>
    </div>
  );
}

export function ActionCard({ rec }: { rec: RecommendationContract }) {
  const openObject = useShellStore((s) => s.openObject);
  const r = rec.recommendation;
  const action = r.best_action || r.action || "watch";
  const lens = LENS[r.primary_lens ?? ""] ?? r.primary_lens ?? "";
  const decisive = r.decision_attribution?.decisive_factors ?? [];
  const thesis = decisive.length > 0 ? decisive[0] : r.title || "";
  const watchNext = r.watch_next ?? [];
  const doNot = r.do_not ?? [];
  const matchedPattern =
    r.matched_success_pattern_signature ?? rec.summary.matched_success_pattern_signature ?? null;

  return (
    <div className="eden-proposal" onClick={() => openObject({ kind: "recommendation", id: rec.id, label: r.title ?? r.symbol })}>
      <div className="eden-inline-row eden-inline-row--spread">
        <div className="eden-inline-row eden-inline-row--baseline eden-inline-row--tight">
          <span className="mono eden-proposal__symbol-strong">{r.symbol}</span>
          {r.price_at_decision != null && (
            <span className="mono text-muted eden-section-meta">
              ${Number(r.price_at_decision).toFixed(2)}
            </span>
          )}
        </div>
        <span className={`eden-proposal__action ${actionCls(action)}`}>{action}</span>
      </div>

      <div className="eden-inline-row eden-inline-row--tight eden-proposal__meta-row">
        {lens && (
          <ToneBadge tone="var(--eden-cyan)" className="eden-tone-badge--compact">
            {lens}
          </ToneBadge>
        )}
        <span className={`eden-proposal__bias eden-proposal__bias--${r.bias}`}>
          {BIAS[r.bias] || r.bias}
        </span>
        <span className="mono text-muted eden-section-meta">
          {pct(r.confidence)}
        </span>
      </div>

      {thesis && <div className="eden-body-copy eden-proposal__thesis">{thesis}</div>}

      {matchedPattern && (
        <div className="eden-note-list eden-note-list--compact">
          <div className="eden-note-item">
            pattern {matchedPattern}
          </div>
        </div>
      )}

      <div className="eden-metric-strip">
        <MetricCell label="ALPHA" value={pct(r.expected_net_alpha)} cls={numCls(r.expected_net_alpha)} />
        <MetricCell label="FOLLOW" value={signed(r.follow_expectancy)} cls={numCls(r.follow_expectancy)} />
        <MetricCell label="FADE" value={signed(r.fade_expectancy)} cls={numCls(r.fade_expectancy)} />
      </div>

      {watchNext.length > 0 && (
        <div className="eden-note-list">
          {watchNext.slice(0, 2).map((w, i) => (
            <div key={i} className="eden-note-item">
              {w}
            </div>
          ))}
        </div>
      )}

      {doNot.length > 0 && (
        <div className="eden-note-list eden-note-list--compact">
          {doNot.slice(0, 1).map((d, i) => (
            <div key={i} className="eden-note-item eden-note-item--danger">
              {d}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function MetricCell({ label, value, cls }: { label: string; value: string; cls: string }) {
  if (value === "-") return null;
  return (
    <div className="eden-metric-strip__cell">
      <div className="mono text-muted eden-metric-strip__label">{label}</div>
      <div className={`mono eden-metric-strip__value ${cls}`}>{value}</div>
    </div>
  );
}
