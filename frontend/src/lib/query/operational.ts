import { QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";

import { fetchOperationalSnapshot } from "@/lib/api/client";
import { LIVE_REFRESH_INTERVAL_MS, liveRefetchInterval } from "@/lib/query/live";
import { useShellStore } from "@/state/shell-store";

export function invalidateOperationalQueries(
  queryClient: QueryClient,
  market: string,
) {
  queryClient.invalidateQueries({ queryKey: ["operational-snapshot", market] });
  queryClient.invalidateQueries({ queryKey: ["operational-navigation", market] });
  queryClient.invalidateQueries({ queryKey: ["operational-neighborhood", market] });
  queryClient.invalidateQueries({ queryKey: ["operational-graph-node", market] });
  queryClient.invalidateQueries({ queryKey: ["operational-history", market] });
}

export function useOperationalSnapshot() {
  const market = useShellStore((s) => s.market);
  const liveRefreshEnabled = useShellStore((s) => s.liveRefreshEnabled);
  return useQuery({
    queryKey: ["operational-snapshot", market],
    queryFn: ({ signal }) => fetchOperationalSnapshot(market, signal),
    refetchInterval: liveRefetchInterval(
      liveRefreshEnabled,
      LIVE_REFRESH_INTERVAL_MS,
    ),
    refetchOnWindowFocus: liveRefreshEnabled,
    staleTime: liveRefreshEnabled ? LIVE_REFRESH_INTERVAL_MS / 2 : Infinity,
  });
}

/** Call this to manually refresh the snapshot. */
export function useRefreshSnapshot() {
  const queryClient = useQueryClient();
  const market = useShellStore((s) => s.market);
  return () => invalidateOperationalQueries(queryClient, market);
}
