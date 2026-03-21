"use client";
import { useEffect, useState, useCallback, useMemo } from "react";

const API_BASE = process.env.NEXT_PUBLIC_EDEN_API_URL || "http://localhost:8787";
const API_KEY = process.env.NEXT_PUBLIC_EDEN_API_KEY || "";

type Market = "hk" | "us";

const HK_SECTORS: Record<string, string> = {
  "981.HK": "半導體", "1347.HK": "半導體",
  "700.HK": "科技", "9988.HK": "科技", "9618.HK": "科技", "3690.HK": "科技",
  "9999.HK": "科技", "1024.HK": "科技", "9888.HK": "科技", "1810.HK": "科技",
  "941.HK": "電訊", "728.HK": "電訊",
  "1211.HK": "汽車", "2015.HK": "汽車", "9866.HK": "汽車",
  "1398.HK": "金融", "3988.HK": "金融", "939.HK": "金融",
  "2318.HK": "金融", "1299.HK": "金融", "388.HK": "金融",
  "883.HK": "能源", "857.HK": "能源", "386.HK": "能源", "2688.HK": "能源",
  "1928.HK": "消費", "9961.HK": "消費", "6060.HK": "消費",
  "268.HK": "醫藥", "2269.HK": "醫藥",
};
const US_SECTORS: Record<string, string> = {
  "AAPL.US": "科技", "MSFT.US": "科技", "GOOGL.US": "科技", "META.US": "科技",
  "AMZN.US": "科技", "NVDA.US": "半導體", "AMD.US": "半導體", "INTC.US": "半導體",
  "TSM.US": "半導體", "AVGO.US": "半導體", "QCOM.US": "半導體",
  "BABA.US": "中概", "JD.US": "中概", "PDD.US": "中概", "BIDU.US": "中概",
  "NIO.US": "中概", "LI.US": "中概", "XPEV.US": "中概",
  "JPM.US": "金融", "BAC.US": "金融", "GS.US": "金融",
  "XOM.US": "能源", "CVX.US": "能源", "TSLA.US": "汽車",
};

interface LiveSnapshot {
  tick: number;
  timestamp: string;
  market_regime?: { bias: string; confidence: string; breadth_up: string; breadth_down: string; average_return: string };
  stress?: { sector_synchrony: string; pressure_consensus: string; conflict_intensity_mean: string; composite_stress: string; market_temperature_stress: string };
  pressures?: { symbol: string; net_pressure?: string; pressure_delta: string; pressure_duration: number; accelerating: boolean; buy_inst_count?: number; sell_inst_count?: number; capital_flow_pressure?: string; volume_intensity?: string; momentum?: string }[];
  pair_trades?: { institution: string; buy_symbols: string[]; sell_symbols: string[]; net_direction: string }[];
  exoduses?: { institution: string; prev_stock_count: number; curr_stock_count: number; dropped_count: number }[];
  hidden_links?: { symbol_a: string; symbol_b: string; sector_a: string | null; sector_b: string | null; jaccard: string; shared_institutions: number }[];
  conflicts?: { inst_a: string; inst_b: string; jaccard_overlap: string; direction_a: string; direction_b: string; conflict_age: number; intensity_delta: string }[];
  tactical_cases?: { title: string; action: string; confidence: string; confidence_gap: string; heuristic_edge: string }[];
  hypothesis_tracks?: { title: string; status: string; age_ticks: number; confidence: string }[];
  scorecard?: { signal_type: string; total: number; resolved: number; hits: number; hit_rate: string; mean_return: string }[];
  top_signals?: { symbol: string; composite: string; institutional_alignment: string; sector_coherence: string | null; cross_stock_correlation: string; mark_price: string | null }[];
  events?: { kind: string; magnitude: string; summary: string }[];
  convergence_scores?: { symbol: string; dimension_composite: string; cross_stock_correlation: string; sector_coherence: string; cross_market_propagation: string }[];
  cross_market_signals?: { us_symbol: string; hk_symbol: string; hk_composite: string; propagation_confidence: string; time_since_hk_close_minutes: number }[];
  observation_count?: number;
  hypothesis_count?: number;
  lineage?: { template: string; total: number; resolved: number; hits: number; hit_rate: string; mean_return: string }[];
  // US new modules (same key names as backend JSON)
  // note: `pressures` is reused — HK has buy/sell_inst_count, US has capital_flow_pressure
  // `stress` is reused — HK has sector_synchrony, US has momentum_consensus
  rotations?: { sector_a: string; sector_b: string; spread: string; spread_delta: string; widening: boolean }[];
  clusters?: { members: string[]; directional_alignment: string; stability: string; age: number }[];
  cross_market_anomalies?: { us_symbol: string; hk_symbol: string; expected_direction: string; actual_direction: string; divergence: string }[];
  backward_chains?: { symbol: string; conclusion: string; primary_driver: string; confidence: string; evidence: { source: string; description: string; weight: string; direction: string }[] }[];
  workflows?: { symbol: string; stage: string; confidence_at_entry: string; current_confidence: string; pnl: string | null; entry_tick: number }[];
  active_positions?: number;
  causal_leaders?: { symbol: string; current_leader: string; leader_streak: number; flips: number }[];
}

function pct(v: string | number): string {
  const n = typeof v === "string" ? parseFloat(v) : v;
  if (isNaN(n)) return String(v);
  return `${(n * 100).toFixed(1)}%`;
}
function pctColor(v: string | number): string {
  const n = typeof v === "string" ? parseFloat(v) : v;
  if (isNaN(n) || n === 0) return "text-[var(--text-muted)]";
  return n > 0 ? "text-[var(--accent-green)]" : "text-[var(--accent-red)]";
}

