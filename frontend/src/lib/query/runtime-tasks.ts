import { useQuery } from "@tanstack/react-query";

import { fetchRuntimeTasks } from "@/lib/api/client";
import { LIVE_REFRESH_INTERVAL_MS, liveRefetchInterval } from "@/lib/query/live";
import { useShellStore } from "@/state/shell-store";

export function useRuntimeTasksQuery() {
  const market = useShellStore((s) => s.market);
  const liveRefreshEnabled = useShellStore((s) => s.liveRefreshEnabled);

  return useQuery({
    queryKey: ["runtime-tasks", market],
    queryFn: ({ signal }) => fetchRuntimeTasks({ market }, signal),
    refetchInterval: liveRefetchInterval(liveRefreshEnabled, LIVE_REFRESH_INTERVAL_MS),
    refetchOnWindowFocus: liveRefreshEnabled,
    staleTime: liveRefreshEnabled ? LIVE_REFRESH_INTERVAL_MS / 2 : 15_000,
  });
}
