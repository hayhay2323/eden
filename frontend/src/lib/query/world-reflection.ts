import { useQuery } from "@tanstack/react-query";

import {
  fetchAgentWorldReflection,
  type WorldReflectionRequest,
} from "@/lib/api/client";
import { LIVE_REFRESH_INTERVAL_MS, liveRefetchInterval } from "@/lib/query/live";
import { useShellStore } from "@/state/shell-store";

export function useWorldReflection(options: WorldReflectionRequest = { limit: 8 }) {
  const market = useShellStore((s) => s.market);
  const liveRefreshEnabled = useShellStore((s) => s.liveRefreshEnabled);

  return useQuery({
    queryKey: [
      "world-reflection",
      market,
      options.kind ?? null,
      options.direction ?? null,
      options.limit ?? null,
    ],
    queryFn: ({ signal }) => fetchAgentWorldReflection(market, options, signal),
    refetchInterval: liveRefetchInterval(
      liveRefreshEnabled,
      LIVE_REFRESH_INTERVAL_MS,
    ),
    refetchOnWindowFocus: liveRefreshEnabled,
    staleTime: liveRefreshEnabled ? LIVE_REFRESH_INTERVAL_MS / 2 : Infinity,
  });
}
