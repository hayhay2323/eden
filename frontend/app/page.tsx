"use client";
import { useEffect, useState, useCallback, useMemo } from "react";

const API_BASE = process.env.NEXT_PUBLIC_EDEN_API_URL || "http://localhost:8787";
const API_KEY = process.env.NEXT_PUBLIC_EDEN_API_KEY || "";

type Market = "hk" | "us";

/* eslint-disable @typescript-eslint/no-explicit-any */
interface Snap {
  tick: number; timestamp: string;
  stress?: any; market_regime?: any;
  pressures?: any[]; pair_trades?: any[]; exoduses?: any[];
  hidden_links?: any[]; conflicts?: any[];
  tactical_cases?: any[]; hypothesis_tracks?: any[];
  scorecard?: any; top_signals?: any[];
  events?: any[]; convergence_scores?: any[];
  cross_market_signals?: any[];
  lineage?: any;
  backward_chains?: any[]; workflows?: any[];
  active_positions?: number; causal_leaders?: any[];
  rotations?: any[]; clusters?: any[];
  cross_market_anomalies?: any[];
  observation_count?: number; hypothesis_count?: number;
}

function pct(v: string | number): string {
  const n = typeof v === "string" ? parseFloat(v) : v;
  if (isNaN(n)) return "—";
  return `${(n * 100).toFixed(1)}%`;
}
function pctColor(v: string | number): string {
  const n = typeof v === "string" ? parseFloat(v) : v;
  if (isNaN(n) || n === 0) return "text-[var(--text-muted)]";
  return n > 0 ? "text-[var(--accent-green)]" : "text-[var(--accent-red)]";
}
function pctBg(v: string | number): string {
  const n = typeof v === "string" ? parseFloat(v) : v;
  if (isNaN(n) || n === 0) return "";
  return n > 0 ? "bg-[var(--accent-green-10)] border-[var(--accent-green)]/20" : "bg-[var(--accent-red-20)] border-[var(--accent-red)]/20";
}

