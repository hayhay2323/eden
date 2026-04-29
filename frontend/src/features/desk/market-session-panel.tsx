import type { CSSProperties } from "react";

import { useShellStore } from "@/state/shell-store";
import type { OperationalSnapshot } from "@/lib/api/types";
import { ToneBadge } from "@/features/workbench/primitives";
import { BIAS, numCls, pct, signed } from "./format";

export function MarketSessionPanel({ snap }: { snap: OperationalSnapshot }) {
  const ms = snap.market_session;
  const regime = ms.market_regime;
  const stress = ms.stress;
  const mr = snap.market_recommendation;
  const openObject = useShellStore((s) => s.openObject);

  const regimeBias = regime.bias;
  const regimeColor =
    regimeBias === "risk_on" ? "var(--eden-green)"
      : regimeBias === "risk_off" ? "var(--eden-red)"
      : "var(--eden-cyan)";

  const breadthUp = Number(regime.breadth_up);
  const breadthDown = Number(regime.breadth_down);
  const breadthSkew = breadthUp - breadthDown;
  const temporalBars = snap.temporal_bars ?? [];
  const temporalSummary = temporalBars.slice(0, 4);

  return (
    <div className="eden-card eden-panel-block eden-market-session" style={{ "--eden-tone": regimeColor } as CSSProperties}>
      <div className="eden-inline-row eden-inline-row--tight eden-market-session__header">
        <span className="mono eden-market-session__regime">
          {regimeBias.toUpperCase()}
        </span>
        <span className="mono text-dim eden-section-meta">
          conf {pct(regime.confidence)}
        </span>
        {ms.should_speak && (
          <ToneBadge tone="var(--eden-cyan)">
            ACTIVE
          </ToneBadge>
        )}
        <span className="mono text-muted eden-market-session__tick">
          tick {snap.source_tick}
        </span>
      </div>

      {ms.wake_headline && (
        <div className="eden-market-session__headline">
          {ms.wake_headline}
        </div>
      )}

      {ms.market_summary && (
        <div className="eden-body-copy eden-market-session__summary">
          {ms.market_summary}
        </div>
      )}

      {mr && (
        <div className="eden-market-session__summary-card">
          <span className={`eden-proposal__bias eden-proposal__bias--${mr.bias}`}>
            {BIAS[mr.bias] || mr.bias}
          </span>
          <span className="eden-market-session__summary-copy">{mr.summary}</span>
          {mr.why_not_single_name && (
            <div className="text-dim eden-market-session__subcopy">
              {mr.why_not_single_name}
            </div>
          )}
        </div>
      )}

      <div className="eden-metric-row">
        <Metric label="BREADTH" value={breadthUp > breadthDown ? `${pct(breadthUp)} ↑` : `${pct(breadthDown)} ↓`} cls={numCls(breadthSkew)} />
        <Metric label="AVG RETURN" value={pct(regime.average_return)} cls={numCls(regime.average_return)} />
        <Metric label="STRESS" value={Number(stress.composite_stress ?? 0).toFixed(2)} />
        <Metric label="THREADS" value={String(ms.active_thread_count)} />
      </div>

      {ms.wake_reasons.length > 0 && (
        <div className="eden-note-list">
          {ms.wake_reasons.slice(0, 4).map((reason, i) => (
            <div key={i} className="eden-note-item">
              {reason}
            </div>
          ))}
        </div>
      )}

      {ms.focus_symbols.length > 0 && (
        <div className="eden-chip-list">
          {ms.focus_symbols.map((s) => (
            <button
              key={s}
              className="mono eden-chip eden-chip--interactive"
              onClick={() => openObject({ kind: "symbol_state", id: s, label: s })}
            >
              {s}
            </button>
          ))}
        </div>
      )}

      {temporalSummary.length > 0 && (
        <div className="eden-note-list eden-note-list--compact">
          {temporalSummary.map((bar, index) => (
            <div key={`${bar.symbol}-${bar.horizon}-${index}`} className="eden-note-item">
              <span className="mono eden-market-session__bar-symbol">{bar.symbol}</span>
              <span className="mono text-muted eden-section-meta">{bar.horizon}</span>
              <span className={numCls(bar.composite_close)}>
                {signed(bar.composite_close)}
              </span>
              <span className={numCls(bar.capital_flow_delta)}>
                flow {signed(bar.capital_flow_delta)}
              </span>
              <span className="text-dim">
                ev {bar.event_count} · p {bar.signal_persistence}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function Metric({
  label,
  value,
  cls,
}: {
  label: string;
  value: string;
  cls?: string;
}) {
  return (
    <div className="eden-metric">
      <div className="mono text-muted eden-metric__label">
        {label}
      </div>
      <div className={`mono eden-metric__value ${cls ?? ""}`}>
        {value}
      </div>
    </div>
  );
}
