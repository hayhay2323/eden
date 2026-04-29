import { useQuery } from "@tanstack/react-query";

import { fetchHealthReport } from "@/lib/api/client";

export function useHealthReportQuery() {
  return useQuery({
    queryKey: ["health-report"],
    queryFn: ({ signal }) => fetchHealthReport(signal),
    refetchInterval: 20_000,
  });
}
