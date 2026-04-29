import { useEffect, useMemo, useState } from "react";
import { useLocation } from "@tanstack/react-router";

import type {
  LiveLineageMetric,
  LiveTemporalBar,
  OperationalSnapshot,
  SymbolStateContract,
} from "@/lib/api/types";
import { isFamilySupported, normalizedFamilyKey, supportedFamilyKeys } from "@/features/desk/format";

export type SignalsSortKey =
  | "support"
  | "composite"
  | "flow"
  | "momentum"
  | "volume"
  | "symbol";

export interface SignalsSymbolRow {
  sym: SymbolStateContract;
  supportState: "supported" | "gated" | "unknown";
  supportReason?: string | null;
  family?: string | null;
  bars: LiveTemporalBar[];
  lineage: LiveLineageMetric[];
}

export interface SignalsSectorSupport {
  supported: number;
  gated: number;
  unknown: number;
}

export function useSignalsView(snapshot: OperationalSnapshot | undefined) {
  const location = useLocation({
    select: (value) => ({
      pathname: value.pathname,
      searchStr: value.searchStr,
      hash: value.hash,
    }),
  });
  const [sortKey, setSortKey] = useState<SignalsSortKey>(() =>
    parseSignalsSortKey(location.searchStr),
  );
  const [filterSector, setFilterSector] = useState<string | null>(() =>
    parseSignalsSector(location.searchStr),
  );

  useEffect(() => {
    const nextSort = parseSignalsSortKey(location.searchStr);
    const nextSector = parseSignalsSector(location.searchStr);
    setSortKey((current) => (current === nextSort ? current : nextSort));
    setFilterSector((current) => (current === nextSector ? current : nextSector));
  }, [location.searchStr]);

  useEffect(() => {
    if (typeof window === "undefined") return;

    const nextSearch = buildSignalsSearch(sortKey, filterSector);
    const nextUrl = `${location.pathname}${nextSearch}${location.hash || ""}`;
    const currentUrl = `${window.location.pathname}${window.location.search}${window.location.hash}`;

    if (nextUrl !== currentUrl) {
      window.history.replaceState(window.history.state, "", nextUrl);
    }
  }, [filterSector, location.hash, location.pathname, sortKey]);

  const symbols = useMemo(() => {
    if (!snapshot) return [];
    let list = [...snapshot.symbols];
    const supportedFamilies = supportedFamilyKeys(snapshot.lineage);
    const barsBySymbol = new Map<string, LiveTemporalBar[]>();
    for (const bar of snapshot.temporal_bars ?? []) {
      const items = barsBySymbol.get(bar.symbol) ?? [];
      items.push(bar);
      barsBySymbol.set(bar.symbol, items);
    }
    const casesBySymbol = new Map<string, typeof snapshot.cases>();
    for (const item of snapshot.cases) {
      const items = casesBySymbol.get(item.symbol) ?? [];
      items.push(item);
      casesBySymbol.set(item.symbol, items);
    }

    if (filterSector) {
      list = list.filter((item) => item.sector === filterSector);
    }

    const rows = list.map((sym) => {
      const symbolCases = casesBySymbol.get(sym.symbol) ?? [];
      const preferredCase = [...symbolCases].sort((left, right) => {
        const leftBlocked = Boolean(left.multi_horizon_gate_reason);
        const rightBlocked = Boolean(right.multi_horizon_gate_reason);
        if (leftBlocked !== rightBlocked) return leftBlocked ? 1 : -1;
        return (right.confidence ?? 0) - (left.confidence ?? 0);
      })[0];
      const family = preferredCase?.thesis_family ?? null;
      const normalizedFamily = normalizedFamilyKey(family);
      const lineage = (snapshot.lineage ?? []).filter((item) => {
        const candidate = normalizedFamilyKey(item.template);
        return (
          normalizedFamily.length > 0 &&
          (candidate === normalizedFamily ||
            candidate.includes(normalizedFamily) ||
            normalizedFamily.includes(candidate))
        );
      });
      const supportState: SignalsSymbolRow["supportState"] =
        preferredCase && family && isFamilySupported(normalizedFamily, supportedFamilies)
          ? "supported"
          : preferredCase && preferredCase.multi_horizon_gate_reason
            ? "gated"
            : "unknown";
      return {
        sym,
        supportState,
        supportReason: preferredCase?.multi_horizon_gate_reason ?? preferredCase?.policy_reason ?? null,
        family,
        bars: (barsBySymbol.get(sym.symbol) ?? []).slice(0, 4),
        lineage,
      } satisfies SignalsSymbolRow;
    });

    rows.sort((left, right) => {
      const leftSignal = left.sym.state.signal;
      const rightSignal = right.sym.state.signal;
      const leftSupportRank = left.supportState === "supported" ? 2 : left.supportState === "gated" ? 1 : 0;
      const rightSupportRank = right.supportState === "supported" ? 2 : right.supportState === "gated" ? 1 : 0;
      switch (sortKey) {
        case "support":
          if (rightSupportRank !== leftSupportRank) return rightSupportRank - leftSupportRank;
          return (right.lineage[0]?.hit_rate ?? 0) - (left.lineage[0]?.hit_rate ?? 0);
        case "composite":
          if (rightSupportRank !== leftSupportRank) return rightSupportRank - leftSupportRank;
          return Math.abs(rightSignal?.composite ?? 0) - Math.abs(leftSignal?.composite ?? 0);
        case "flow":
          if (rightSupportRank !== leftSupportRank) return rightSupportRank - leftSupportRank;
          return (
            Math.abs(rightSignal?.capital_flow_direction ?? 0) -
            Math.abs(leftSignal?.capital_flow_direction ?? 0)
          );
        case "momentum":
          if (rightSupportRank !== leftSupportRank) return rightSupportRank - leftSupportRank;
          return (
            Math.abs(rightSignal?.price_momentum ?? 0) -
            Math.abs(leftSignal?.price_momentum ?? 0)
          );
        case "volume":
          if (rightSupportRank !== leftSupportRank) return rightSupportRank - leftSupportRank;
          return (
            Math.abs(rightSignal?.volume_profile ?? 0) -
            Math.abs(leftSignal?.volume_profile ?? 0)
          );
        case "symbol":
          return left.sym.symbol.localeCompare(right.sym.symbol);
        default:
          return 0;
      }
    });

    return rows;
  }, [snapshot, sortKey, filterSector]);

  const sectors = useMemo(() => {
    if (!snapshot) return [];
    const set = new Set<string>();
    snapshot.symbols.forEach((item) => {
      if (item.sector) set.add(item.sector);
    });
    return Array.from(set).sort();
  }, [snapshot]);

  const sectorSupport = useMemo(() => {
    const stats = new Map<string, SignalsSectorSupport>();
    for (const row of symbols) {
      const sector = row.sym.sector ?? "Unclassified";
      const current = stats.get(sector) ?? {
        supported: 0,
        gated: 0,
        unknown: 0,
      };
      current[row.supportState] += 1;
      stats.set(sector, current);
    }
    return stats;
  }, [symbols]);

  return {
    sortKey,
    setSortKey,
    filterSector,
    setFilterSector,
    symbols,
    sectors,
    sectorSupport,
  };
}

function parseSignalsSortKey(searchStr: string): SignalsSortKey {
  const params = new URLSearchParams(searchStr);
  const raw = params.get("sort");
  return isSignalsSortKey(raw) ? raw : "composite";
}

function parseSignalsSector(searchStr: string): string | null {
  const params = new URLSearchParams(searchStr);
  const raw = params.get("sector");
  return raw && raw.trim().length > 0 ? raw.trim() : null;
}

function buildSignalsSearch(sortKey: SignalsSortKey, filterSector: string | null) {
  const params = new URLSearchParams();

  if (sortKey !== "composite") {
    params.set("sort", sortKey);
  }

  if (filterSector && filterSector.trim().length > 0) {
    params.set("sector", filterSector.trim());
  }

  const search = params.toString();
  return search.length > 0 ? `?${search}` : "";
}

function isSignalsSortKey(value: string | null): value is SignalsSortKey {
  return (
    value === "support" ||
    value === "composite" ||
    value === "flow" ||
    value === "momentum" ||
    value === "volume" ||
    value === "symbol"
  );
}