export default function Dashboard() {
  const [data, setData] = useState<Snap | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [market, setMarket] = useState<Market>("us");

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
    setData(null); setSelected(null);
    fetchLive();
    const iv = setInterval(fetchLive, 2000);
    return () => clearInterval(iv);
  }, [fetchLive]);

  // Merge tactical cases with backward reasoning
  const opportunities = useMemo(() => {
    if (!data?.tactical_cases) return [];
    return data.tactical_cases.slice(0, 8).map(tc => {
      const sym = tc.title?.split(" ")[0] || "";
      const chain = data.backward_chains?.find(c => c.symbol === sym);
      const pressure = data.pressures?.find(p => p.symbol === sym);
      return { ...tc, sym, reason: chain?.conclusion || tc.entry_rationale || "", evidence: chain?.evidence || [], pressure };
    });
  }, [data]);

  // Top movers with reasons
  const movers = useMemo(() => {
    const cs = data?.convergence_scores || data?.top_signals || [];
    return cs.slice(0, 6).map(c => {
      const sym = c.symbol;
      const comp = parseFloat(c.composite || c.dimension_composite || "0");
      const chain = data?.backward_chains?.find(ch => ch.symbol === sym);
      return { sym, comp, reason: chain?.conclusion || "" };
    });
  }, [data]);

  // Lineage (normalize format)
  const lineage: any[] = useMemo(() => {
    if (!data?.lineage) return [];
    if (Array.isArray(data.lineage)) return data.lineage;
    return data.lineage.by_template || [];
  }, [data]);

  // Scorecard hit rate
  const hitRate = useMemo(() => {
    if (!data?.scorecard) return null;
    if (Array.isArray(data.scorecard)) return null;
    if (data.scorecard.hit_rate) return parseFloat(data.scorecard.hit_rate);
    return null;
  }, [data]);

  // Stress
  const stressVal = data?.stress?.composite_stress ? parseFloat(data.stress.composite_stress) : null;
  const consensusVal = data?.stress?.momentum_consensus ? parseFloat(data.stress.momentum_consensus) : data?.stress?.sector_synchrony ? parseFloat(data.stress.sector_synchrony) : null;

  return (
    <div className="h-full flex flex-col bg-[var(--bg-page)]">
      {/* ── 頂欄：一眼看到大盤狀態 ── */}
      <div className="h-10 bg-[var(--bg-sidebar)] border-b border-[var(--border-gray)] flex items-center px-5 justify-between shrink-0">
        <div className="flex items-center gap-4">
          <span className="font-display text-base font-bold text-[var(--accent-green)]">EDEN</span>
          <div className="flex">
            <button onClick={() => setMarket("hk")} className={`font-mono-eden text-[10px] px-3 py-1 transition-all ${market === "hk" ? "bg-[var(--accent-green-10)] text-[var(--accent-green)] font-bold border border-[var(--accent-green)]/40" : "text-[var(--text-muted)] border border-[var(--border-gray)]"}`}>港股</button>
            <button onClick={() => setMarket("us")} className={`font-mono-eden text-[10px] px-3 py-1 transition-all ${market === "us" ? "bg-[var(--accent-green-10)] text-[var(--accent-green)] font-bold border border-[var(--accent-green)]/40" : "text-[var(--text-muted)] border border-[var(--border-gray)]"}`}>美股</button>
          </div>
          <span className="font-mono-eden text-[10px] text-[var(--text-muted)]">
            #{data?.tick ?? "—"} {data?.timestamp ? new Date(data.timestamp).toLocaleTimeString("zh-HK") : ""}
          </span>
        </div>
        <div className="flex items-center gap-3">
          {stressVal != null && (
            <div className="flex items-center gap-1.5">
              <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">市場壓力</span>
              <span className={`font-mono-eden text-[10px] font-bold ${stressVal > 0.3 ? "text-[var(--accent-red)]" : stressVal > 0.15 ? "text-[var(--accent-orange)]" : "text-[var(--accent-green)]"}`}>{pct(stressVal)}</span>
            </div>
          )}
          {consensusVal != null && (
            <div className="flex items-center gap-1.5">
              <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">方向共識</span>
              <span className="font-mono-eden text-[10px] font-bold text-[var(--text-primary)]">{pct(consensusVal)}</span>
            </div>
          )}
          {hitRate != null && (
            <div className="flex items-center gap-1.5">
              <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">系統命中</span>
              <span className={`font-mono-eden text-[10px] font-bold ${hitRate > 0.5 ? "text-[var(--accent-green)]" : hitRate > 0.3 ? "text-[var(--accent-orange)]" : "text-[var(--accent-red)]"}`}>{pct(hitRate)}</span>
            </div>
          )}
          <div className="w-1.5 h-1.5 rounded-full bg-[var(--accent-green)] animate-pulse" />
          <span className="font-mono-eden text-[8px] font-bold text-[var(--accent-green)]">即時</span>
        </div>
      </div>

      {/* ── 主區 ── */}
      <div className="flex-1 flex min-h-0">

        {/* ── 左欄：交易機會 + 資金流向 ── */}
        <div className="w-[380px] bg-[var(--bg-sidebar)] border-r border-[var(--border-gray)] flex flex-col overflow-y-auto shrink-0">
          <div className="p-3 flex flex-col gap-2">

            {/* 交易機會 — 最重要的東西 */}
            <div className="flex items-center justify-between">
              <span className="font-mono-eden text-[11px] font-bold text-[var(--accent-green)] tracking-wider">交易機會</span>
              <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">{opportunities.filter(o => o.action === "enter").length} 個進場信號</span>
            </div>

            {opportunities.length === 0 && (
              <span className="font-mono-eden text-[9px] text-[var(--text-muted)] py-4 text-center">等待信號...</span>
            )}

            {opportunities.map((opp, i) => (
              <div key={i}
                className={`border rounded-md p-2.5 cursor-pointer transition-all hover:brightness-110 ${selected === opp.sym ? "ring-1 ring-[var(--accent-green)]/50" : ""} ${pctBg(opp.action === "enter" ? "1" : "-0.5")}`}
                onClick={() => setSelected(opp.sym)}>
                {/* 第一行：股票 + 行動 + 信心 */}
                <div className="flex items-center justify-between mb-1">
                  <div className="flex items-center gap-2">
                    <span className="font-display text-[13px] font-bold">{opp.sym}</span>
                    <span className={`font-mono-eden text-[8px] px-1.5 py-0.5 rounded font-bold ${opp.action === "enter" ? "bg-[var(--accent-green)] text-[var(--bg-page)]" : opp.action === "review" ? "bg-[var(--accent-orange-20)] text-[var(--accent-orange)]" : "bg-[var(--bg-elevated)] text-[var(--text-muted)]"}`}>
                      {opp.action === "enter" ? "進場" : opp.action === "review" ? "觀望" : "觀察"}
                    </span>
                  </div>
                  <span className="font-mono-eden text-[11px] font-bold text-[var(--text-primary)]">{pct(opp.confidence)}</span>
                </div>
                {/* 第二行：一句話原因 — 這是最值錢的 */}
                <div className="font-mono-eden text-[9px] text-[var(--text-secondary)] leading-relaxed">
                  {opp.reason.length > 80 ? opp.reason.slice(0, 80) + "..." : opp.reason}
                </div>
                {/* 第三行：策略命中率 */}
                {lineage.length > 0 && (() => {
                  const family = opp.title?.includes("Momentum") ? "momentum_continuation" : opp.title?.includes("Pre-Market") ? "pre_market_positioning" : "";
                  const lin = lineage.find(l => l.template === family);
                  return lin ? (
                    <div className="font-mono-eden text-[8px] text-[var(--text-muted)] mt-1">
                      此策略命中率 <span className={pctColor(lin.hit_rate)}>{pct(lin.hit_rate)}</span> ({lin.resolved}筆)
                    </div>
                  ) : null;
                })()}
              </div>
            ))}

            {/* 分隔 */}
            <div className="h-px bg-[var(--border-gray)] my-1" />

            {/* 異動股票 — 什麼在動？為什麼？ */}
            <span className="font-mono-eden text-[11px] font-bold text-[var(--text-muted)] tracking-wider">異動股票</span>
            {movers.map((m, i) => (
              <div key={i} className="flex items-center gap-2 px-1 py-0.5 rounded cursor-pointer hover:bg-[var(--bg-elevated)] transition-colors"
                onClick={() => setSelected(m.sym)}>
                <span className={`font-mono-eden text-[11px] font-bold w-16 ${selected === m.sym ? "text-[var(--accent-green)]" : ""}`}>{m.sym.replace(".HK", "").replace(".US", "")}</span>
                <span className={`font-mono-eden text-[11px] font-bold w-14 text-right ${pctColor(m.comp)}`}>{pct(m.comp)}</span>
                <span className="font-mono-eden text-[8px] text-[var(--text-muted)] flex-1 truncate">{m.reason.replace(m.sym + " ", "").slice(0, 40)}</span>
              </div>
            ))}

            {/* 分隔 */}
            <div className="h-px bg-[var(--border-gray)] my-1" />

            {/* 資金流向 — 大錢往哪走？ */}
            <span className="font-mono-eden text-[11px] font-bold text-[var(--text-muted)] tracking-wider">資金流向</span>
            {data?.pressures?.slice(0, 5).map((p, i) => {
              const flow = parseFloat(p.capital_flow_pressure ?? p.net_pressure ?? "0");
              const mom = parseFloat(p.momentum ?? "0");
              return (
                <div key={i} className="flex items-center gap-2 px-1 py-0.5 cursor-pointer hover:bg-[var(--bg-elevated)] rounded transition-colors"
                  onClick={() => setSelected(p.symbol)}>
                  <span className={`font-mono-eden text-[10px] font-semibold w-16 ${selected === p.symbol ? "text-[var(--accent-green)]" : ""}`}>{p.symbol.replace(".HK", "").replace(".US", "")}</span>
                  <span className={`font-mono-eden text-[10px] font-bold w-14 text-right ${pctColor(flow)}`}>{flow > 0 ? "▲" : "▼"}{pct(flow)}</span>
                  <span className={`font-mono-eden text-[9px] w-12 text-right ${pctColor(mom)}`}>{pct(mom)}</span>
                  <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">{p.pressure_duration > 1 ? `${p.pressure_duration}次` : ""}{p.accelerating ? " ↑" : ""}</span>
                </div>
              );
            })}
          </div>
        </div>

        {/* ── 中間：異動地圖（小一點） ── */}
        <div className="flex-1 bg-[#0c0c18] relative overflow-hidden min-w-0">
          <BubbleMap data={data} market={market} selected={selected} onSelect={setSelected} />
        </div>

        {/* ── 右欄：選中股票的完整故事 ── */}
        {selected && (
          <div className="w-[320px] bg-[var(--bg-sidebar)] border-l border-[var(--border-gray)] flex flex-col shrink-0">
            <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--border-gray)]">
              <span className="font-display text-sm font-bold">{selected}</span>
              <button onClick={() => setSelected(null)} className="text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors text-sm">✕</button>
            </div>
            <div className="flex-1 overflow-y-auto p-3 flex flex-col gap-2.5">
              <DetailPanel data={data} symbol={selected} market={market} />
            </div>
            {/* 行動 */}
            <div className="border-t border-[var(--border-gray)] p-3 flex gap-1.5">
              <button className="flex-1 py-1.5 bg-[var(--accent-green)] font-mono-eden text-[9px] font-bold text-[var(--bg-page)] rounded hover:brightness-110 active:scale-95 transition-all">確認進場</button>
              <button className="flex-1 py-1.5 border border-[var(--accent-orange)]/40 font-mono-eden text-[9px] font-semibold text-[var(--accent-orange)] rounded hover:bg-[var(--accent-orange-20)] active:scale-95 transition-all">觀望</button>
              <button className="flex-1 py-1.5 border border-[var(--border-gray)] font-mono-eden text-[9px] text-[var(--text-muted)] rounded hover:text-[var(--text-secondary)] active:scale-95 transition-all">忽略</button>
            </div>
          </div>
        )}
      </div>

      {/* 離線 overlay */}
      {error && !data && (
        <div className="fixed inset-0 flex items-center justify-center bg-black/80 z-50">
          <div className="bg-[var(--bg-card)] border border-[var(--border-gray)] p-8 max-w-md text-center rounded">
            <div className="font-display text-xl font-bold mb-2">Eden 未連接</div>
            <div className="font-mono-eden text-sm text-[var(--text-muted)]">請先啟動 Eden 後端</div>
          </div>
        </div>
      )}
    </div>
  );
}

