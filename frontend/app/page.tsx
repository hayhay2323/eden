"use client";
import { useEffect, useState, useCallback, useMemo } from "react";

const API = process.env.NEXT_PUBLIC_EDEN_API_URL || "http://localhost:8787";
const KEY = process.env.NEXT_PUBLIC_EDEN_API_KEY || "";

/* eslint-disable @typescript-eslint/no-explicit-any */

const P = (v: any) => { const n = parseFloat(v); return isNaN(n) ? "—" : `${(n * 100).toFixed(1)}%`; };
const C = (v: any) => { const n = parseFloat(v); return isNaN(n) || n === 0 ? "t-m" : n > 0 ? "t-g" : "t-r"; };

type NarrEntry = { t: number; level: "tick" | "min" | "hr"; text: string; color: string };

// Generate narrative entries by diffing two snapshots
function diffNarr(prev: any, curr: any): NarrEntry[] {
  if (!prev || !curr) return [];
  const now = Date.now();
  const entries: NarrEntry[] = [];

  // Convergence changes (top movers shift)
  const prevCs: Map<string, number> = new Map((prev.convergence_scores || prev.top_signals || []).map((c: any) => [c.symbol, parseFloat(c.composite || c.dimension_composite || "0")] as [string, number]));
  for (const c of (curr.convergence_scores || curr.top_signals || []).slice(0, 10)) {
    const sym = c.symbol;
    const val = parseFloat(c.composite || c.dimension_composite || "0");
    const pv = prevCs.get(sym);
    if (pv != null) {
      const delta = val - pv;
      if (Math.abs(delta) > 0.01) {
        entries.push({ t: now, level: "tick", text: `${sym} 收斂${delta > 0 ? "增強" : "減弱"} ${(pv*100).toFixed(1)}%→${(val*100).toFixed(1)}%`, color: delta > 0 ? "t-g" : "t-r" });
      }
    } else if (Math.abs(val) > 0.2) {
      entries.push({ t: now, level: "tick", text: `${sym} 新進異動 ${(val*100).toFixed(1)}%`, color: val > 0 ? "t-g" : "t-r" });
    }
  }

  // Capital flow reversals
  const prevPr: Map<string, number> = new Map((prev.pressures || []).map((p: any) => [p.symbol, parseFloat(p.capital_flow_pressure ?? p.net_pressure ?? "0")] as [string, number]));
  for (const p of (curr.pressures || []).slice(0, 10)) {
    const pv = prevPr.get(p.symbol);
    const cv = parseFloat(p.capital_flow_pressure ?? p.net_pressure ?? "0");
    if (pv != null && pv !== 0 && cv !== 0 && Math.sign(pv) !== Math.sign(cv)) {
      entries.push({ t: now, level: "tick", text: `${p.symbol} 資金流反轉 ${pv > 0 ? "流入→流出" : "流出→流入"}`, color: "t-o" });
    }
    if (p.accelerating && !(prev.pressures || []).find((pp: any) => pp.symbol === p.symbol)?.accelerating) {
      entries.push({ t: now, level: "tick", text: `${p.symbol} 壓力開始加速`, color: "t-o" });
    }
  }

  // Tactical case changes
  const prevTc: Map<string, { action: string; confidence: number }> = new Map((prev.tactical_cases || []).map((t: any) => [t.title?.split(" ")[0], { action: t.action, confidence: parseFloat(t.confidence) }] as [string, { action: string; confidence: number }]));
  for (const tc of (curr.tactical_cases || []).slice(0, 10)) {
    const sym = tc.title?.split(" ")[0];
    const pt = prevTc.get(sym);
    const cc = parseFloat(tc.confidence);
    if (pt) {
      const delta = cc - pt.confidence;
      if (Math.abs(delta) > 0.01) {
        entries.push({ t: now, level: "tick", text: `${sym} 信心${delta > 0 ? "↑" : "↓"} ${(pt.confidence*100).toFixed(0)}%→${(cc*100).toFixed(0)}%`, color: delta > 0 ? "t-g" : "t-r" });
      }
      if (pt.action !== tc.action) {
        entries.push({ t: now, level: "tick", text: `${sym} 行動升降級 ${pt.action}→${tc.action}`, color: tc.action === "enter" ? "t-g" : "t-o" });
      }
    } else {
      entries.push({ t: now, level: "tick", text: `${sym} 新戰術案件 [${tc.action}] ${(cc*100).toFixed(0)}%`, color: "t-g" });
    }
  }

  // Stress change
  const prevStress = parseFloat(prev.stress?.composite_stress ?? "0");
  const currStress = parseFloat(curr.stress?.composite_stress ?? "0");
  if (Math.abs(currStress - prevStress) > 0.02) {
    entries.push({ t: now, level: "tick", text: `市場壓力 ${(prevStress*100).toFixed(0)}%→${(currStress*100).toFixed(0)}%`, color: currStress > prevStress ? "t-r" : "t-g" });
  }

  return entries;
}

