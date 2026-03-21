"use client";
import { useEffect, useState } from "react";
import Link from "next/link";
import { fetchCausalFlips, type CausalFlip } from "@/lib/api";

export default function CausalPage() {
  const [flips, setFlips] = useState<CausalFlip[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchCausalFlips(100)
      .then(setFlips)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, []);

  return (
    <main className="min-h-screen bg-gray-950 text-gray-100 p-8">
      <div className="max-w-6xl mx-auto">
        <Link href="/" className="text-blue-400 text-sm mb-4 block">&larr; Back</Link>
        <h1 className="text-3xl font-bold mb-2">Causal Reasoning</h1>
        <p className="text-gray-400 mb-8">When the leading causal explanation flips — sudden regime shifts vs gradual erosion.</p>

        {loading && <p className="text-gray-500">Loading...</p>}
        {error && <p className="text-red-400">Error: {error}</p>}

        {flips.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-gray-500 border-b border-gray-800">
                  <th className="text-left py-2 pr-4">Scope</th>
                  <th className="text-right py-2 px-2">Tick</th>
                  <th className="text-left py-2 px-4">Previous Leader</th>
                  <th className="text-center py-2 px-2">Style</th>
                  <th className="text-left py-2 px-4">New Leader</th>
                  <th className="text-right py-2 px-2">Gap</th>
                </tr>
              </thead>
              <tbody>
                {flips.map((flip, i) => (
                  <tr key={`${flip.scope_key}-${flip.tick}-${i}`} className="border-b border-gray-900 hover:bg-gray-900/50">
                    <td className="py-2 pr-4 font-mono text-xs">{flip.scope_key}</td>
                    <td className="text-right py-2 px-2 text-gray-500">#{flip.tick}</td>
                    <td className="py-2 px-4 text-sm text-gray-400 max-w-xs truncate">{flip.prev_leader}</td>
                    <td className="text-center py-2 px-2">
                      <span className={`px-2 py-0.5 rounded text-xs ${
                        flip.style === "sudden"
                          ? "bg-red-900 text-red-300"
                          : "bg-yellow-900 text-yellow-300"
                      }`}>
                        {flip.style}
                      </span>
                    </td>
                    <td className="py-2 px-4 text-sm max-w-xs truncate">{flip.new_leader}</td>
                    <td className="text-right py-2 px-2 text-gray-400">{flip.gap}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {!loading && flips.length === 0 && !error && (
          <p className="text-gray-500">No causal flips recorded yet. Run Eden during market hours to collect data.</p>
        )}
      </div>
    </main>
  );
}
