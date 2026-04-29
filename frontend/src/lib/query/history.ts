import { useQuery } from "@tanstack/react-query";

import { fetchOperationalHistory } from "@/lib/api/client";
import {
  LIVE_DETAIL_REFRESH_INTERVAL_MS,
  liveRefetchInterval,
} from "@/lib/query/live";
import type { OperationalHistoryRef } from "@/lib/api/types";
import { useShellStore } from "@/state/shell-store";

export function useHistoryRefData(
  historyRef: OperationalHistoryRef | null,
  limit = 20,
) {
  const market = useShellStore((s) => s.market);
  const liveRefreshEnabled = useShellStore((s) => s.liveRefreshEnabled);

  return useQuery({
    queryKey: [
      "operational-history",
      market,
      historyRef?.endpoint,
      limit,
    ],
    enabled: Boolean(historyRef?.endpoint),
    queryFn: ({ signal }) =>
      fetchOperationalHistory(historyRef!.endpoint, signal, limit),
    refetchInterval: liveRefetchInterval(
      liveRefreshEnabled,
      LIVE_DETAIL_REFRESH_INTERVAL_MS,
    ),
    refetchOnWindowFocus: liveRefreshEnabled,
  });
}