// Aggregate tick entries into minute/hour summaries
function aggregateNarr(entries: NarrEntry[]): NarrEntry[] {
  const now = Date.now();
  const oneMin = entries.filter(e => e.level === "tick" && now - e.t < 60_000);
  const fiveMin = entries.filter(e => e.level === "tick" && now - e.t < 300_000);
  const result: NarrEntry[] = [];

  // Minute summary: count by stock direction
  if (oneMin.length >= 3) {
    const ups = new Set<string>(), downs = new Set<string>();
    for (const e of oneMin) {
      const sym = e.text.split(" ")[0];
      if (e.color === "t-g") ups.add(sym);
      if (e.color === "t-r") downs.add(sym);
    }
    if (ups.size > 0 || downs.size > 0) {
      const parts = [];
      if (ups.size > 0) parts.push(`${[...ups].slice(0, 3).join("/")} 走強`);
      if (downs.size > 0) parts.push(`${[...downs].slice(0, 3).join("/")} 走弱`);
      result.push({ t: now, level: "min", text: `1分鐘：${parts.join("，")}`, color: "t-s" });
    }
  }

  // 5-min summary
  if (fiveMin.length >= 5) {
    const flowReversals = fiveMin.filter(e => e.text.includes("資金流反轉")).length;
    const confChanges = fiveMin.filter(e => e.text.includes("信心")).length;
    const parts = [];
    if (flowReversals > 0) parts.push(`${flowReversals}次資金流反轉`);
    if (confChanges > 0) parts.push(`${confChanges}次信心變動`);
    if (parts.length > 0) result.push({ t: now, level: "hr", text: `5分鐘：${parts.join("，")} | 共${fiveMin.length}條信號`, color: "t-m" });
  }

  return result;
}