// ── 泡泡地圖 ──

function BubbleMap({ data, market, selected, onSelect }: { data: Snap | null; market: Market; selected: string | null; onSelect: (s: string) => void }) {
  const bubbles = useMemo(() => {
    type S = { symbol: string; composite: string };
    const sigs: S[] = market === "hk"
      ? (data?.top_signals?.slice(0, 20) ?? [])
      : (data?.convergence_scores?.slice(0, 20)?.map((c: any) => ({ symbol: c.symbol, composite: c.dimension_composite || c.composite })) ?? []);
    if (!sigs.length) return [];
    const phi = 2.39996323;
    return sigs.map((sig, i) => {
      const comp = parseFloat(sig.composite) || 0, absComp = Math.abs(comp);
      const pr = data?.pressures?.find((p: any) => p.symbol === sig.symbol);
      const flowMag = Math.abs(parseFloat(pr?.capital_flow_pressure ?? pr?.net_pressure ?? "0"));
      const r = Math.max(14, 10 + flowMag * 50 + absComp * 25);
      const theta = i * phi, dist = Math.sqrt(i + 0.5) * 48;
      return {
        symbol: sig.symbol, r,
        cx: Math.max(r + 5, Math.min(795 - r, 400 + dist * Math.cos(theta) + comp * 80)),
        cy: Math.max(r + 30, Math.min(570 - r, 300 + dist * Math.sin(theta))),
        comp, absComp, accelerating: pr?.accelerating ?? false,
      };
    });
  }, [data, market]);

  return (
    <svg className="absolute inset-0 w-full h-full" viewBox="0 0 800 600" preserveAspectRatio="xMidYMid meet">
      <defs>
        <filter id="gl" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="6" result="b" /><feColorMatrix in="b" type="matrix" values="0 0 0 0 0.13 0 0 0 0 0.77 0 0 0 0 0.37 0 0 0 0.4 0" /><feMerge><feMergeNode /><feMergeNode in="SourceGraphic" /></feMerge></filter>
        <filter id="gr" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="6" result="b" /><feColorMatrix in="b" type="matrix" values="0 0 0 0 0.94 0 0 0 0 0.27 0 0 0 0 0.27 0 0 0 0.4 0" /><feMerge><feMergeNode /><feMergeNode in="SourceGraphic" /></feMerge></filter>
        <radialGradient id="bg" cx="50%" cy="50%" r="60%"><stop offset="0%" stopColor="#111122" /><stop offset="100%" stopColor="#0a0a14" /></radialGradient>
      </defs>
      <rect width="800" height="600" fill="url(#bg)" />
      {bubbles.map(b => {
        const sel = selected === b.symbol, bull = b.comp > 0;
        const fa = 0.05 + b.absComp * 0.2, sa = 0.2 + b.absComp * 0.5;
        return (
          <g key={b.symbol} className="cursor-pointer" onClick={() => onSelect(b.symbol)}>
            {b.absComp > 0.25 && <circle cx={b.cx} cy={b.cy} r={b.r + 3} fill="none" stroke={bull ? `rgba(34,197,94,${sa * 0.3})` : `rgba(239,68,68,${sa * 0.3})`} strokeWidth={5} filter={b.absComp > 0.35 ? (bull ? "url(#gl)" : "url(#gr)") : undefined} />}
            <circle cx={b.cx} cy={b.cy} r={b.r} fill={bull ? `rgba(34,197,94,${fa})` : `rgba(239,68,68,${fa})`} stroke={sel ? (bull ? "#22c55e" : "#ef4444") : (bull ? `rgba(34,197,94,${sa})` : `rgba(239,68,68,${sa})`)} strokeWidth={sel ? 2.5 : 0.7} />
            {b.accelerating && <circle cx={b.cx} cy={b.cy} r={b.r + 5} fill="none" stroke={bull ? "rgba(34,197,94,0.15)" : "rgba(239,68,68,0.15)"} strokeWidth={0.5} strokeDasharray="3 4"><animateTransform attributeName="transform" type="rotate" from={`0 ${b.cx} ${b.cy}`} to={`360 ${b.cx} ${b.cy}`} dur="10s" repeatCount="indefinite" /></circle>}
            <text x={b.cx} y={b.cy - (b.r > 24 ? 3 : 1)} textAnchor="middle" fontSize={b.r > 24 ? 10 : 7} fontWeight="600" fontFamily="'JetBrains Mono',monospace" fill={bull ? "#22c55e" : "#ef4444"}>{b.symbol.replace(".HK", "").replace(".US", "")}</text>
            {b.r > 18 && <text x={b.cx} y={b.cy + (b.r > 24 ? 8 : 6)} textAnchor="middle" fontSize={b.r > 24 ? 8 : 6} fontFamily="'JetBrains Mono',monospace" fill={bull ? "rgba(34,197,94,0.5)" : "rgba(239,68,68,0.5)"}>{pct(String(b.comp))}</text>}
          </g>
        );
      })}
    </svg>
  );
}