export default function Dashboard() {
  const [data, setData] = useState<LiveSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [market, setMarket] = useState<Market>("us");
  const [sectorFilter, setSectorFilter] = useState<string | null>(null);
  const [actionTaken, setActionTaken] = useState<Record<string, string>>({});

  const fetchLive = useCallback(async () => {
    try {
      const res = await fetch(`${API_BASE}${market === "us" ? "/api/us/live" : "/api/live"}`, {
        headers: { Authorization: `Bearer ${API_KEY}` }, cache: "no-store",
      });
      if (!res.ok) throw new Error(`${res.status}`);
      setData(await res.json());
      setError(null);
    } catch (e: unknown) { setError(e instanceof Error ? e.message : "failed"); }
  }, [market]);

  useEffect(() => {
    setData(null); setSelected(null); setSectorFilter(null);
    fetchLive();
    const iv = setInterval(fetchLive, 2000);
    return () => clearInterval(iv);
  }, [fetchLive]);

  const selectStock = (s: string) => setSelected(s);
  const sectorMap = market === "hk" ? HK_SECTORS : US_SECTORS;

  const selectedSignal = selected
    ? (market === "hk"
      ? data?.top_signals?.find(s => s.symbol === selected)
      : (() => { const c = data?.convergence_scores?.find(c => c.symbol === selected); return c ? { symbol: c.symbol, composite: c.dimension_composite, institutional_alignment: "0", sector_coherence: c.sector_coherence, cross_stock_correlation: c.cross_stock_correlation, mark_price: null } : undefined; })())
    : undefined;
  const selectedPressure = selected ? data?.pressures?.find(p => p.symbol === selected) : undefined;

  const bubbles = useMemo(() => {
    type S = { symbol: string; composite: string };
    const sigs: S[] = market === "hk"
      ? (data?.top_signals?.slice(0, 18) ?? [])
      : (data?.convergence_scores?.slice(0, 18)?.map(c => ({ symbol: c.symbol, composite: c.dimension_composite })) ?? []);
    if (!sigs.length) return [];
    const phi = 2.39996323;
    return sigs.map((sig, i) => {
      const comp = parseFloat(sig.composite) || 0, absComp = Math.abs(comp);
      const pr = data?.pressures?.find(p => p.symbol === sig.symbol);
      const ic = pr ? (pr.buy_inst_count ?? 0) + (pr.sell_inst_count ?? 0) || 2 : 2;
      const r = Math.max(16, 14 + ic * 3 + absComp * 30);
      const theta = i * phi, dist = Math.sqrt(i + 0.5) * 54;
      return {
        symbol: sig.symbol,
        cx: Math.max(r + 10, Math.min(790 - r, 400 + dist * Math.cos(theta) + comp * 90)),
        cy: Math.max(r + 50, Math.min(560 - r, 300 + dist * Math.sin(theta))),
        r, comp, absComp, accelerating: pr?.accelerating ?? false, instCount: ic,
      };
    });
  }, [data?.top_signals, data?.convergence_scores, data?.pressures, market]);

  const filtered = sectorFilter
    ? bubbles.filter(b => (market === "hk" ? HK_SECTORS : US_SECTORS)[b.symbol] === sectorFilter)
    : bubbles;

  const sectors = useMemo(() => {
    const m = market === "hk" ? HK_SECTORS : US_SECTORS, s = new Set<string>();
    bubbles.forEach(b => { const sec = m[b.symbol]; if (sec) s.add(sec); });
    return Array.from(s);
  }, [bubbles, market]);

  const curAction = selected ? actionTaken[selected] : undefined;

  return (
    <div className="h-full flex">
      {/* ── 側欄 ── */}
      <div className="w-12 bg-[var(--bg-sidebar)] flex flex-col items-center py-4 gap-2 shrink-0 border-r border-[var(--border-gray)]">
        <span className="font-display text-lg font-bold text-[var(--accent-green)]">E</span>
        <div className="mt-auto flex flex-col items-center gap-2">
          <div className="w-7 h-px bg-[var(--border-gray)]" />
          <PinBtn label="981" color="red" onClick={() => selectStock("981.HK")} active={selected === "981.HK"} />
          <PinBtn label="6060" color="green" onClick={() => selectStock("6060.HK")} active={selected === "6060.HK"} />
        </div>
      </div>

      {/* ── 主區 ── */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* 頂欄 */}
        <div className="h-9 bg-[var(--bg-sidebar)] border-b border-[var(--border-gray)] flex items-center px-4 justify-between shrink-0">
          <div className="flex items-center gap-3">
            <span className="font-mono-eden text-[12px] font-semibold text-[var(--accent-green)] tracking-wider">EDEN</span>
            <div className="w-px h-4 bg-[var(--border-gray)]" />
            <div className="flex">
              <button onClick={() => setMarket("hk")} className={`font-mono-eden text-[10px] px-2.5 py-0.5 transition-all ${market === "hk" ? "bg-[var(--accent-green-10)] text-[var(--accent-green)] font-bold border border-[var(--accent-green)]/40" : "text-[var(--text-muted)] border border-[var(--border-gray)] hover:text-[var(--text-secondary)]"}`}>港股</button>
              <button onClick={() => setMarket("us")} className={`font-mono-eden text-[10px] px-2.5 py-0.5 transition-all ${market === "us" ? "bg-[var(--accent-green-10)] text-[var(--accent-green)] font-bold border border-[var(--accent-green)]/40" : "text-[var(--text-muted)] border border-[var(--border-gray)] hover:text-[var(--text-secondary)]"}`}>美股</button>
            </div>
            <div className="w-px h-4 bg-[var(--border-gray)]" />
            <span className="font-mono-eden text-[10px] text-[var(--text-muted)]">
              #{data?.tick ?? "—"} {data?.timestamp ? new Date(data.timestamp).toLocaleTimeString("zh-HK") : ""}
            </span>
          </div>
          <div className="flex items-center gap-2.5">
            {data?.stress && <Badge label={`壓力 ${pct(data.stress.composite_stress)}`} color="orange" />}
            {data?.stress?.sector_synchrony && <Badge label={`同步 ${pct(data.stress.sector_synchrony)}`} color="green" />}
            {market === "us" && data?.stress && "momentum_consensus" in data.stress && <Badge label={`共識 ${pct((data.stress as Record<string,string>).momentum_consensus)}`} color="green" />}
            <div className="w-1.5 h-1.5 rounded-full bg-[var(--accent-green)] animate-pulse" />
            <span className="font-mono-eden text-[8px] font-bold text-[var(--accent-green)]">即時</span>
          </div>
        </div>

        {/* 內容 */}
        <div className="flex-1 flex min-h-0">
          {/* ── 左面板：信號列表 ── */}
          <div className="w-72 bg-[var(--bg-sidebar)] border-r border-[var(--border-gray)] flex flex-col overflow-y-auto shrink-0">
            <div className="p-2.5 flex flex-col gap-1.5">
              {market === "hk" ? (<>
                <Lbl text="// 聰明資金" />
                {data?.pressures?.slice(0, 6).map(p => (
                  <Row key={p.symbol} active={selected === p.symbol} onClick={() => selectStock(p.symbol)}>
                    <span className={`font-mono-eden text-[11px] font-semibold ${selected === p.symbol ? "text-[var(--accent-green)]" : ""}`}>{p.symbol}</span>
                    <span className={`font-mono-eden text-[11px] font-bold ${pctColor(p.net_pressure ?? "0")}`}>
                      {parseFloat(p.net_pressure ?? "0") > 0 ? "▲" : "▼"}{pct(p.net_pressure ?? "0")}
                    </span>
                    <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">{p.pressure_duration}次</span>
                  </Row>
                ))}
                <Div />
                <Lbl text="// 對手倉" />
                {data?.pair_trades?.slice(0, 3).map((pt, i) => (
                  <Row key={i}>
                    <div className="flex flex-col gap-0.5">
                      <span className="font-mono-eden text-[10px] font-semibold">{pt.institution}</span>
                      <span className="font-mono-eden text-[9px] text-[var(--text-secondary)]">買 [{pt.buy_symbols.join(",")}] 賣 [{pt.sell_symbols.join(",")}]</span>
                    </div>
                  </Row>
                ))}
                <Div />
                <Lbl text="// 機構撤退" color="red" />
                {data?.exoduses?.slice(0, 3).map((e, i) => (
                  <span key={i} className="font-mono-eden text-[9px] text-[var(--accent-red)] opacity-60">
                    {e.institution} {e.prev_stock_count}→{e.curr_stock_count} (-{e.dropped_count})
                  </span>
                ))}
              </>) : (<>
                <Lbl text="// 收斂信號" />
                {data?.convergence_scores?.slice(0, 8).map(c => (
                  <Row key={c.symbol} active={selected === c.symbol} onClick={() => selectStock(c.symbol)}>
                    <span className={`font-mono-eden text-[11px] font-semibold ${selected === c.symbol ? "text-[var(--accent-green)]" : ""}`}>{c.symbol}</span>
                    <span className={`font-mono-eden text-[11px] font-bold ${pctColor(c.dimension_composite)}`}>{pct(c.dimension_composite)}</span>
                    {parseFloat(c.cross_market_propagation) !== 0 && <span className={`font-mono-eden text-[8px] ${pctColor(c.cross_market_propagation)}`}>港:{pct(c.cross_market_propagation)}</span>}
                  </Row>
                ))}
                <Div />
                <Lbl text="// 跨市場 HK→US" />
                {data?.cross_market_signals?.slice(0, 3).map((cm, i) => (
                  <Row key={i}>
                    <div className="flex flex-col gap-0.5 w-full">
                      <div className="flex justify-between">
                        <span className="font-mono-eden text-[10px] font-semibold text-[var(--accent-orange)]">{cm.us_symbol} ← {cm.hk_symbol}</span>
                        <span className={`font-mono-eden text-[9px] font-bold ${pctColor(cm.hk_composite)}`}>{pct(cm.propagation_confidence)}</span>
                      </div>
                      <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">港股綜合={pct(cm.hk_composite)} | {cm.time_since_hk_close_minutes}分鐘前</span>
                    </div>
                  </Row>
                ))}
                <Div />
                <Lbl text="// 事件" />
                {data?.events?.slice(0, 4).map((ev, i) => (
                  <div key={i} className="flex items-center gap-1.5">
                    <div className={`w-1.5 h-1.5 rounded-full shrink-0 ${parseFloat(ev.magnitude) > 0.5 ? "bg-[var(--accent-red)]" : "bg-[var(--accent-orange)]"}`} />
                    <span className="font-mono-eden text-[9px] text-[var(--text-secondary)]">{ev.summary}</span>
                  </div>
                ))}
              </>)}

              {/* US-only: 壓力 + 板塊輪動 + 持倉 + 跨市場異常 */}
              {market === "us" && data?.pressures && data.pressures.length > 0 && (<>
                <Div />
                <Lbl text="// 資金壓力" />
                {data.pressures.slice(0, 5).map(p => (
                  <Row key={p.symbol} active={selected === p.symbol} onClick={() => selectStock(p.symbol)}>
                    <span className={`font-mono-eden text-[11px] font-semibold ${selected === p.symbol ? "text-[var(--accent-green)]" : ""}`}>{p.symbol}</span>
                    <span className={`font-mono-eden text-[10px] font-bold ${pctColor(p.capital_flow_pressure ?? "0")}`}>
                      {parseFloat(p.capital_flow_pressure ?? "0") > 0 ? "▲" : "▼"}{pct(p.capital_flow_pressure ?? "0")}
                    </span>
                    <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">{p.pressure_duration}次{p.accelerating ? " ↑" : ""}</span>
                  </Row>
                ))}
              </>)}
              {market === "us" && data?.rotations && data.rotations.length > 0 && (<>
                <Div />
                <Lbl text="// 板塊輪動" />
                {data.rotations.slice(0, 3).map((r, i) => (
                  <div key={i} className="flex justify-between font-mono-eden text-[9px]">
                    <span>{r.sector_a} → {r.sector_b}</span>
                    <span className={pctColor(r.widening ? "1" : "-1")}>{pct(r.spread)} {r.widening ? "↑擴大" : "↓收窄"}</span>
                  </div>
                ))}
              </>)}
              {market === "us" && data?.cross_market_anomalies && data.cross_market_anomalies.length > 0 && (<>
                <Div />
                <Lbl text="// 跨市場異常" color="red" />
                {data.cross_market_anomalies.slice(0, 3).map((a, i) => (
                  <div key={i} className="flex flex-col gap-0.5 bg-[var(--accent-red-20)] px-2 py-1 rounded cursor-pointer" onClick={() => selectStock(a.us_symbol)}>
                    <span className="font-mono-eden text-[9px] font-semibold text-[var(--accent-red)]">{a.us_symbol} ← {a.hk_symbol}</span>
                    <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">預期{parseFloat(a.expected_direction) > 0 ? "多" : "空"} 實際{parseFloat(a.actual_direction) > 0 ? "多" : "空"} 偏差={pct(a.divergence)}</span>
                  </div>
                ))}
              </>)}
              {market === "us" && (data?.active_positions ?? 0) > 0 && (<>
                <Div />
                <Lbl text="// 持倉追蹤" />
                {data?.workflows?.filter(w => w.stage === "monitoring").slice(0, 3).map((w, i) => (
                  <div key={i} className="flex justify-between items-center bg-[var(--bg-elevated)] px-2 py-1 rounded">
                    <span className="font-mono-eden text-[10px] font-semibold">{w.symbol}</span>
                    <div className="flex gap-2 items-center">
                      {w.pnl && <span className={`font-mono-eden text-[9px] font-bold ${pctColor(w.pnl)}`}>{parseFloat(w.pnl) > 0 ? "+" : ""}{parseFloat(w.pnl).toFixed(2)}</span>}
                      <Badge label="監控中" color="orange" small />
                    </div>
                  </div>
                ))}
              </>)}

              {/* 常駐：評分 + 戰術 + 追蹤 */}
              <Div />
              <Lbl text="// 信號評分" />
              {Array.isArray(data?.scorecard) && data.scorecard.length > 0 ? data.scorecard.map(s => (
                <div key={s.signal_type} className="flex justify-between items-center bg-[var(--bg-elevated)] px-2 py-1 rounded">
                  <span className="font-mono-eden text-[9px] text-[var(--text-secondary)]">{s.signal_type}</span>
                  <div className="flex items-center gap-2">
                    <span className={`font-display text-sm font-bold ${pctColor(s.hit_rate)}`}>{pct(s.hit_rate)}</span>
                    <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">{s.resolved}/{s.total}</span>
                  </div>
                </div>
              )) : <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">等待信號評分</span>}

              <Div />
              <Lbl text="// 戰術案件" />
              {data?.tactical_cases?.slice(0, 3).map((c, i) => (
                <div key={i} className="flex items-center gap-1.5 bg-[var(--bg-elevated)] px-2 py-1 rounded cursor-pointer hover:brightness-125 transition-all"
                  onClick={() => { const sym = c.title.match(/\d+/)?.[0]; if (sym) selectStock(market === "hk" ? `${sym}.HK` : `${sym}.US`); }}>
                  <Badge label={c.action === "enter" ? "進場" : c.action === "review" ? "觀望" : c.action === "exit" ? "退出" : c.action} color={c.action === "enter" ? "green" : c.action === "review" ? "orange" : "red"} small />
                  <span className="font-mono-eden text-[9px]">{c.title}</span>
                  <span className="font-mono-eden text-[8px] text-[var(--text-muted)] ml-auto">{pct(c.confidence)}</span>
                </div>
              )) ?? <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">等待戰術案件</span>}

              {Array.isArray(data?.lineage) && data.lineage.length > 0 && (<>
                <Div />
                <Lbl text="// 信號追蹤" />
                {data.lineage.slice(0, 4).map((l, i) => (
                  <div key={i} className="flex justify-between">
                    <span className="font-mono-eden text-[9px] text-[var(--text-secondary)]">{l.template}</span>
                    <span className={`font-mono-eden text-[9px] font-bold ${pctColor(l.hit_rate)}`}>{pct(l.hit_rate)}</span>
                  </div>
                ))}
              </>)}
            </div>
          </div>

          {/* ── 熱力圖 ── */}
          <div className="flex-1 bg-[#0c0c18] relative overflow-hidden">
            <div className="absolute top-3 left-4 flex gap-1.5 z-10">
              <Chip label="全部" active={!sectorFilter} onClick={() => setSectorFilter(null)} />
              {sectors.map(s => <Chip key={s} label={s} active={sectorFilter === s} onClick={() => setSectorFilter(sectorFilter === s ? null : s)} />)}
            </div>
            <svg className="absolute inset-0 w-full h-full" viewBox="0 0 800 600" preserveAspectRatio="xMidYMid meet">
              <defs>
                <filter id="gl" x="-50%" y="-50%" width="200%" height="200%">
                  <feGaussianBlur stdDeviation="8" result="b" />
                  <feColorMatrix in="b" type="matrix" values="0 0 0 0 0.13 0 0 0 0 0.77 0 0 0 0 0.37 0 0 0 0.45 0" />
                  <feMerge><feMergeNode /><feMergeNode in="SourceGraphic" /></feMerge>
                </filter>
                <filter id="gr" x="-50%" y="-50%" width="200%" height="200%">
                  <feGaussianBlur stdDeviation="8" result="b" />
                  <feColorMatrix in="b" type="matrix" values="0 0 0 0 0.94 0 0 0 0 0.27 0 0 0 0 0.27 0 0 0 0.45 0" />
                  <feMerge><feMergeNode /><feMergeNode in="SourceGraphic" /></feMerge>
                </filter>
                <radialGradient id="bg" cx="50%" cy="50%" r="60%">
                  <stop offset="0%" stopColor="#141428" /><stop offset="100%" stopColor="#0a0a14" />
                </radialGradient>
              </defs>
              <rect width="800" height="600" fill="url(#bg)" />
              {data?.pair_trades?.flatMap((pt, pi) =>
                pt.buy_symbols.flatMap(buy =>
                  pt.sell_symbols.map(sell => {
                    const b1 = filtered.find(b => b.symbol === buy), b2 = filtered.find(b => b.symbol === sell);
                    if (!b1 || !b2) return null;
                    return <path key={`c-${pi}-${buy}-${sell}`} d={`M${b1.cx},${b1.cy} Q${(b1.cx + b2.cx) / 2},${(b1.cy + b2.cy) / 2 - 25} ${b2.cx},${b2.cy}`} fill="none" stroke="rgba(251,146,60,0.12)" strokeWidth={1.5} strokeDasharray="6 4" />;
                  })
                )
              )}
              {market === "us" && data?.cross_market_signals?.map((cm, i) => {
                const ub = filtered.find(b => b.symbol === cm.us_symbol);
                if (!ub) return null;
                return <g key={`x-${i}`}><line x1={ub.cx} y1={ub.cy + ub.r + 2} x2={ub.cx} y2={ub.cy + ub.r + 16} stroke="rgba(251,146,60,0.35)" strokeWidth={1} /><text x={ub.cx} y={ub.cy + ub.r + 24} textAnchor="middle" fill="rgba(251,146,60,0.5)" fontSize={7} fontFamily="'JetBrains Mono',monospace">{`← ${cm.hk_symbol}`}</text></g>;
              })}
              {filtered.map(b => {
                const sel = selected === b.symbol, bull = b.comp > 0;
                const fa = 0.06 + b.absComp * 0.22, sa = 0.25 + b.absComp * 0.5;
                return (
                  <g key={b.symbol} className="cursor-pointer" onClick={() => selectStock(b.symbol)}>
                    {b.absComp > 0.2 && <circle cx={b.cx} cy={b.cy} r={b.r + 4} fill="none" stroke={bull ? `rgba(34,197,94,${sa * 0.3})` : `rgba(239,68,68,${sa * 0.3})`} strokeWidth={6} filter={b.absComp > 0.35 ? (bull ? "url(#gl)" : "url(#gr)") : undefined} />}
                    <circle cx={b.cx} cy={b.cy} r={b.r} fill={bull ? `rgba(34,197,94,${fa})` : `rgba(239,68,68,${fa})`} stroke={sel ? (bull ? "#22c55e" : "#ef4444") : (bull ? `rgba(34,197,94,${sa})` : `rgba(239,68,68,${sa})`)} strokeWidth={sel ? 2.5 : 0.8} />
                    {b.accelerating && <circle cx={b.cx} cy={b.cy} r={b.r + 6} fill="none" stroke={bull ? "rgba(34,197,94,0.2)" : "rgba(239,68,68,0.2)"} strokeWidth={0.6} strokeDasharray="3 5"><animateTransform attributeName="transform" type="rotate" from={`0 ${b.cx} ${b.cy}`} to={`360 ${b.cx} ${b.cy}`} dur="10s" repeatCount="indefinite" /></circle>}
                    <text x={b.cx} y={b.cy - (b.r > 28 ? 4 : 2)} textAnchor="middle" fontSize={b.r > 28 ? 11 : 8} fontWeight="600" fontFamily="'JetBrains Mono',monospace" fill={bull ? "#22c55e" : "#ef4444"}>{b.symbol.replace(".HK", "").replace(".US", "")}</text>
                    {b.r > 22 && <text x={b.cx} y={b.cy + (b.r > 28 ? 9 : 7)} textAnchor="middle" fontSize={b.r > 28 ? 9 : 7} fontFamily="'JetBrains Mono',monospace" fill={bull ? "rgba(34,197,94,0.55)" : "rgba(239,68,68,0.55)"}>{pct(String(b.comp))}</text>}
                    {b.instCount > 3 && b.r > 25 && <text x={b.cx} y={b.cy + (b.r > 28 ? 19 : 15)} textAnchor="middle" fontSize={6} fontFamily="'JetBrains Mono',monospace" fill="rgba(148,163,184,0.35)">{b.instCount}機構</text>}
                  </g>
                );
              })}
            </svg>
          </div>

          {/* ── 右面板：一頁看完，不分 tab ── */}
          {selected && (
            <div className="w-[320px] bg-[var(--bg-sidebar)] border-l border-[var(--border-gray)] flex flex-col shrink-0">
              <div className="flex items-center justify-between px-3 py-1.5 bg-[var(--bg-card)] border-b border-[var(--border-gray)]">
                <div className="flex items-center gap-1.5">
                  <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">{sectorMap[selected] ?? "—"}</span>
                  <span className="text-[var(--border-gray)]">›</span>
                  <span className={`font-mono-eden text-[9px] font-semibold ${pctColor(selectedSignal?.composite ?? "0")}`}>{selected}</span>
                </div>
                <button onClick={() => setSelected(null)} className="font-mono-eden text-xs text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors p-0.5">✕</button>
              </div>

              <div className="flex-1 overflow-y-auto p-3 flex flex-col gap-2">
                {/* 概覽 */}
                <div className="flex justify-between items-center">
                  <div>
                    <div className="font-display text-base font-bold tracking-tight">{selected}</div>
                    <div className="font-mono-eden text-[8px] text-[var(--text-muted)]">{selectedSignal ? `綜合=${pct(selectedSignal.composite)}` : ""}</div>
                  </div>
                  {selectedSignal?.mark_price && <div className={`font-display text-lg font-bold ${pctColor(selectedSignal.composite)}`}>{parseFloat(selectedSignal.mark_price).toFixed(2)}</div>}
                </div>
                {selectedSignal && (
                  <div className="flex gap-1">
                    <Stat label="綜合" value={pct(selectedSignal.composite)} color={pctColor(selectedSignal.composite)} />
                    <Stat label="機構" value={pct(selectedSignal.institutional_alignment)} color={pctColor(selectedSignal.institutional_alignment)} />
                    <Stat label="板塊" value={selectedSignal.sector_coherence ? pct(selectedSignal.sector_coherence) : "無"} color={selectedSignal.sector_coherence ? pctColor(selectedSignal.sector_coherence) : "text-[var(--text-muted)]"} />
                    <Stat label="相關性" value={pct(selectedSignal.cross_stock_correlation)} color={pctColor(selectedSignal.cross_stock_correlation)} />
                  </div>
                )}

                {/* 戰術 */}
                {data?.tactical_cases?.filter(c => c.title.includes(selected.replace(".HK", "").replace(".US", ""))).slice(0, 2).map((c, i) => (
                  <div key={i} className="flex items-center gap-1.5 bg-[var(--accent-red-20)] border border-[var(--accent-red)]/20 px-2 py-1.5 rounded">
                    <div className="w-1.5 h-1.5 rounded-full bg-[var(--accent-red)]" />
                    <span className="font-mono-eden text-[10px] font-semibold">{c.title}</span>
                    <Badge label={c.action === "enter" ? "進場" : c.action === "review" ? "觀望" : "退出"} color={c.action === "enter" ? "green" : "orange"} small />
                  </div>
                ))}

                {/* 機構 */}
                {data?.pair_trades?.filter(pt => pt.buy_symbols.includes(selected) || pt.sell_symbols.includes(selected)).length ? (<>
                  <Div />
                  <Lbl text="// 機構活動" />
                  {data?.pair_trades?.filter(pt => pt.buy_symbols.includes(selected) || pt.sell_symbols.includes(selected)).slice(0, 4).map((pt, i) => (
                    <div key={i} className="flex flex-col gap-1 bg-[var(--bg-elevated)] px-2 py-1.5 rounded">
                      <div className="flex items-center gap-1.5">
                        <div className={`w-1.5 h-1.5 rounded-full shrink-0 ${pt.sell_symbols.includes(selected) ? "bg-[var(--accent-red)]" : "bg-[var(--accent-green)]"}`} />
                        <span className={`font-mono-eden text-[10px] font-semibold ${pt.sell_symbols.includes(selected) ? "text-[var(--accent-red)]" : "text-[var(--accent-green)]"}`}>
                          {pt.institution} → {pt.sell_symbols.includes(selected) ? "賣出" : "買入"}
                        </span>
                      </div>
                      <div className="flex gap-1 flex-wrap">
                        {pt.buy_symbols.filter(s => s !== selected).map(s => <button key={s} onClick={() => selectStock(s)} className="font-mono-eden text-[8px] text-[var(--accent-green)] hover:underline">▲{s}</button>)}
                        {pt.sell_symbols.filter(s => s !== selected).map(s => <button key={s} onClick={() => selectStock(s)} className="font-mono-eden text-[8px] text-[var(--accent-red)] hover:underline">▼{s}</button>)}
                      </div>
                    </div>
                  ))}
                </>) : null}

                {/* 壓力 */}
                {selectedPressure && (<>
                  <Div />
                  <Lbl text="// 壓力指標" />
                  <div className="bg-[var(--bg-elevated)] px-2 py-1.5 rounded flex flex-col gap-0.5">
                    <div className="flex justify-between">
                      <span className="font-mono-eden text-[9px] text-[var(--text-secondary)]">淨壓力</span>
                      <span className={`font-mono-eden text-[10px] font-bold ${pctColor((selectedPressure.net_pressure ?? selectedPressure.capital_flow_pressure ?? "0"))}`}>{pct((selectedPressure.net_pressure ?? selectedPressure.capital_flow_pressure ?? "0"))}</span>
                    </div>
                    <div className="flex gap-3 font-mono-eden text-[8px] text-[var(--text-muted)]">
                      <span>變化={pct(selectedPressure.pressure_delta)}</span>
                      <span>持續={selectedPressure.pressure_duration}次</span>
                      <span>買={selectedPressure.buy_inst_count} 賣={selectedPressure.sell_inst_count}</span>
                    </div>
                  </div>
                </>)}

                {/* 隱藏連結 */}
                {data?.hidden_links?.filter(hl => hl.symbol_a === selected || hl.symbol_b === selected).length ? (<>
                  <Div />
                  <Lbl text="// 隱藏連結" />
                  {data.hidden_links.filter(hl => hl.symbol_a === selected || hl.symbol_b === selected).slice(0, 3).map((hl, i) => {
                    const other = hl.symbol_a === selected ? hl.symbol_b : hl.symbol_a;
                    return (
                      <div key={i} className="bg-[var(--bg-elevated)] px-2 py-1.5 rounded cursor-pointer hover:brightness-125 transition-all" onClick={() => selectStock(other)}>
                        <div className="flex justify-between">
                          <span className="font-mono-eden text-[10px] font-semibold text-[var(--accent-orange)]">↔ {other}</span>
                          <span className="font-mono-eden text-[9px] text-[var(--text-secondary)]">相似={pct(hl.jaccard)}</span>
                        </div>
                        <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">{hl.shared_institutions} 間共同機構</span>
                      </div>
                    );
                  })}
                </>) : null}

                {/* 假說 */}
                {data?.hypothesis_tracks?.filter(h => h.title.includes(selected.replace(".HK", "").replace(".US", ""))).length ? (<>
                  <Div />
                  <Lbl text="// 假說追蹤" />
                  {data.hypothesis_tracks.filter(h => h.title.includes(selected.replace(".HK", "").replace(".US", ""))).slice(0, 3).map((h, i) => (
                    <div key={i} className="bg-[var(--bg-elevated)] px-2 py-1.5 rounded flex flex-col gap-0.5">
                      <div className="flex justify-between items-center">
                        <span className="font-mono-eden text-[9px] font-semibold">{h.title}</span>
                        <Badge label={h.status === "strengthening" ? "增強中" : h.status === "weakening" ? "減弱中" : h.status === "invalidated" ? "已失效" : "穩定"} color={h.status === "strengthening" ? "green" : h.status === "weakening" ? "red" : "orange"} small />
                      </div>
                      <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">持續={h.age_ticks}次 | 信心={pct(h.confidence)}</span>
                    </div>
                  ))}
                </>) : null}

                {/* 跨市場 */}
                {market === "us" && data?.cross_market_signals?.filter(cm => cm.us_symbol === selected).slice(0, 2).map((cm, i) => (
                  <div key={i}>
                    <Div /><Lbl text="// 跨市場" />
                    <div className="bg-[var(--bg-elevated)] px-2 py-1.5 rounded mt-1">
                      <div className="flex justify-between">
                        <span className="font-mono-eden text-[10px] font-semibold text-[var(--accent-orange)]">← {cm.hk_symbol}</span>
                        <span className={`font-mono-eden text-[9px] font-bold ${pctColor(cm.hk_composite)}`}>{pct(cm.propagation_confidence)}</span>
                      </div>
                      <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">港股綜合={pct(cm.hk_composite)} | {cm.time_since_hk_close_minutes}分鐘前收盤</span>
                    </div>
                  </div>
                ))}
                {/* 回溯推理 */}
                {selected && data?.backward_chains?.filter(c => c.symbol === selected).slice(0, 1).map((chain, i) => (
                  <div key={i}>
                    <Div />
                    <Lbl text="// 回溯推理" />
                    <div className="bg-[var(--bg-elevated)] px-2 py-1.5 rounded flex flex-col gap-1 mt-1">
                      <span className="font-mono-eden text-[9px] font-semibold">{chain.conclusion}</span>
                      {chain.evidence.slice(0, 4).map((e, j) => (
                        <div key={j} className="flex justify-between">
                          <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">{e.description}</span>
                          <span className={`font-mono-eden text-[8px] font-bold ${pctColor(e.direction)}`}>{pct(e.weight)}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                ))}

                {/* 因果 leader */}
                {selected && data?.causal_leaders?.filter(c => c.symbol === selected).slice(0, 1).map((cl, i) => (
                  <div key={i}>
                    <Div />
                    <Lbl text="// 因果追蹤" />
                    <div className="bg-[var(--bg-elevated)] px-2 py-1.5 rounded flex gap-3 mt-1">
                      <span className="font-mono-eden text-[9px] text-[var(--text-secondary)]">主導維度：<span className="font-semibold text-[var(--text-primary)]">{cl.current_leader}</span></span>
                      <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">持續{cl.leader_streak}次 | {cl.flips}次翻轉</span>
                    </div>
                  </div>
                ))}
              </div>

              {/* 行動欄 */}
              <div className="border-t border-[var(--border-gray)] bg-[var(--bg-card)] p-3 flex flex-col gap-1.5">
                {curAction ? (
                  <div className="flex items-center justify-center gap-2 py-1">
                    <span className={`font-mono-eden text-[10px] font-bold ${curAction === "confirm" ? "text-[var(--accent-red)]" : curAction === "review" ? "text-[var(--accent-orange)]" : "text-[var(--text-muted)]"}`}>
                      {curAction === "confirm" ? "✓ 已確認做空" : curAction === "review" ? "⟳ 已降級為觀望" : "— 已忽略"}
                    </span>
                    <button onClick={() => setActionTaken(p => { const n = { ...p }; delete n[selected]; return n; })} className="font-mono-eden text-[8px] text-[var(--text-muted)] hover:text-[var(--text-primary)] underline">撤回</button>
                  </div>
                ) : (
                  <div className="flex gap-1.5">
                    <button onClick={() => { if (selected) setActionTaken(p => ({ ...p, [selected]: "confirm" })); }} className="flex-1 py-1.5 bg-[var(--accent-red)] font-mono-eden text-[9px] font-bold text-[var(--bg-page)] rounded hover:brightness-125 active:scale-95 transition-all">確認做空</button>
                    <button onClick={() => { if (selected) setActionTaken(p => ({ ...p, [selected]: "review" })); }} className="flex-1 py-1.5 border border-[var(--accent-orange)]/40 font-mono-eden text-[9px] font-semibold text-[var(--accent-orange)] rounded hover:bg-[var(--accent-orange-20)] active:scale-95 transition-all">降級觀望</button>
                    <button onClick={() => { if (selected) setActionTaken(p => ({ ...p, [selected]: "dismiss" })); }} className="flex-1 py-1.5 border border-[var(--border-gray)] font-mono-eden text-[9px] text-[var(--text-muted)] rounded hover:text-[var(--text-secondary)] active:scale-95 transition-all">忽略</button>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </div>

      {error && !data && (
        <div className="fixed inset-0 flex items-center justify-center bg-black/80 z-50">
          <div className="bg-[var(--bg-card)] border border-[var(--border-gray)] p-8 max-w-md text-center rounded">
            <div className="font-display text-xl font-bold mb-2">Eden 未連接</div>
            <div className="font-mono-eden text-sm text-[var(--text-muted)] mb-4">請先啟動 Eden: <code className="text-[var(--accent-green)]">cargo run</code></div>
          </div>
        </div>
      )}
    </div>
  );
}

// ── 組件 ──

function PinBtn({ label, color, onClick, active }: { label: string; color: "red" | "green"; onClick: () => void; active?: boolean }) {
  const c = color === "red" ? "bg-[var(--accent-red-20)] border-[var(--accent-red)] text-[var(--accent-red)]" : "bg-[var(--accent-green-10)] border-[var(--accent-green)] text-[var(--accent-green)]";
  return <button onClick={onClick} className={`w-7 h-5 border rounded flex items-center justify-center font-mono-eden text-[7px] font-semibold transition-all hover:brightness-150 active:scale-90 ${c} ${active ? "ring-1 ring-white/30 scale-110" : ""}`}>{label}</button>;
}

function Badge({ label, color, small }: { label: string; color: "green" | "orange" | "red"; small?: boolean }) {
  const bg = color === "green" ? "bg-[var(--accent-green-10)]" : color === "orange" ? "bg-[var(--accent-orange-20)]" : "bg-[var(--accent-red-20)]";
  const fg = color === "green" ? "text-[var(--accent-green)]" : color === "orange" ? "text-[var(--accent-orange)]" : "text-[var(--accent-red)]";
  return <span className={`font-mono-eden ${small ? "text-[7px] px-1.5 py-0.5" : "text-[9px] px-2 py-0.5"} font-bold rounded ${bg} ${fg}`}>{label}</span>;
}

function Lbl({ text, color }: { text: string; color?: "red" }) {
  return <span className={`font-mono-eden text-[10px] font-bold tracking-wider ${color === "red" ? "text-[var(--accent-red)]" : "text-[var(--accent-green)]"}`}>{text}</span>;
}

function Div() { return <div className="w-full h-px bg-[var(--border-gray)]" />; }

function Row({ children, active, onClick }: { children: React.ReactNode; active?: boolean; onClick?: () => void }) {
  return <div className={`flex justify-between items-center px-2 py-0.5 -mx-1 rounded transition-colors ${onClick ? "cursor-pointer" : ""} ${active ? "bg-[var(--bg-elevated)]" : "hover:bg-[var(--bg-elevated)]"}`} onClick={onClick}>{children}</div>;
}

function Chip({ label, active, onClick }: { label: string; active?: boolean; onClick: () => void }) {
  return <button onClick={onClick} className={`font-mono-eden text-[8px] px-2 py-0.5 border rounded transition-all ${active ? "bg-[var(--accent-green-10)] border-[var(--accent-green)]/40 text-[var(--accent-green)] font-semibold" : "border-[var(--border-gray)] text-[var(--text-muted)] hover:text-[var(--text-secondary)]"}`}>{label}</button>;
}

function Stat({ label, value, color }: { label: string; value: string; color: string }) {
  return (
    <div className="flex-1 bg-[var(--bg-elevated)] border border-[var(--border-gray)] p-1.5 flex flex-col gap-0.5 rounded">
      <span className="font-mono-eden text-[7px] text-[var(--text-muted)]">{label}</span>
      <span className={`font-display text-sm font-bold ${color}`}>{value}</span>
    </div>
  );
}
