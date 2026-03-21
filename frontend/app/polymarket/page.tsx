"use client";
import { useEffect, useState } from "react";
import Link from "next/link";
import { fetchPolymarket, type PolymarketSnapshot } from "@/lib/api";

export default function PolymarketPage() {
  const [snapshot, setSnapshot] = useState<PolymarketSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchPolymarket()
      .then(setSnapshot)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, []);

  return (
    <main className="min-h-screen bg-gray-950 text-gray-100 p-8">
      <div className="max-w-4xl mx-auto">
        <Link href="/" className="text-blue-400 text-sm mb-4 block">&larr; Back</Link>
        <h1 className="text-3xl font-bold mb-2">Polymarket Priors</h1>
        <p className="text-gray-400 mb-8">External event probabilities that confirm or contradict Eden&apos;s institutional signals.</p>

        {loading && <p className="text-gray-500">Loading...</p>}
        {error && <p className="text-red-400">Error: {error}</p>}

        {snapshot && (
          <>
            <p className="text-sm text-gray-500 mb-6">Fetched: {new Date(snapshot.fetched_at).toLocaleString()}</p>

            {snapshot.priors.length === 0 ? (
              <p className="text-gray-500">No Polymarket markets configured. Set POLYMARKET_MARKETS env var.</p>
            ) : (
              <div className="space-y-4">
                {snapshot.priors.map((prior) => (
                  <div
                    key={prior.slug}
                    className={`p-5 rounded-xl border ${
                      parseFloat(prior.probability) > 0.5
                        ? "border-red-800 bg-red-950/30"
                        : "border-gray-800 bg-gray-900"
                    }`}
                  >
                    <div className="flex items-center justify-between mb-2">
                      <h3 className="font-semibold">{prior.label}</h3>
                      <span className={`text-2xl font-bold ${
                        prior.bias === "risk_off" ? "text-red-400" : "text-green-400"
                      }`}>
                        {(parseFloat(prior.probability) * 100).toFixed(0)}%
                      </span>
                    </div>
                    <p className="text-sm text-gray-400 mb-2">{prior.question}</p>
                    <div className="flex gap-4 text-xs text-gray-500">
                      <span>Bias: <span className={prior.bias === "risk_off" ? "text-red-400" : "text-green-400"}>{prior.bias}</span></span>
                      <span>Active: {prior.active ? "yes" : "no"}</span>
                      <span>Closed: {prior.closed ? "yes" : "no"}</span>
                      {prior.category && <span>Category: {prior.category}</span>}
                    </div>

                    {/* Probability bar */}
                    <div className="mt-3 w-full h-2 bg-gray-800 rounded-full overflow-hidden">
                      <div
                        className={`h-full rounded-full ${
                          prior.bias === "risk_off" ? "bg-red-500" : "bg-green-500"
                        }`}
                        style={{ width: `${parseFloat(prior.probability) * 100}%` }}
                      />
                    </div>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </main>
  );
}
