import { useQuery } from "@tanstack/react-query";

import { fetchOperationalGraphNode } from "@/lib/api/client";
import {
  LIVE_DETAIL_REFRESH_INTERVAL_MS,
  liveRefetchInterval,
} from "@/lib/query/live";
import { useShellStore } from "@/state/shell-store";

export function useGraphNode(endpoint: string | null, limit = 24) {
  const market = useShellStore((s) => s.market);
  const liveRefreshEnabled = useShellStore((s) => s.liveRefreshEnabled);

  return useQuery({
    queryKey: ["operational-graph-node", market, endpoint, limit],
    enabled: Boolean(endpoint),
    queryFn: ({ signal }) => fetchOperationalGraphNode(endpoint!, signal, limit),
    refetchInterval: liveRefetchInterval(
      liveRefreshEnabled,
      LIVE_DETAIL_REFRESH_INTERVAL_MS,
    ),
    refetchOnWindowFocus: liveRefreshEnabled,
  });
}
