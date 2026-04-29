import { useQuery } from "@tanstack/react-query";

import { fetchContextStatus } from "@/lib/api/client";

export function useContextStatus() {
  return useQuery({
    queryKey: ["context-status"],
    queryFn: ({ signal }) => fetchContextStatus(signal),
    refetchInterval: 30_000,
  });
}
