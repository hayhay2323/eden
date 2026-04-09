import { useMemo } from "react";

import type { CaseContract, OperationalSnapshot } from "@/lib/api/types";
import {
  compareCasesByOperationalPriority,
  supportedFamilyKeys,
} from "@/features/desk/format";

export interface WorkspaceAssignmentBucket {
  label: string;
  count: number;
}

export type WorkspaceFilter =
  | { kind: "all" }
  | { kind: "review" }
  | { kind: "pinned" }
  | { kind: "owner"; label: string }
  | { kind: "reviewer"; label: string };

export function useWorkspaceView(snapshot: OperationalSnapshot | undefined) {
  const cases = useMemo(() => {
    if (!snapshot) return [];
    const supportedFamilies = supportedFamilyKeys(snapshot.lineage);
    return [...snapshot.cases].sort((left, right) =>
      compareCasesByOperationalPriority(left, right, supportedFamilies),
    );
  }, [snapshot]);

  const reviewCaseCount = useMemo(
    () => cases.filter((item) => item.workflow_state === "review").length,
    [cases],
  );

  const enterCaseCount = useMemo(
    () => cases.filter((item) => item.action.toLowerCase().includes("enter")).length,
    [cases],
  );

  const reviewCases = useMemo(
    () => cases.filter((item) => item.workflow_state === "review"),
    [cases],
  );

  const pinnedCases = useMemo(
    () => cases.filter((item) => Boolean(item.queue_pin)),
    [cases],
  );

  const ownerBuckets = useMemo(() => groupAssignments(cases, "owner"), [cases]);
  const reviewerBuckets = useMemo(() => groupAssignments(cases, "reviewer"), [cases]);

  return {
    cases,
    reviewCaseCount,
    enterCaseCount,
    reviewCases,
    pinnedCases,
    ownerBuckets,
    reviewerBuckets,
  };
}

export function filterWorkspaceCases(
  cases: CaseContract[],
  filter: WorkspaceFilter,
) {
  switch (filter.kind) {
    case "all":
      return cases;
    case "review":
      return cases.filter((item) => item.workflow_state === "review");
    case "pinned":
      return cases.filter((item) => Boolean(item.queue_pin));
    case "owner":
      return cases.filter((item) => (item.owner?.trim() || "unassigned") === filter.label);
    case "reviewer":
      return cases.filter((item) => (item.reviewer?.trim() || "unassigned") === filter.label);
    default:
      return cases;
  }
}

function groupAssignments(
  cases: CaseContract[],
  key: "owner" | "reviewer",
): WorkspaceAssignmentBucket[] {
  const counts = new Map<string, number>();

  for (const item of cases) {
    const label = item[key]?.trim() || "unassigned";
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }

  return Array.from(counts.entries())
    .map(([label, count]) => ({ label, count }))
    .sort((left, right) => {
      if (right.count !== left.count) return right.count - left.count;
      return left.label.localeCompare(right.label);
    });
}
