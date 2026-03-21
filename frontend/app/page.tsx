"use client";
import { useEffect, useState, useCallback, useMemo } from "react";

const API = process.env.NEXT_PUBLIC_EDEN_API_URL || "http://localhost:8787";
const KEY = process.env.NEXT_PUBLIC_EDEN_API_KEY || "";

/* eslint-disable @typescript-eslint/no-explicit-any */

const P = (v: any) => { const n = parseFloat(v); return isNaN(n) ? "—" : `${(n * 100).toFixed(1)}%`; };
const C = (v: any) => { const n = parseFloat(v); return isNaN(n) || n === 0 ? "t-m" : n > 0 ? "t-g" : "t-r"; };

export default function Dashboard() {
  const [d, setD] = useState<any>(null);
  const [err, setErr] = useState(false);
  const [exp, setExp] = useState<string | null>(null);
  const [mkt, setMkt] = useState<"hk" | "us">("us");
  const [acts, setActs] = useState<Record<string, string>>({});

  const fetch_ = useCallback(async () => {
    try {
      const r = await fetch(`${API}${mkt === "us" ? "/api/us/live" : "/api/live"}`, { headers: { Authorization: `Bearer ${KEY}` }, cache: "no-store" });
      if (!r.ok) throw 0;
      setD(await r.json()); setErr(false);
    } catch { setErr(true); }
  }, [mkt]);

  useEffect(() => { setD(null); setExp(null); fetch_(); const i = setInterval(fetch_, 2000); return () => clearInterval(i); }, [fetch_]);

  const opps = useMemo(() => (d?.tactical_cases || []).slice(0, 5).map((t: any) => {
    const s = t.title?.split(" ")[0] || "";
    const dims = d?.top_signals?.find((ts: any) => ts.symbol === s);
    const causal = d?.causal_leaders?.find((c: any) => c.symbol === s);
    return { ...t, s, chain: d?.backward_chains?.find((c: any) => c.symbol === s), pr: d?.pressures?.find((p: any) => p.symbol === s), dims, causal };
  }), [d]);

  const movers = useMemo(() => (d?.convergence_scores || d?.top_signals || []).slice(0, 8).map((c: any) => {
    const s = c.symbol, v = parseFloat(c.composite || c.dimension_composite || "0");
    return { s, v, why: d?.backward_chains?.find((ch: any) => ch.symbol === s)?.conclusion || "" };
  }), [d]);

  const flows = useMemo(() => (d?.pressures || []).slice(0, 8).map((p: any) => ({
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
          {lin.map((l, i) => <span key={i} className="t-m">{l.template.replace("_continuation","").replace("_positioning","")} <b className={C(l.hit_rate)}>{P(l.hit_rate)}</b></span>)}
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
              const reason = o.chain?.conclusion || o.entry_rationale || "";
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
                          <span className="text-[9px] t-m font-bold tracking-wider">維度收斂</span>
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
                          <div className="text-[9px] t-s truncate">{o.title?.split(" — ")[1] || "延續"}</div>
                        </div>
                        <div className="flex-1 bg-[var(--accent-red-20)] rounded p-1.5">
                          <div className="text-[9px] t-r font-bold">反面假說</div>
                          <div className="text-[11px] font-bold">{P(1 - parseFloat(o.confidence))}</div>
                          <div className="text-[9px] t-s truncate">{o.title?.split(" — ")[1]?.replace("Continuation","Reversal").replace("Positioning","Fakeout") || "反轉"}</div>
                        </div>
                      </div>

                      {/* ③ 證據鏈 */}
                      {o.chain?.evidence?.slice(0, 4).map((e: any, j: number) => (
                        <div key={j} className="flex justify-between"><span className="t-s text-[11px]">{e.description}</span><b className={C(e.direction)}>{P(e.weight)}</b></div>
                      ))}

                      {/* ④ 壓力 + 因果 leader */}
                      <div className="flex gap-3 t-m text-[10px] flex-wrap">
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
                      {d?.edge_count && <div className="text-[10px] t-m">圖譜: {d.stock_count}隻股票 · {d.edge_count}條關聯邊 · {d.hypothesis_count}個假說正在競爭</div>}

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
        </div>

        {/* ─── 中：異動 + 資金 ─── */}
        <div className="w-[320px] flex flex-col border-r border-[var(--border-gray)] shrink-0">
          {/* 異動 */}
          <div className="flex-1 flex flex-col min-h-0">
            <div className="px-3 pt-2 pb-1"><span className="font-bold text-[11px] t-s" style={{fontFamily:"Space Grotesk,sans-serif"}}>異動監察</span></div>
            <div className="flex-1 overflow-y-auto px-3 pb-1">
              {movers.map((m: any, i: number) => (
                <div key={i} className="flex items-center gap-2 py-[3px] hover:bg-[var(--bg-elevated)] rounded px-1 -mx-1 cursor-pointer transition-colors">
                  <span className="font-bold w-[70px] truncate">{m.s.replace(".HK", "").replace(".US", "")}</span>
                  <span className={`font-bold w-12 text-right ${C(m.v)}`}>{P(m.v)}</span>
                  <span className="t-m text-[10px] flex-1 truncate">{m.why.replace(m.s + " ", "").slice(0, 35)}</span>
                </div>
              ))}
            </div>
          </div>
          {/* 資金 */}
          <div className="border-t border-[var(--border-gray)] flex-1 flex flex-col min-h-0">
            <div className="px-3 pt-2 pb-1"><span className="font-bold text-[11px] t-s" style={{fontFamily:"Space Grotesk,sans-serif"}}>資金動向</span></div>
            <div className="flex-1 flex px-3 pb-2 gap-2 min-h-0">
              <div className="flex-1 bg-[var(--accent-green-10)] rounded p-2 overflow-y-auto">
                <div className="text-[9px] font-bold t-g mb-1">流入 ▲</div>
                {flows.filter((f: any) => f.f > 0).slice(0, 5).map((f: any, i: number) => (
                  <div key={i} className="flex justify-between py-px">
                    <span className="text-[11px]">{f.s.replace(".HK","").replace(".US","")}</span>
                    <span className="font-bold t-g text-[11px]">+{P(f.f)}</span>
                  </div>
                ))}
                {flows.filter((f: any) => f.f > 0).length === 0 && <span className="t-m">暫無</span>}
              </div>
              <div className="flex-1 bg-[var(--accent-red-20)] rounded p-2 overflow-y-auto">
                <div className="text-[9px] font-bold t-r mb-1">流出 ▼</div>
                {flows.filter((f: any) => f.f < 0).slice(0, 5).map((f: any, i: number) => (
                  <div key={i} className="flex justify-between py-px">
                    <span className="text-[11px]">{f.s.replace(".HK","").replace(".US","")}</span>
                    <span className="font-bold t-r text-[11px]">{P(f.f)}</span>
                  </div>
                ))}
                {flows.filter((f: any) => f.f < 0).length === 0 && <span className="t-m">暫無</span>}
              </div>
            </div>
          </div>
        </div>

        {/* ─── 右：事件 + 板塊 + 系統 ─── */}
        <div className="w-[200px] shrink-0 flex flex-col overflow-y-auto">
          {/* 板塊 */}
          <div className="px-3 pt-2 pb-1 border-b border-[var(--border-gray)]">
            <div className="text-[9px] font-bold t-m tracking-wider mb-1">板塊</div>
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
                if (sec) { if (!sf[sec]) sf[sec] = []; sf[sec].push(v); }
              });
              return Object.entries(sf).map(([sec, vals]) => {
                const avg = vals.reduce((a, b) => a + b, 0) / vals.length;
                return { sec, avg };
              }).sort((a, b) => b.avg - a.avg).map((s, i) => (
                <div key={i} className="flex justify-between py-px">
                  <span className="text-[11px]">{s.sec}</span>
                  <span className={`text-[11px] font-bold ${C(s.avg)}`}>{s.avg > 0 ? "▲" : s.avg < 0 ? "▼" : "—"}</span>
                </div>
              ));
            })()}
          </div>

          {/* 事件流 */}
          <div className="px-3 pt-2 pb-1 flex-1">
            <div className="text-[9px] font-bold t-m tracking-wider mb-1">事件</div>
            {d?.hypothesis_tracks?.filter((h: any) => h.status === "strengthening" || h.status === "weakening").slice(0, 4).map((h: any, i: number) => (
              <div key={`h${i}`} className="flex items-center gap-1 py-px">
                <span className={`w-1 h-1 rounded-full shrink-0 ${h.status === "strengthening" ? "bg-[var(--accent-green)]" : "bg-[var(--accent-red)]"}`} />
                <span className="text-[10px] truncate">{h.title?.split(" ")[0]} {h.status === "strengthening" ? "↑" : "↓"}</span>
              </div>
            ))}
            {d?.cross_market_signals?.slice(0, 3).map((cm: any, i: number) => (
              <div key={`c${i}`} className="flex items-center gap-1 py-px">
                <span className="w-1 h-1 rounded-full shrink-0 bg-[var(--accent-orange)]" />
                <span className="text-[10px] t-o truncate">{cm.us_symbol}←{cm.hk_symbol}</span>
              </div>
            ))}
            {d?.events?.filter((e: any) => parseFloat(e.magnitude) < 0.99).slice(0, 3).map((e: any, i: number) => (
              <div key={`e${i}`} className="flex items-center gap-1 py-px">
                <span className="w-1 h-1 rounded-full shrink-0 bg-[var(--text-muted)]" />
                <span className="text-[10px] t-m truncate">{e.summary?.slice(0, 25)}</span>
              </div>
            ))}
            {!d?.hypothesis_tracks?.length && !d?.cross_market_signals?.length && <span className="t-m text-[10px]">等待盤中...</span>}
          </div>

          {/* 系統 */}
          <div className="px-3 py-2 border-t border-[var(--border-gray)] t-m text-[10px] flex flex-col gap-px">
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
