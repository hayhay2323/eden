import { useQuery } from "@tanstack/react-query";

import {
  fetchOperationalNavigation,
  fetchOperationalNeighborhood,
} from "@/lib/api/client";
import {
  LIVE_DETAIL_REFRESH_INTERVAL_MS,
  liveRefetchInterval,
} from "@/lib/query/live";
import { useShellStore } from "@/state/shell-store";

export function useSelectedNavigation() {
  const market = useShellStore((s) => s.market);
  const liveRefreshEnabled = useShellStore((s) => s.liveRefreshEnabled);
  const selectedObject = useShellStore((s) => s.selectedObject);

  return useQuery({
    queryKey: [
      "operational-navigation",
      market,
      selectedObject?.kind,
      selectedObject?.id,
    ],
    enabled: Boolean(selectedObject),
    queryFn: ({ signal }) =>
      fetchOperationalNavigation(
        market,
        selectedObject!.kind,
        selectedObject!.id,
        signal,
      ),
    refetchInterval: liveRefetchInterval(
      liveRefreshEnabled,
      LIVE_DETAIL_REFRESH_INTERVAL_MS,
    ),
    refetchOnWindowFocus: liveRefreshEnabled,
  });
}

export function useSelectedNeighborhood() {
  const market = useShellStore((s) => s.market);
  const liveRefreshEnabled = useShellStore((s) => s.liveRefreshEnabled);
  const selectedObject = useShellStore((s) => s.selectedObject);

  return useQuery({
    queryKey: [
      "operational-neighborhood",
      market,
      selectedObject?.kind,
      selectedObject?.id,
    ],
    enabled: Boolean(selectedObject),
    queryFn: ({ signal }) =>
      fetchOperationalNeighborhood(
        market,
        selectedObject!.kind,
        selectedObject!.id,
        signal,
      ),
    refetchInterval: liveRefetchInterval(
      liveRefreshEnabled,
      LIVE_DETAIL_REFRESH_INTERVAL_MS,
    ),
    refetchOnWindowFocus: liveRefreshEnabled,
  });
}