export default function Dashboard() {
  const [d, setD] = useState<any>(null);
  const [prevD, setPrevD] = useState<any>(null);
  const [err, setErr] = useState(false);
  const [exp, setExp] = useState<string | null>(null);
  const [mkt, setMkt] = useState<"hk" | "us">("us");
  const [acts, setActs] = useState<Record<string, string>>({});
  const [narr, setNarr] = useState<NarrEntry[]>([]);
  const [narrLevel, setNarrLevel] = useState<"all" | "min" | "hr">("all");

  const fetch_ = useCallback(async () => {
    try {
      const r = await fetch(`${API}${mkt === "us" ? "/api/us/live" : "/api/live"}`, { headers: { Authorization: `Bearer ${KEY}` }, cache: "no-store" });
      if (!r.ok) throw 0;
      const newData = await r.json();
      setD((prev: any) => { setPrevD(prev); return newData; });
      setErr(false);
    } catch { setErr(true); }
  }, [mkt]);

  useEffect(() => { setD(null); setPrevD(null); setExp(null); setNarr([]); fetch_(); const i = setInterval(fetch_, 2000); return () => clearInterval(i); }, [fetch_]);

  // Generate narratives on each data update
  useEffect(() => {
    if (!d || !prevD) return;
    const newEntries = diffNarr(prevD, d);
    if (newEntries.length === 0) return;
    setNarr(prev => {
      const updated = [...newEntries, ...prev].slice(0, 200); // keep last 200
      const agg = aggregateNarr(updated);
      return [...agg, ...updated].slice(0, 200);
    });
  }, [d, prevD]);

  const opps = useMemo(() => (d?.tactical_cases || []).slice(0, 5).map((t: any) => {
    const s = t.title?.split(" ")[0] || "";
    const dims = d?.top_signals?.find((ts: any) => ts.symbol === s);
    const causal = d?.causal_leaders?.find((c: any) => c.symbol === s);
    return { ...t, s, chain: d?.backward_chains?.find((c: any) => c.symbol === s), pr: d?.pressures?.find((p: any) => p.symbol === s), dims, causal };
  }), [d]);

  const movers = useMemo(() => (d?.convergence_scores || d?.top_signals || []).slice(0, 20).map((c: any) => {
    const s = c.symbol, v = parseFloat(c.composite || c.dimension_composite || "0");
    return { s, v, why: d?.backward_chains?.find((ch: any) => ch.symbol === s)?.conclusion || "" };
  }), [d]);

  const flows = useMemo(() => (d?.pressures || []).slice(0, 20).map((p: any) => ({
    s: p.symbol, f: parseFloat(p.capital_flow_pressure ?? p.net_pressure ?? "0"), m: parseFloat(p.momentum ?? "0"),
  })), [d]);

  const lin: any[] = useMemo(() => { const l = d?.lineage; if (!l) return []; return Array.isArray(l) ? l : l.by_template || []; }, [d]);
  const hr = d?.scorecard?.hit_rate ? parseFloat(d.scorecard.hit_rate) : null;
  const stress = d?.stress?.composite_stress ? parseFloat(d.stress.composite_stress) : null;
  const consensus = d?.stress?.momentum_consensus ? parseFloat(d.stress.momentum_consensus) : d?.stress?.sector_synchrony ? parseFloat(d.stress.sector_synchrony) : null;
  const regime = typeof d?.market_regime === "string" ? d.market_regime : d?.market_regime?.bias || null;

  return (
    <div className="h-full flex flex-col text-[12px]">
      {/* ═══ 頂欄 ═══ */}
      <div className="h-8 bg-[var(--bg-sidebar)] border-b border-[var(--border-gray)] flex items-center px-4 justify-between shrink-0">
        <div className="flex items-center gap-3">
          <span className="font-bold text-[var(--accent-green)] text-[13px] tracking-wider" style={{fontFamily:"Space Grotesk,sans-serif"}}>EDEN</span>
          <div className="flex text-[11px]">
            <button onClick={() => setMkt("hk")} className={`px-2 py-0.5 border transition-all ${mkt === "hk" ? "bg-[var(--accent-green-10)] text-[var(--accent-green)] font-bold border-[var(--accent-green)]/30" : "t-m border-[var(--border-gray)]"}`}>港股</button>
            <button onClick={() => setMkt("us")} className={`px-2 py-0.5 border transition-all ${mkt === "us" ? "bg-[var(--accent-green-10)] text-[var(--accent-green)] font-bold border-[var(--accent-green)]/30" : "t-m border-[var(--border-gray)]"}`}>美股</button>
          </div>
          <span className="t-m text-[11px]">#{d?.tick ?? "—"}</span>
        </div>
        <div className="flex items-center gap-4 text-[11px]">
          {regime && <span className={regime === "bullish" ? "t-g font-bold" : regime === "bearish" ? "t-r font-bold" : "t-s"}>
            {regime === "bullish" ? "偏多" : regime === "bearish" ? "偏空" : "中性"}
          </span>}
          {stress != null && <span>壓力 <b className={stress > 0.3 ? "t-r" : stress > 0.15 ? "t-o" : "t-g"}>{P(stress)}</b></span>}
          {consensus != null && <span>共識 <b>{P(consensus)}</b></span>}
          {hr != null && <span>命中 <b className={hr > 0.5 ? "t-g" : hr > 0.3 ? "t-o" : "t-r"}>{P(hr)}</b></span>}
          {lin.map((l, i) => <span key={i} className="t-m">{l.template === "momentum_continuation" ? "動量" : l.template === "pre_market_positioning" ? "盤前" : l.template === "cross_market_arbitrage" ? "跨市場" : l.template === "sector_rotation" ? "板塊" : l.template} <b className={C(l.hit_rate)}>{P(l.hit_rate)}</b></span>)}
          <span className="w-1.5 h-1.5 rounded-full bg-[var(--accent-green)] animate-pulse inline-block" />
        </div>
      </div>

      {/* ═══ 主體 ═══ */}
      <div className="flex-1 flex min-h-0">

        {/* ─── 左：行動建議 ─── */}
        <div className="flex-1 flex flex-col min-w-0 border-r border-[var(--border-gray)]">
          <div className="px-3 pt-2 pb-1 flex items-center justify-between">
            <span className="font-bold text-[11px]" style={{fontFamily:"Space Grotesk,sans-serif"}}>行動建議</span>
            <span className="t-m text-[10px]">{opps.filter((o: any) => o.action === "enter").length} 進場</span>
          </div>
          <div className="flex-1 overflow-y-auto px-3 pb-2 flex flex-col gap-1.5">
            {opps.length === 0 && <div className="t-m text-center py-6">等待信號...</div>}
            {opps.map((o: any, i: number) => {
              const open = exp === o.s;
              const acted = acts[o.s];
              const rawReason = o.chain?.conclusion || o.entry_rationale || "";
              // Translate common English rationales to Chinese
              const reason = rawReason
                .replace(/pre-market move reflects institutional positioning before regular hours/gi, "盤前異動反映機構在盤前的定位行為")
                .replace(/capital flow momentum suggests continuation/gi, "資金流動量顯示趨勢可能延續")
                .replace(/may follow HK counterpart/gi, "可能跟隨港股對應股走勢")
                .replace(/sector is gaining.*relative/gi, "板塊相對大盤走強")
                .replace(/pre-market/gi, "盤前")
                .replace(/institutional positioning/gi, "機構定位")
                .replace(/momentum continuation/gi, "動量延續")
                .replace(/capital flow/gi, "資金流");
              return (
                <div key={i} className={`border rounded px-2.5 py-1.5 cursor-pointer transition-all ${o.action === "enter" ? "border-[var(--accent-green)]/20 bg-[var(--accent-green-10)]" : "border-[var(--border-gray)] bg-[var(--bg-card)]"} ${open ? "ring-1 ring-[var(--accent-green)]/20" : ""}`}
                  onClick={() => setExp(open ? null : o.s)}>
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <b className="text-[13px]">{o.s}</b>
                      <span className={`text-[9px] px-1.5 py-px rounded-full font-bold ${o.action === "enter" ? "bg-[var(--accent-green)] text-black" : "bg-[var(--bg-elevated)] t-m"}`}>
                        {o.action === "enter" ? "進場" : "觀望"}
                      </span>
                    </div>
                    <b>{P(o.confidence)}</b>
                  </div>
                  <div className="t-s mt-0.5 leading-snug">{reason.length > 80 ? reason.slice(0, 80) + "…" : reason}</div>
                  {open && (
                    <div className="mt-1.5 pt-1.5 border-t border-[var(--border-gray)] flex flex-col gap-1.5">

                      {/* ① 收斂分解 — 87% 從哪來 */}
                      {o.dims && (
                        <div className="flex flex-col gap-0.5">
                          <span className="text-[9px] t-s font-bold tracking-wider">維度收斂</span>
                          {[
                            { k: "capital_flow_direction", label: "資金流" },
                            { k: "price_momentum", label: "動量" },
                            { k: "volume_profile", label: "量能" },
                            { k: "pre_post_market_anomaly", label: "盤前" },
                            { k: "valuation", label: "估值" },
                          ].map(({ k, label }) => {
                            const v = parseFloat(o.dims[k] ?? "0");
                            const w = Math.min(Math.abs(v) * 100, 100);
                            return (
                              <div key={k} className="flex items-center gap-1.5">
                                <span className="text-[10px] w-8 text-right t-m">{label}</span>
                                <div className="flex-1 h-[6px] bg-[var(--bg-elevated)] rounded-full overflow-hidden relative">
                                  <div className={`absolute top-0 h-full rounded-full ${v > 0 ? "bg-[var(--accent-green)]" : "bg-[var(--accent-red)]"}`} style={{ width: `${w}%`, left: v > 0 ? "50%" : `${50 - w}%` }} />
                                  <div className="absolute top-0 left-1/2 w-px h-full bg-[var(--border-gray)]" />
                                </div>
                                <span className={`text-[10px] w-12 text-right font-bold ${C(v)}`}>{v ? (v > 0 ? "+" : "") + (v * 100).toFixed(0) + "%" : "—"}</span>
                              </div>
                            );
                          })}
                        </div>
                      )}

                      {/* ② 假說競爭 — 正反兩面 */}
                      <div className="flex gap-2">
                        <div className="flex-1 bg-[var(--accent-green-10)] rounded p-1.5">
                          <div className="text-[9px] t-g font-bold">正面假說</div>
                          <div className="text-[11px] font-bold">{P(o.confidence)}</div>
                          <div className="text-[9px] t-s truncate">{o.title?.includes("Momentum") ? "動量延續" : o.title?.includes("Pre-Market") ? "盤前定位" : o.title?.includes("Cross") ? "跨市場套利" : "趨勢延續"}</div>
                        </div>
                        <div className="flex-1 bg-[var(--accent-red-20)] rounded p-1.5">
                          <div className="text-[9px] t-r font-bold">反面假說</div>
                          <div className="text-[11px] font-bold">{P(1 - parseFloat(o.confidence))}</div>
                          <div className="text-[9px] t-s truncate">{o.title?.includes("Momentum") ? "動量反轉" : o.title?.includes("Pre-Market") ? "盤前假突破" : o.title?.includes("Cross") ? "跨市場脫鉤" : "趨勢反轉"}</div>
                        </div>
                      </div>

                      {/* ③ 證據鏈 */}
                      {o.chain?.evidence?.slice(0, 4).map((e: any, j: number) => (
                        <div key={j} className="flex justify-between"><span className="t-s text-[11px]">{e.description}</span><b className={C(e.direction)}>{P(e.weight)}</b></div>
                      ))}

                      {/* ④ 壓力 + 因果 leader */}
                      <div className="flex gap-3 t-s text-[10px] flex-wrap">
                        {o.pr && <>
                          <span>資金={P(o.pr.capital_flow_pressure ?? o.pr.net_pressure ?? "0")}</span>
                          <span>持續={o.pr.pressure_duration}次</span>
                          {o.pr.accelerating && <span className="t-o">↑加速</span>}
                        </>}
                        {o.causal && <span>主導: <b className="t-s">{o.causal.current_leader}</b> {o.causal.leader_streak}次</span>}
                        <span>差距={P(o.confidence_gap)}</span>
                        <span>邊際={P(o.heuristic_edge)}</span>
                      </div>

                      {/* ⑤ 圖譜連接 */}
                      {d?.edge_count && <div className="text-[10px] t-s">圖譜: {d.stock_count}隻股票 · {d.edge_count}條關聯邊 · {d.hypothesis_count}個假說正在競爭</div>}

                      {/* 行動按鈕 */}
                      {acted ? (
                        <div className="flex items-center gap-2 justify-center py-0.5">
                          <b className={acted === "enter" ? "t-g" : "t-m"}>{acted === "enter" ? "✓ 已進場" : acted === "review" ? "⟳ 觀望" : "— 忽略"}</b>
                          <button onClick={e => { e.stopPropagation(); setActs(p => { const n = { ...p }; delete n[o.s]; return n; }); }} className="t-m underline text-[10px]">撤回</button>
                        </div>
                      ) : (
                        <div className="flex gap-1.5 mt-0.5">
                          <button onClick={e => { e.stopPropagation(); setActs(p => ({ ...p, [o.s]: "enter" })); }} className="flex-1 py-1 bg-[var(--accent-green)] text-black font-bold rounded text-[10px] hover:brightness-110 active:scale-[0.98]">進場</button>
                          <button onClick={e => { e.stopPropagation(); setActs(p => ({ ...p, [o.s]: "review" })); }} className="flex-1 py-1 border border-[var(--accent-orange)]/30 t-o font-semibold rounded text-[10px] hover:bg-[var(--accent-orange-20)]">觀望</button>
                          <button onClick={e => { e.stopPropagation(); setActs(p => ({ ...p, [o.s]: "dismiss" })); }} className="flex-1 py-1 border border-[var(--border-gray)] t-m rounded text-[10px]">忽略</button>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>

          {/* ─── 信號情報流 ─── */}
          <div className="border-t border-[var(--border-gray)] flex-1 flex flex-col min-h-0">
            <div className="px-3 pt-1.5 pb-1 flex items-center justify-between shrink-0">
              <span className="font-bold text-[11px]" style={{fontFamily:"Space Grotesk,sans-serif"}}>信號情報</span>
              <div className="flex gap-0 text-[9px]">
                <button onClick={() => setNarrLevel("all")} className={`px-1.5 py-px border ${narrLevel === "all" ? "bg-[var(--bg-elevated)] border-[var(--border-gray)] font-bold" : "border-transparent t-m"}`}>全部</button>
                <button onClick={() => setNarrLevel("min")} className={`px-1.5 py-px border ${narrLevel === "min" ? "bg-[var(--bg-elevated)] border-[var(--border-gray)] font-bold" : "border-transparent t-m"}`}>分鐘</button>
                <button onClick={() => setNarrLevel("hr")} className={`px-1.5 py-px border ${narrLevel === "hr" ? "bg-[var(--bg-elevated)] border-[var(--border-gray)] font-bold" : "border-transparent t-m"}`}>彙總</button>
              </div>
            </div>
            <div className="flex-1 overflow-y-auto px-3 pb-1">
              {narr.filter(n => narrLevel === "all" || n.level === (narrLevel === "min" ? "min" : "hr")).length === 0 ? (
                <div className="t-m text-[10px] text-center py-2">等待信號變動...</div>
              ) : (
                narr.filter(n => narrLevel === "all" || n.level === (narrLevel === "min" ? "min" : "hr")).slice(0, 30).map((n, i) => (
                  <div key={i} className={`flex items-start gap-1.5 py-[2px] ${n.level === "min" || n.level === "hr" ? "bg-[var(--bg-elevated)] -mx-1 px-1 rounded my-0.5" : ""}`}>
                    <span className="t-m text-[9px] w-[42px] shrink-0 text-right">{new Date(n.t).toLocaleTimeString("zh-HK", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}</span>
                    <span className={`text-[10px] leading-snug ${n.color}`}>{n.text}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>

        {/* ─── 中：4 格情報面板 ─── */}
        <div className="w-[260px] flex flex-col border-r border-[var(--border-gray)] shrink-0">
          {/* ① 異動監察 */}
          <div className="flex-1 flex flex-col min-h-0">
            <div className="px-3 pt-2 pb-1"><span className="font-bold text-[11px] t-s" style={{fontFamily:"Space Grotesk,sans-serif"}}>異動監察</span></div>
            <div className="flex-1 overflow-y-auto px-3 pb-1">
              {movers.map((m: any, i: number) => (
                <div key={i} className="flex items-center gap-1.5 py-[2px] hover:bg-[var(--bg-elevated)] rounded px-1 -mx-1 cursor-pointer transition-colors">
                  <span className="font-bold w-[60px] truncate text-[11px]">{m.s.replace(".HK", "").replace(".US", "")}</span>
                  <span className={`font-bold w-12 text-right text-[11px] ${C(m.v)}`}>{P(m.v)}</span>
                  <span className="t-m text-[9px] flex-1 truncate">{m.why.replace(m.s + " ", "").slice(0, 30)}</span>
                </div>
              ))}
            </div>
          </div>
          {/* ② 資金動向 */}
          <div className="border-t border-[var(--border-gray)] flex-1 flex flex-col min-h-0">
            <div className="px-3 pt-1.5 pb-1"><span className="font-bold text-[11px] t-s" style={{fontFamily:"Space Grotesk,sans-serif"}}>資金動向</span></div>
            <div className="flex-1 flex px-3 pb-1 gap-1.5 min-h-0">
              <div className="flex-1 bg-[var(--accent-green-10)] rounded p-1.5 overflow-y-auto">
                <div className="text-[9px] font-bold t-g mb-0.5">流入 ▲</div>
                {flows.filter((f: any) => f.f > 0).slice(0, 8).map((f: any, i: number) => (
                  <div key={i} className="flex justify-between py-px">
                    <span className="text-[10px]">{f.s.replace(".HK","").replace(".US","")}</span>
                    <span className="font-bold t-g text-[10px]">+{P(f.f)}</span>
                  </div>
                ))}
                {flows.filter((f: any) => f.f > 0).length === 0 && <span className="t-m text-[10px]">暫無</span>}
              </div>
              <div className="flex-1 bg-[var(--accent-red-20)] rounded p-1.5 overflow-y-auto">
                <div className="text-[9px] font-bold t-r mb-0.5">流出 ▼</div>
                {flows.filter((f: any) => f.f < 0).slice(0, 8).map((f: any, i: number) => (
                  <div key={i} className="flex justify-between py-px">
                    <span className="text-[10px]">{f.s.replace(".HK","").replace(".US","")}</span>
                    <span className="font-bold t-r text-[10px]">{P(f.f)}</span>
                  </div>
                ))}
                {flows.filter((f: any) => f.f < 0).length === 0 && <span className="t-m text-[10px]">暫無</span>}
              </div>
            </div>
          </div>
          {/* ③ 假說動態 — 信號在增強還是減弱 */}
          <div className="border-t border-[var(--border-gray)] flex-1 flex flex-col min-h-0">
            <div className="px-3 pt-1.5 pb-1"><span className="font-bold text-[11px] t-s" style={{fontFamily:"Space Grotesk,sans-serif"}}>假說動態</span></div>
            <div className="flex-1 overflow-y-auto px-3 pb-1">
              {d?.hypothesis_tracks?.filter((h: any) => h.status !== "stable").slice(0, 15).map((h: any, i: number) => (
                <div key={i} className="flex items-center gap-1.5 py-[2px]">
                  <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${h.status === "strengthening" ? "bg-[var(--accent-green)]" : h.status === "weakening" ? "bg-[var(--accent-red)]" : h.status === "new" ? "bg-[var(--accent-orange)]" : "bg-[var(--text-muted)]"}`} />
                  <span className="font-bold text-[10px] w-[55px] truncate">{h.title?.split(" ")[0]}</span>
                  <span className={`text-[10px] font-bold ${h.status === "strengthening" ? "t-g" : h.status === "weakening" ? "t-r" : "t-o"}`}>
                    {h.status === "strengthening" ? "↑增強" : h.status === "weakening" ? "↓減弱" : h.status === "new" ? "⊕新" : "✗失效"}
                  </span>
                  <span className="t-m text-[9px]">{P(h.confidence)}</span>
                </div>
              ))}
              {(!d?.hypothesis_tracks || d.hypothesis_tracks.filter((h: any) => h.status !== "stable").length === 0) && <span className="t-m text-[10px]">全部穩定</span>}
            </div>
          </div>
          {/* ④ 板塊氣氛 */}
          <div className="border-t border-[var(--border-gray)] flex-1 flex flex-col min-h-0">
            <div className="px-3 pt-1.5 pb-1"><span className="font-bold text-[11px] t-s" style={{fontFamily:"Space Grotesk,sans-serif"}}>板塊氣氛</span></div>
            <div className="flex-1 overflow-y-auto px-3 pb-1">
              {(() => {
                const sf: Record<string, number[]> = {};
                (d?.pressures || []).forEach((p: any) => {
                  const v = parseFloat(p.capital_flow_pressure ?? p.net_pressure ?? "0") + parseFloat(p.momentum ?? "0") * 0.3;
                  const s = p.symbol as string;
                  let sec = "";
                  if (s.match(/^(AAPL|MSFT|GOOGL|META|AMZN|CRM|ORCL|ADBE|SNOW|PLTR|DDOG|CRWD|NET|PANW)\./)) sec = "科技";
                  else if (s.match(/^(NVDA|AMD|AVGO|QCOM|TSM|INTC|MU|ASML|ARM)\./)) sec = "半導體";
                  else if (s.match(/^(BABA|NIO|XPEV|PDD|JD|BIDU|LI|TCOM|TME|FUTU|TIGR)\./)) sec = "中概";
                  else if (s.match(/^(JPM|GS|MS|BAC|V|MA)\./)) sec = "金融";
                  else if (s.match(/^(XOM|CVX|OXY|SLB|COP)\./)) sec = "能源";
                  else if (s.match(/^(TSLA|RIVN|GM|F)\./)) sec = "電動車";
                  else if (s.match(/^(UNH|JNJ|LLY|ABBV|PFE|MRK)\./)) sec = "醫療";
                  else if (s.match(/^(SPY|QQQ|IWM|DIA)\./)) sec = "ETF";
                  if (sec) { if (!sf[sec]) sf[sec] = []; sf[sec].push(v); }
                });
                return Object.entries(sf).map(([sec, vals]) => {
                  const avg = vals.reduce((a, b) => a + b, 0) / vals.length;
                  const pctVal = (avg * 100).toFixed(1);
                  return { sec, avg, pctVal };
                }).sort((a, b) => b.avg - a.avg).map((s, i) => (
                  <div key={i} className="flex items-center justify-between py-[2px]">
                    <span className="text-[11px] w-14">{s.sec}</span>
                    <div className="flex-1 h-[4px] bg-[var(--bg-elevated)] rounded-full mx-1.5 overflow-hidden">
                      <div className={`h-full rounded-full ${s.avg > 0 ? "bg-[var(--accent-green)]" : "bg-[var(--accent-red)]"}`} style={{ width: `${Math.min(Math.abs(s.avg) * 300, 100)}%`, marginLeft: s.avg < 0 ? "auto" : undefined }} />
                    </div>
                    <span className={`text-[10px] font-bold w-10 text-right ${C(s.avg)}`}>{s.avg > 0 ? "+" : ""}{s.pctVal}%</span>
                  </div>
                ));
              })()}
            </div>
          </div>
        </div>

        {/* ─── 右：跨市場 + 系統 ─── */}
        <div className="w-[170px] shrink-0 flex flex-col">
          {/* 跨市場 */}
          <div className="px-3 pt-2 pb-1 flex-1 border-b border-[var(--border-gray)]">
            <div className="text-[9px] font-bold t-m tracking-wider mb-1">跨市場</div>
            {d?.cross_market_signals?.slice(0, 5).map((cm: any, i: number) => (
              <div key={i} className="flex flex-col py-0.5">
                <div className="flex items-center gap-1">
                  <span className="w-1 h-1 rounded-full shrink-0 bg-[var(--accent-orange)]" />
                  <span className="text-[10px] t-o font-bold">{cm.us_symbol}←{cm.hk_symbol}</span>
                </div>
                <span className="text-[9px] t-m pl-2.5">信心={P(cm.propagation_confidence)} {cm.time_since_hk_close_minutes}分鐘前</span>
              </div>
            ))}
            {d?.cross_market_anomalies?.slice(0, 3).map((a: any, i: number) => (
              <div key={`a${i}`} className="flex items-center gap-1 py-0.5">
                <span className="w-1 h-1 rounded-full shrink-0 bg-[var(--accent-red)]" />
                <span className="text-[10px] t-r">{a.us_symbol} 方向矛盾</span>
              </div>
            ))}
            {!d?.cross_market_signals?.length && !d?.cross_market_anomalies?.length && <span className="t-m text-[10px]">港股未連線</span>}
          </div>

          {/* 事件 */}
          <div className="px-3 pt-1.5 pb-1 flex-1 border-b border-[var(--border-gray)]">
            <div className="text-[9px] font-bold t-m tracking-wider mb-1">事件</div>
            {d?.events?.filter((e: any) => parseFloat(e.magnitude) < 0.99).slice(0, 6).map((e: any, i: number) => (
              <div key={i} className="flex items-center gap-1 py-px">
                <span className={`w-1 h-1 rounded-full shrink-0 ${parseFloat(e.magnitude) > 0.5 ? "bg-[var(--accent-red)]" : "bg-[var(--accent-orange)]"}`} />
                <span className="text-[9px] t-s truncate">{e.summary?.slice(0, 22)}</span>
              </div>
            ))}
            {(!d?.events || d.events.filter((e: any) => parseFloat(e.magnitude) < 0.99).length === 0) && <span className="t-m text-[10px]">等待盤中...</span>}
          </div>

          {/* 系統 */}
          <div className="px-3 py-2 t-m text-[10px] flex flex-col gap-px">
            <span>{d?.stock_count ?? "—"}隻 · {d?.edge_count ?? "—"}邊</span>
            <span>{d?.hypothesis_count ?? "—"}假說 · {d?.observation_count ?? "—"}觀察</span>
            {(d?.active_positions ?? 0) > 0 && <span className="t-o font-bold">持倉 {d.active_positions}</span>}
          </div>
        </div>
      </div>

      {/* 離線 */}
      {err && !d && (
        <div className="fixed inset-0 flex items-center justify-center bg-black/80 z-50">
          <div className="bg-[var(--bg-card)] border border-[var(--border-gray)] p-8 text-center rounded-lg">
            <div className="text-lg font-bold mb-2" style={{fontFamily:"Space Grotesk,sans-serif"}}>Eden 未連接</div>
            <div className="t-m">請先啟動後端</div>
          </div>
        </div>
      )}

      <style jsx global>{`
        .t-g { color: var(--accent-green); }
        .t-r { color: var(--accent-red); }
        .t-o { color: var(--accent-orange); }
        .t-m { color: var(--text-muted); }
        .t-s { color: var(--text-secondary); }
      `}</style>
    </div>
  );
}
