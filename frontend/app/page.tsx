"use client";
import { useEffect, useState, useCallback, useMemo } from "react";

const API_BASE = process.env.NEXT_PUBLIC_EDEN_API_URL || "http://localhost:8787";
const API_KEY = process.env.NEXT_PUBLIC_EDEN_API_KEY || "";

type Market = "hk" | "us";

/* eslint-disable @typescript-eslint/no-explicit-any */

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

export default function Dashboard() {
  const [data, setData] = useState<any>(null);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [market, setMarket] = useState<Market>("us");
  const [actions, setActions] = useState<Record<string, string>>({});

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
    setData(null); setExpanded(null);
    fetchLive();
    const iv = setInterval(fetchLive, 2000);
    return () => clearInterval(iv);
  }, [fetchLive]);

  // ── Derived data ──

  const opportunities = useMemo(() => {
    if (!data?.tactical_cases) return [];
    return data.tactical_cases.slice(0, 6).map((tc: any) => {
      const sym = tc.title?.split(" ")[0] || "";
      const chain = data.backward_chains?.find((c: any) => c.symbol === sym);
      const pressure = data.pressures?.find((p: any) => p.symbol === sym);
      const causal = data.causal_leaders?.find((c: any) => c.symbol === sym);
      return { ...tc, sym, chain, pressure, causal };
    });
  }, [data]);

  const movers = useMemo(() => {
    const cs = data?.convergence_scores || data?.top_signals || [];
    return cs.slice(0, 8).map((c: any) => {
      const sym = c.symbol;
      const comp = parseFloat(c.composite || c.dimension_composite || "0");
      const chain = data?.backward_chains?.find((ch: any) => ch.symbol === sym);
      return { sym, comp, reason: chain?.conclusion || "" };
    });
  }, [data]);

  const flows = useMemo(() => {
    return (data?.pressures || []).slice(0, 6).map((p: any) => ({
      sym: p.symbol,
      flow: parseFloat(p.capital_flow_pressure ?? p.net_pressure ?? "0"),
      mom: parseFloat(p.momentum ?? "0"),
      dur: p.pressure_duration,
      acc: p.accelerating,
    }));
  }, [data]);

  const lineage: any[] = useMemo(() => {
    if (!data?.lineage) return [];
    if (Array.isArray(data.lineage)) return data.lineage;
    return data.lineage.by_template || [];
  }, [data]);

  const hitRate = data?.scorecard?.hit_rate ? parseFloat(data.scorecard.hit_rate) : null;
  const stressVal = data?.stress?.composite_stress ? parseFloat(data.stress.composite_stress) : null;
  const consensusVal = data?.stress?.momentum_consensus ? parseFloat(data.stress.momentum_consensus) : data?.stress?.sector_synchrony ? parseFloat(data.stress.sector_synchrony) : null;

  const handleAction = (sym: string, action: string) => {
    setActions(prev => ({ ...prev, [sym]: action }));
  };

  return (
    <div className="h-full flex flex-col">
      {/* ── 頂欄：一句話看大盤 ── */}
      <div className="h-10 bg-[var(--bg-sidebar)] border-b border-[var(--border-gray)] flex items-center px-6 justify-between shrink-0">
        <div className="flex items-center gap-4">
          <span className="font-display text-base font-bold text-[var(--accent-green)]">EDEN</span>
          <div className="flex">
            <button onClick={() => setMarket("hk")} className={`font-mono-eden text-[10px] px-3 py-1 transition-all ${market === "hk" ? "bg-[var(--accent-green-10)] text-[var(--accent-green)] font-bold border border-[var(--accent-green)]/40" : "text-[var(--text-muted)] border border-[var(--border-gray)]"}`}>港股</button>
            <button onClick={() => setMarket("us")} className={`font-mono-eden text-[10px] px-3 py-1 transition-all ${market === "us" ? "bg-[var(--accent-green-10)] text-[var(--accent-green)] font-bold border border-[var(--accent-green)]/40" : "text-[var(--text-muted)] border border-[var(--border-gray)]"}`}>美股</button>
          </div>
          <span className="font-mono-eden text-[10px] text-[var(--text-muted)]">#{data?.tick ?? "—"} {data?.timestamp ? new Date(data.timestamp).toLocaleTimeString("zh-HK") : ""}</span>
        </div>
        <div className="flex items-center gap-5">
          {stressVal != null && <Metric label="市場壓力" value={pct(stressVal)} color={stressVal > 0.3 ? "text-[var(--accent-red)]" : stressVal > 0.15 ? "text-[var(--accent-orange)]" : "text-[var(--accent-green)]"} />}
          {consensusVal != null && <Metric label="方向共識" value={pct(consensusVal)} color="text-[var(--text-primary)]" />}
          {hitRate != null && <Metric label="系統命中" value={pct(hitRate)} color={hitRate > 0.5 ? "text-[var(--accent-green)]" : hitRate > 0.3 ? "text-[var(--accent-orange)]" : "text-[var(--accent-red)]"} />}
          <div className="flex items-center gap-1.5">
            <div className="w-1.5 h-1.5 rounded-full bg-[var(--accent-green)] animate-pulse" />
            <span className="font-mono-eden text-[8px] font-bold text-[var(--accent-green)]">即時</span>
          </div>
        </div>
      </div>

      {/* ── 主內容：單欄 feed ── */}
      <div className="flex-1 overflow-y-auto">
        <div className="max-w-3xl mx-auto p-4 flex flex-col gap-3">

          {/* ═══ 行動建議 ═══ */}
          <div className="flex items-center justify-between">
            <span className="font-display text-sm font-bold text-[var(--text-primary)]">行動建議</span>
            <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">{opportunities.filter((o: any) => o.action === "enter").length} 個進場信號</span>
          </div>

          {opportunities.length === 0 && (
            <div className="text-center py-8 font-mono-eden text-[11px] text-[var(--text-muted)]">等待信號中...</div>
          )}

          {opportunities.map((opp: any, i: number) => {
            const isExpanded = expanded === opp.sym;
            const acted = actions[opp.sym];
            const reason = opp.chain?.conclusion || opp.entry_rationale || "";
            const family = opp.title?.includes("Momentum") ? "momentum_continuation" : opp.title?.includes("Pre-Market") ? "pre_market_positioning" : "";
            const lin = lineage.find(l => l.template === family);

            return (
              <div key={i}
                className={`border rounded-lg transition-all ${opp.action === "enter" ? "border-[var(--accent-green)]/20 bg-[var(--accent-green-10)]" : "border-[var(--border-gray)] bg-[var(--bg-card)]"} ${isExpanded ? "ring-1 ring-[var(--accent-green)]/30" : ""}`}>
                {/* 卡片主體 — 點擊展開 */}
                <div className="p-3 cursor-pointer" onClick={() => setExpanded(isExpanded ? null : opp.sym)}>
                  {/* 行1：股票 + 行動 + 信心 */}
                  <div className="flex items-center justify-between mb-1.5">
                    <div className="flex items-center gap-2.5">
                      <span className="font-display text-[15px] font-bold tracking-tight">{opp.sym}</span>
                      <span className={`font-mono-eden text-[8px] px-2 py-0.5 rounded-full font-bold ${opp.action === "enter" ? "bg-[var(--accent-green)] text-[var(--bg-page)]" : opp.action === "review" ? "bg-[var(--accent-orange-20)] text-[var(--accent-orange)]" : "bg-[var(--bg-elevated)] text-[var(--text-muted)]"}`}>
                        {opp.action === "enter" ? "建議進場" : opp.action === "review" ? "觀望" : "觀察"}
                      </span>
                    </div>
                    <div className="flex items-center gap-3">
                      <span className="font-mono-eden text-[12px] font-bold">{pct(opp.confidence)}</span>
                      <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">{isExpanded ? "▲" : "▼"}</span>
                    </div>
                  </div>
                  {/* 行2：一句話原因 */}
                  <div className="font-mono-eden text-[10px] text-[var(--text-secondary)] leading-relaxed">
                    {reason.length > 100 ? reason.slice(0, 100) + "..." : reason}
                  </div>
                  {/* 行3：策略命中率 */}
                  {lin && (
                    <div className="font-mono-eden text-[8px] text-[var(--text-muted)] mt-1.5">
                      此策略歷史命中率 <span className={pctColor(lin.hit_rate)}>{pct(lin.hit_rate)}</span>
                      <span className="ml-1">({lin.resolved}筆已驗證)</span>
                    </div>
                  )}
                </div>

                {/* 展開區域 — 完整證據 + 行動按鈕 */}
                {isExpanded && (
                  <div className="border-t border-[var(--border-gray)] p-3 flex flex-col gap-2.5">
                    {/* 證據鏈 */}
                    {opp.chain?.evidence && (
                      <div className="flex flex-col gap-1">
                        <span className="font-mono-eden text-[8px] font-bold text-[var(--text-muted)] tracking-wider">推理證據</span>
                        {opp.chain.evidence.slice(0, 5).map((e: any, j: number) => (
                          <div key={j} className="flex justify-between items-center">
                            <span className="font-mono-eden text-[9px] text-[var(--text-secondary)]">{e.description}</span>
                            <span className={`font-mono-eden text-[9px] font-bold ${pctColor(e.direction)}`}>{pct(e.weight)}</span>
                          </div>
                        ))}
                      </div>
                    )}

                    {/* 壓力 + 因果 */}
                    <div className="flex gap-4 font-mono-eden text-[8px] text-[var(--text-muted)] flex-wrap">
                      {opp.pressure && <>
                        <span>資金={pct(opp.pressure.capital_flow_pressure ?? opp.pressure.net_pressure ?? "0")}</span>
                        <span>動量={pct(opp.pressure.momentum ?? "0")}</span>
                        <span>持續={opp.pressure.pressure_duration}次</span>
                        {opp.pressure.accelerating && <span className="text-[var(--accent-orange)]">↑加速中</span>}
                      </>}
                      {opp.causal && <span>主導: {opp.causal.current_leader} ({opp.causal.leader_streak}次)</span>}
                    </div>

                    {/* 信心細節 */}
                    <div className="flex gap-4 font-mono-eden text-[8px] text-[var(--text-muted)]">
                      <span>信心差距={pct(opp.confidence_gap)}</span>
                      <span>邊際={pct(opp.heuristic_edge)}</span>
                    </div>

                    {/* 行動按鈕 */}
                    {acted ? (
                      <div className="flex items-center justify-center gap-2 py-1">
                        <span className={`font-mono-eden text-[10px] font-bold ${acted === "enter" ? "text-[var(--accent-green)]" : acted === "review" ? "text-[var(--accent-orange)]" : "text-[var(--text-muted)]"}`}>
                          {acted === "enter" ? "✓ 已確認進場" : acted === "review" ? "⟳ 已標記觀望" : "— 已忽略"}
                        </span>
                        <button onClick={(e) => { e.stopPropagation(); setActions(p => { const n = { ...p }; delete n[opp.sym]; return n; }); }}
                          className="font-mono-eden text-[8px] text-[var(--text-muted)] hover:text-[var(--text-primary)] underline">撤回</button>
                      </div>
                    ) : (
                      <div className="flex gap-2">
                        <button onClick={(e) => { e.stopPropagation(); handleAction(opp.sym, "enter"); }}
                          className="flex-1 py-1.5 bg-[var(--accent-green)] font-mono-eden text-[9px] font-bold text-[var(--bg-page)] rounded hover:brightness-110 active:scale-[0.98] transition-all">確認進場</button>
                        <button onClick={(e) => { e.stopPropagation(); handleAction(opp.sym, "review"); }}
                          className="flex-1 py-1.5 border border-[var(--accent-orange)]/40 font-mono-eden text-[9px] font-semibold text-[var(--accent-orange)] rounded hover:bg-[var(--accent-orange-20)] active:scale-[0.98] transition-all">觀望</button>
                        <button onClick={(e) => { e.stopPropagation(); handleAction(opp.sym, "dismiss"); }}
                          className="flex-1 py-1.5 border border-[var(--border-gray)] font-mono-eden text-[9px] text-[var(--text-muted)] rounded hover:text-[var(--text-secondary)] active:scale-[0.98] transition-all">忽略</button>
                      </div>
                    )}
                  </div>
                )}
              </div>
            );
          })}

          {/* ═══ 異動監察 ═══ */}
          {movers.length > 0 && (<>
            <div className="h-px bg-[var(--border-gray)] mt-2" />
            <span className="font-display text-sm font-bold text-[var(--text-muted)]">異動監察</span>
            <div className="bg-[var(--bg-card)] border border-[var(--border-gray)] rounded-lg overflow-hidden">
              {movers.map((m: any, i: number) => (
                <div key={i} className={`flex items-center gap-3 px-3 py-1.5 ${i > 0 ? "border-t border-[var(--border-gray)]" : ""} hover:bg-[var(--bg-elevated)] transition-colors cursor-pointer`}
                  onClick={() => setExpanded(expanded === m.sym ? null : m.sym)}>
                  <span className="font-mono-eden text-[11px] font-bold w-20">{m.sym.replace(".HK", "").replace(".US", "")}</span>
                  <span className={`font-mono-eden text-[11px] font-bold w-14 text-right ${pctColor(m.comp)}`}>{pct(m.comp)}</span>
                  <span className="font-mono-eden text-[9px] text-[var(--text-muted)] flex-1 truncate">{m.reason.replace(m.sym + " ", "")}</span>
                </div>
              ))}
            </div>
          </>)}

          {/* ═══ 資金動向 ═══ */}
          {flows.length > 0 && (<>
            <div className="h-px bg-[var(--border-gray)] mt-2" />
            <span className="font-display text-sm font-bold text-[var(--text-muted)]">資金動向</span>
            <div className="grid grid-cols-2 gap-2">
              {/* 流入 */}
              <div className="bg-[var(--accent-green-10)] border border-[var(--accent-green)]/10 rounded-lg p-2.5">
                <span className="font-mono-eden text-[8px] font-bold text-[var(--accent-green)] tracking-wider">流入 ▲</span>
                {flows.filter((f: any) => f.flow > 0).slice(0, 4).map((f: any, i: number) => (
                  <div key={i} className="flex justify-between mt-1">
                    <span className="font-mono-eden text-[10px] font-semibold">{f.sym.replace(".HK", "").replace(".US", "")}</span>
                    <span className="font-mono-eden text-[10px] font-bold text-[var(--accent-green)]">+{pct(f.flow)}</span>
                  </div>
                ))}
                {flows.filter((f: any) => f.flow > 0).length === 0 && <div className="font-mono-eden text-[9px] text-[var(--text-muted)] mt-1">暫無</div>}
              </div>
              {/* 流出 */}
              <div className="bg-[var(--accent-red-20)] border border-[var(--accent-red)]/10 rounded-lg p-2.5">
                <span className="font-mono-eden text-[8px] font-bold text-[var(--accent-red)] tracking-wider">流出 ▼</span>
                {flows.filter((f: any) => f.flow < 0).slice(0, 4).map((f: any, i: number) => (
                  <div key={i} className="flex justify-between mt-1">
                    <span className="font-mono-eden text-[10px] font-semibold">{f.sym.replace(".HK", "").replace(".US", "")}</span>
                    <span className="font-mono-eden text-[10px] font-bold text-[var(--accent-red)]">{pct(f.flow)}</span>
                  </div>
                ))}
                {flows.filter((f: any) => f.flow < 0).length === 0 && <div className="font-mono-eden text-[9px] text-[var(--text-muted)] mt-1">暫無</div>}
              </div>
            </div>
          </>)}

          {/* ═══ HK 專屬：機構活動 ═══ */}
          {market === "hk" && data?.pair_trades?.length > 0 && (<>
            <div className="h-px bg-[var(--border-gray)] mt-2" />
            <span className="font-display text-sm font-bold text-[var(--text-muted)]">機構活動</span>
            {data.pair_trades.slice(0, 3).map((pt: any, i: number) => (
              <div key={i} className="bg-[var(--bg-card)] border border-[var(--border-gray)] rounded-lg p-2.5">
                <span className="font-mono-eden text-[10px] font-semibold">{pt.institution}</span>
                <div className="flex gap-1.5 mt-1 flex-wrap">
                  {pt.buy_symbols?.map((s: string) => <span key={s} className="font-mono-eden text-[8px] text-[var(--accent-green)] bg-[var(--accent-green-10)] px-1.5 py-0.5 rounded">▲{s}</span>)}
                  {pt.sell_symbols?.map((s: string) => <span key={s} className="font-mono-eden text-[8px] text-[var(--accent-red)] bg-[var(--accent-red-20)] px-1.5 py-0.5 rounded">▼{s}</span>)}
                </div>
              </div>
            ))}
          </>)}

          {/* ═══ HK 專屬：機構撤退 ═══ */}
          {market === "hk" && data?.exoduses?.length > 0 && (<>
            <div className="h-px bg-[var(--border-gray)] mt-2" />
            <span className="font-display text-sm font-bold text-[var(--accent-red)]">機構撤退</span>
            {data.exoduses.slice(0, 3).map((e: any, i: number) => (
              <div key={i} className="font-mono-eden text-[9px] text-[var(--accent-red)]">
                {e.institution} {e.prev_stock_count}→{e.curr_stock_count} (-{e.dropped_count})
              </div>
            ))}
          </>)}

          {/* 底部空間 */}
          <div className="h-4" />
        </div>
      </div>

      {/* ── 底欄：策略表現 ── */}
      {lineage.length > 0 && (
        <div className="h-8 bg-[var(--bg-sidebar)] border-t border-[var(--border-gray)] flex items-center px-6 gap-6 shrink-0">
          {lineage.map((l, i) => (
            <div key={i} className="flex items-center gap-1.5">
              <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">{l.template}</span>
              <span className={`font-mono-eden text-[9px] font-bold ${pctColor(l.hit_rate)}`}>{pct(l.hit_rate)}</span>
              <span className="font-mono-eden text-[8px] text-[var(--text-muted)]">({l.resolved}筆)</span>
            </div>
          ))}
          {data?.active_positions > 0 && (
            <div className="flex items-center gap-1.5 ml-auto">
              <span className="font-mono-eden text-[8px] text-[var(--accent-orange)]">持倉 {data.active_positions}</span>
            </div>
          )}
        </div>
      )}

      {/* 離線 */}
      {error && !data && (
        <div className="fixed inset-0 flex items-center justify-center bg-black/80 z-50">
          <div className="bg-[var(--bg-card)] border border-[var(--border-gray)] p-8 max-w-md text-center rounded-lg">
            <div className="font-display text-xl font-bold mb-2">Eden 未連接</div>
            <div className="font-mono-eden text-sm text-[var(--text-muted)]">請先啟動 Eden 後端</div>
          </div>
        </div>
      )}
    </div>
  );
}

function Metric({ label, value, color }: { label: string; value: string; color: string }) {
  return (
    <div className="flex items-center gap-1.5">
      <span className="font-mono-eden text-[9px] text-[var(--text-muted)]">{label}</span>
      <span className={`font-mono-eden text-[10px] font-bold ${color}`}>{value}</span>
    </div>
  );
}