// ── 詳情面板：選中股票的完整故事 ──

function DetailPanel({ data, symbol, market }: { data: Snap | null; symbol: string; market: Market }) {
  const signal = market === "hk"
    ? data?.top_signals?.find((s: any) => s.symbol === symbol)
    : data?.convergence_scores?.find((c: any) => c.symbol === symbol);
  const pressure = data?.pressures?.find((p: any) => p.symbol === symbol);
  const chain = data?.backward_chains?.find((c: any) => c.symbol === symbol);
  const causal = data?.causal_leaders?.find((c: any) => c.symbol === symbol);
  const tactical = data?.tactical_cases?.find((t: any) => t.title?.startsWith(symbol));
  const pairTrades = data?.pair_trades?.filter((pt: any) => pt.buy_symbols?.includes(symbol) || pt.sell_symbols?.includes(symbol)) || [];
  const crossMarket = data?.cross_market_signals?.filter((cm: any) => cm.us_symbol === symbol) || [];

  const comp = parseFloat(signal?.composite || signal?.dimension_composite || "0");
  const flow = parseFloat(pressure?.capital_flow_pressure ?? pressure?.net_pressure ?? "0");

  return (<>
    {/* 數值概覽 */}
    <div className="flex gap-1.5">
      <MiniCard label="綜合" value={pct(comp)} color={pctColor(comp)} />
      <MiniCard label="資金" value={flow ? pct(flow) : "—"} color={flow ? pctColor(flow) : "text-[var(--text-muted)]"} />
      <MiniCard label="動量" value={pressure?.momentum ? pct(pressure.momentum) : "—"} color={pressure?.momentum ? pctColor(pressure.momentum) : "text-[var(--text-muted)]"} />
    </div>

    {/* 戰術判定 */}
    {tactical && (
      <div className={`border rounded-md p-2 ${pctBg(tactical.action === "enter" ? "1" : "-1")}`}>
        <div className="flex justify-between items-center">
          <span className={`font-mono-eden text-[9px] font-bold px-1.5 py-0.5 rounded ${tactical.action === "enter" ? "bg-[var(--accent-green)] text-[var(--bg-page)]" : "bg-[var(--accent-orange-20)] text-[var(--accent-orange)]"}`}>
            {tactical.action === "enter" ? "建議進場" : "建議觀望"}
          </span>
          <span className="font-mono-eden text-[10px] font-bold">{pct(tactical.confidence)}</span>
        </div>
        <div className="font-mono-eden text-[8px] text-[var(--text-muted)] mt-1">
          信心差距={pct(tactical.confidence_gap)} 邊際={pct(tactical.heuristic_edge)}
        </div>
      </div>
    )}

    {/* 回溯推理 — 為什麼在動？ */}
    {chain && (<>
      <SectionTitle text="為什麼在動？" />
      <div className="font-mono-eden text-[10px] text-[var(--text-primary)] leading-relaxed">{chain.conclusion}</div>
      <div className="flex flex-col gap-1">
        {chain.evidence?.slice(0, 5).map((e: any, i: number) => (
          <div key={i} className="flex justify-between items-center">
            <span className="font-mono-eden text-[8px] text-[var(--text-secondary)] flex-1">{e.description}</span>
            <span className={`font-mono-eden text-[8px] font-bold ml-2 ${pctColor(e.direction)}`}>{pct(e.weight)}</span>
          </div>
        ))}
      </div>
    </>)}

    {/* 壓力細節 */}
    {pressure && (<>
      <SectionTitle text="壓力指標" />
      <div className="flex gap-3 font-mono-eden text-[8px] text-[var(--text-muted)] flex-wrap">
        <span>變化={pct(pressure.pressure_delta)}</span>
        <span>持續={pressure.pressure_duration}次</span>
        {pressure.accelerating && <span className="text-[var(--accent-orange)]">↑ 加速中</span>}
        {pressure.buy_inst_count != null && <span>買方={pressure.buy_inst_count} 賣方={pressure.sell_inst_count}</span>}
      </div>
    </>)}

    {/* HK: 機構活動 */}
    {pairTrades.length > 0 && (<>
      <SectionTitle text="機構活動" />
      {pairTrades.slice(0, 3).map((pt: any, i: number) => (
        <div key={i} className="font-mono-eden text-[9px]">
          <span className={pt.sell_symbols?.includes(symbol) ? "text-[var(--accent-red)]" : "text-[var(--accent-green)]"}>
            {pt.institution} → {pt.sell_symbols?.includes(symbol) ? "賣出" : "買入"}
          </span>
        </div>
      ))}
    </>)}

    {/* 跨市場 */}
    {crossMarket.length > 0 && (<>
      <SectionTitle text="跨市場信號" />
      {crossMarket.slice(0, 2).map((cm: any, i: number) => (
        <div key={i} className="font-mono-eden text-[9px] text-[var(--accent-orange)]">
          ← {cm.hk_symbol} 港股綜合={pct(cm.hk_composite)} 信心={pct(cm.propagation_confidence)}
        </div>
      ))}
    </>)}

    {/* 因果追蹤 */}
    {causal && (<>
      <SectionTitle text="信號主導維度" />
      <div className="font-mono-eden text-[9px] text-[var(--text-secondary)]">
        <span className="text-[var(--text-primary)] font-semibold">{causal.current_leader}</span> 持續 {causal.leader_streak} 次 | {causal.flips} 次翻轉
      </div>
    </>)}
  </>);
}

// ── 小組件 ──

function MiniCard({ label, value, color }: { label: string; value: string; color: string }) {
  return (
    <div className="flex-1 bg-[var(--bg-elevated)] border border-[var(--border-gray)] p-1.5 rounded flex flex-col gap-0.5">
      <span className="font-mono-eden text-[7px] text-[var(--text-muted)]">{label}</span>
      <span className={`font-display text-sm font-bold ${color}`}>{value}</span>
    </div>
  );
}

function SectionTitle({ text }: { text: string }) {
  return (
    <div className="flex items-center gap-2 mt-1">
      <div className="h-px flex-1 bg-[var(--border-gray)]" />
      <span className="font-mono-eden text-[8px] font-bold text-[var(--text-muted)] tracking-wider">{text}</span>
      <div className="h-px flex-1 bg-[var(--border-gray)]" />
    </div>
  );
}
