import { useQuery } from "@tanstack/react-query";

import { fetchOperatorWorkItems } from "@/lib/api/client";
import { LIVE_REFRESH_INTERVAL_MS, liveRefetchInterval } from "@/lib/query/live";
import { useShellStore } from "@/state/shell-store";

export function useOperatorWorkItemsQuery() {
  const market = useShellStore((s) => s.market);
  const liveRefreshEnabled = useShellStore((s) => s.liveRefreshEnabled);

  return useQuery({
    queryKey: ["operator-work-items", market],
    queryFn: ({ signal }) => fetchOperatorWorkItems(market, {}, signal),
    refetchInterval: liveRefetchInterval(liveRefreshEnabled, LIVE_REFRESH_INTERVAL_MS),
    refetchOnWindowFocus: liveRefreshEnabled,
    staleTime: liveRefreshEnabled ? LIVE_REFRESH_INTERVAL_MS / 2 : 15_000,
  });
}
