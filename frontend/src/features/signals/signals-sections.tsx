import { useShellStore } from "@/state/shell-store";
import type { AgentSectorFlow, MacroEventContract } from "@/lib/api/types";
import type { SignalsSectorSupport, SignalsSymbolRow } from "./use-signals-view";

function fmtSigned(v: number | null | undefined): string {
  if (v == null) return "-";
  const n = Number(v);
  return (n >= 0 ? "+" : "") + n.toFixed(4);
}

function numClass(v: number | null | undefined): string {
  if (v == null) return "";
  return Number(v) > 0 ? "text-positive" : Number(v) < 0 ? "text-negative" : "";
}

export function SectorFlowBoard({
  flows,
  sectorSupport,
}: {
  flows: AgentSectorFlow[];
  sectorSupport: Map<string, SignalsSectorSupport>;
}) {
  const sorted = [...flows].sort(
    (a, b) => {
      const left = sectorSupport.get(a.sector)?.supported ?? 0;
      const right = sectorSupport.get(b.sector)?.supported ?? 0;
      if (right !== left) return right - left;
      return Math.abs(b.average_capital_flow) - Math.abs(a.average_capital_flow);
    },
  );

  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Sector Flows</span>
        <span className="eden-card__badge eden-card__badge--amber">{flows.length}</span>
      </div>
      {sorted.length === 0 ? (
        <div className="eden-empty">No sector flows</div>
      ) : (
        sorted.map((f) => (
          <div className="eden-stat" key={f.sector}>
            <span className="eden-stat__label eden-stat__label--wide">{f.sector}</span>
            <span className={`eden-stat__value ${numClass(f.average_capital_flow)}`}>
              {fmtSigned(f.average_capital_flow)}
            </span>
            <span className="text-dim mono eden-section-meta">
              {f.member_count} stocks
              {(() => {
                const stats = sectorSupport.get(f.sector);
                return stats
                  ? ` · ${stats.supported} supported / ${stats.gated} gated`
                  : "";
              })()}
              {f.leaders.length > 0 ? ` · leaders: ${f.leaders.slice(0, 2).join(", ")}` : ""}
              {f.exceptions.length > 0 ? ` · ex: ${f.exceptions.slice(0, 2).join(", ")}` : ""}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

export function MacroEventBoard({
  events,
  symbolRows,
}: {
  events: MacroEventContract[];
  symbolRows: SignalsSymbolRow[];
}) {
  const openObject = useShellStore((s) => s.openObject);
  const supportedSymbols = new Set(
    symbolRows.filter((row) => row.supportState === "supported").map((row) => row.sym.symbol),
  );
  const gatedSymbols = new Set(
    symbolRows.filter((row) => row.supportState === "gated").map((row) => row.sym.symbol),
  );

  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Macro Events</span>
        <span className="eden-card__badge eden-card__badge--blue">{events.length}</span>
      </div>
      {events.length === 0 ? (
        <div className="eden-empty">No macro events</div>
      ) : (
        events.slice(0, 8).map((event) => {
          const headline = event.summary?.headline ?? event.event.headline;
          const eventType = event.summary?.event_type ?? event.event.event_type;
          const relatedSymbols = event.relationships.symbols
            .map((ref) => ref.label ?? ref.id)
            .filter((value): value is string => Boolean(value));
          const supportedCount = relatedSymbols.filter((symbol) => supportedSymbols.has(symbol)).length;
          const gatedCount = relatedSymbols.filter((symbol) => gatedSymbols.has(symbol)).length;
          return (
            <div
              key={event.id}
              className="eden-feed-item eden-clickable"
              onClick={() =>
                openObject({
                  kind: "macro_event",
                  id: event.id,
                  label: headline,
                })
              }
            >
              <span className="eden-feed-item__kind">{eventType}</span>
              <span className="eden-feed-item__label">
                {headline}
                {(supportedCount > 0 || gatedCount > 0) && (
                  <span className="eden-focus-row__subtitle">
                    {supportedCount} supported · {gatedCount} gated related symbols
                  </span>
                )}
              </span>
            </div>
          );
        })
      )}
    </div>
  );
}

export function SymbolBoard({
  symbols,
}: {
  symbols: SignalsSymbolRow[];
}) {
  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Symbols</span>
        <span className="eden-card__badge eden-card__badge--blue">{symbols.length}</span>
      </div>

      <div className="eden-case-row eden-case-row__header eden-case-row--signals">
        <span>SYMBOL</span>
        <span>SUPPORT</span>
        <span>COMP</span>
        <span>FLOW</span>
        <span>5M</span>
        <span>30M</span>
        <span>SECTOR</span>
        <span>STATUS</span>
      </div>

      {symbols.length === 0 ? (
        <div className="eden-empty">No symbols match the current filter</div>
      ) : (
        symbols.map((row) => <SymbolRow key={row.sym.symbol} row={row} />)
      )}
    </div>
  );
}

function SymbolRow({ row }: { row: SignalsSymbolRow }) {
  const openObject = useShellStore((s) => s.openObject);
  const { sym } = row;
  const signal = sym.state.signal;
  const bar5 = row.bars.find((item) => item.horizon === "5m");
  const bar30 = row.bars.find((item) => item.horizon === "30m");
  const supportLabel =
    row.supportState === "supported"
      ? "supported"
      : row.supportState === "gated"
        ? "gated"
        : "-";

  return (
    <div
      className="eden-case-row eden-case-row--signals"
      onClick={() => openObject({ kind: "symbol_state", id: sym.symbol, label: sym.symbol })}
    >
      <span className="eden-case-row__sym">{sym.symbol}</span>
      <span className={`mono eden-case-row__meta eden-signal-support eden-signal-support--${row.supportState}`}>
        {supportLabel}
      </span>
      <span className={`mono eden-case-row__metric ${numClass(signal?.composite)}`}>
        {fmtSigned(signal?.composite)}
      </span>
      <span className={`mono eden-case-row__metric ${numClass(signal?.capital_flow_direction)}`}>
        {fmtSigned(signal?.capital_flow_direction)}
      </span>
      <span className={`mono eden-case-row__metric ${numClass(bar5?.composite_close)}`}>
        {fmtSigned(bar5?.composite_close)}
      </span>
      <span className={`mono eden-case-row__metric ${numClass(bar30?.composite_close)}`}>
        {fmtSigned(bar30?.composite_close)}
      </span>
      <span className="mono text-dim eden-case-row__meta">{sym.sector || ""}</span>
      <span className="text-dim eden-case-row__meta eden-text-truncate">
        {row.supportReason || sym.summary.structure_action || ""}
      </span>
    </div>
  );
}
