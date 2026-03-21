"use client";
import { useEffect, useState } from "react";
import Link from "next/link";
import { fetchLineage, type LineageStats, type LineageOutcome } from "@/lib/api";

export default function LineagePage() {
  const [stats, setStats] = useState<LineageStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchLineage(300, 10)
      .then(setStats)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, []);

  return (
    <main className="min-h-screen bg-gray-950 text-gray-100 p-8">
      <div className="max-w-6xl mx-auto">
        <Link href="/" className="text-blue-400 text-sm mb-4 block">&larr; Back</Link>
        <h1 className="text-3xl font-bold mb-2">Signal Lineage</h1>
        <p className="text-gray-400 mb-8">Which evidence patterns predict correctly? Self-tracked hit rates from live trading.</p>

        {loading && <p className="text-gray-500">Loading...</p>}
        {error && <p className="text-red-400">Error: {error}</p>}

        {stats && (
          <div className="space-y-8">
            <Section title="Top Evidence Patterns" items={stats.based_on} />

            <OutcomeTable title="Promoted Outcomes (patterns that helped)" outcomes={stats.promoted_outcomes} color="green" />
            <OutcomeTable title="Blocked Outcomes (patterns that hurt)" outcomes={stats.blocked_outcomes} color="red" />
            <OutcomeTable title="Falsified Outcomes (patterns that invalidated)" outcomes={stats.falsified_outcomes} color="yellow" />

            <Section title="Promoted By" items={stats.promoted_by} />
            <Section title="Blocked By" items={stats.blocked_by} />
            <Section title="Falsified By" items={stats.falsified_by} />
          </div>
        )}
      </div>
    </main>
  );
}

function Section({ title, items }: { title: string; items: [string, number][] }) {
  if (!items || items.length === 0) return null;
  return (
    <div>
      <h2 className="text-lg font-semibold mb-3">{title}</h2>
      <div className="flex flex-wrap gap-2">
        {items.map(([label, count]) => (
          <span key={label} className="px-3 py-1 rounded-full bg-gray-800 text-sm">
            {label} <span className="text-gray-500">×{count}</span>
          </span>
        ))}
      </div>
    </div>
  );
}

function OutcomeTable({ title, outcomes, color }: { title: string; outcomes: LineageOutcome[]; color: string }) {
  if (!outcomes || outcomes.length === 0) return null;
  const accent = color === "green" ? "text-green-400" : color === "red" ? "text-red-400" : "text-yellow-400";

  return (
    <div>
      <h2 className={`text-lg font-semibold mb-3 ${accent}`}>{title}</h2>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-gray-500 border-b border-gray-800">
              <th className="text-left py-2 pr-4">Pattern</th>
              <th className="text-right py-2 px-2">Total</th>
              <th className="text-right py-2 px-2">Resolved</th>
              <th className="text-right py-2 px-2">Hits</th>
              <th className="text-right py-2 px-2">Hit Rate</th>
              <th className="text-right py-2 px-2">Mean Return</th>
              <th className="text-right py-2 px-2">Net Return</th>
              <th className="text-right py-2 px-2">MFE</th>
              <th className="text-right py-2 px-2">MAE</th>
              <th className="text-right py-2 px-2">Follow-thru</th>
              <th className="text-right py-2 px-2">Invalidated</th>
              <th className="text-right py-2 px-2">Struct Retain</th>
            </tr>
          </thead>
          <tbody>
            {outcomes.map((o) => (
              <tr key={o.label} className="border-b border-gray-900 hover:bg-gray-900/50">
                <td className="py-2 pr-4 font-mono text-xs">{o.label}</td>
                <td className="text-right py-2 px-2">{o.total}</td>
                <td className="text-right py-2 px-2">{o.resolved}</td>
                <td className="text-right py-2 px-2">{o.hits}</td>
                <td className={`text-right py-2 px-2 ${pctColor(o.hit_rate)}`}>{pct(o.hit_rate)}</td>
                <td className={`text-right py-2 px-2 ${pctColor(o.mean_return)}`}>{pct(o.mean_return)}</td>
                <td className={`text-right py-2 px-2 ${pctColor(o.mean_net_return)}`}>{pct(o.mean_net_return)}</td>
                <td className="text-right py-2 px-2 text-green-400">{pct(o.mean_mfe)}</td>
                <td className="text-right py-2 px-2 text-red-400">{pct(o.mean_mae)}</td>
                <td className="text-right py-2 px-2">{pct(o.follow_through_rate)}</td>
                <td className="text-right py-2 px-2">{pct(o.invalidation_rate)}</td>
                <td className="text-right py-2 px-2">{pct(o.structure_retention_rate)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function pct(v: string): string {
  const n = parseFloat(v);
  if (isNaN(n)) return v;
  return `${(n * 100).toFixed(1)}%`;
}

function pctColor(v: string): string {
  const n = parseFloat(v);
  if (isNaN(n) || n === 0) return "text-gray-400";
  return n > 0 ? "text-green-400" : "text-red-400";
}
