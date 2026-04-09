import { ObjectInspectorPanel } from "@/features/desk/object-inspector-panel";
import { useOperationalSnapshot } from "@/lib/query/operational";
import { useShellStore } from "@/state/shell-store";
import { SelectionHint, SurfaceKpi } from "@/features/workbench/surface-support";
import { WorkspaceCapabilityBanner } from "@/features/workspace/workspace-capability-banner";
import {
  OperatorWorkItemsCard,
  RuntimeTasksCard,
} from "@/features/workspace/operator-runtime-panels";
import {
  AssignmentBoard,
  CaseBoard,
  NoticesFeed,
  PinnedQueueCard,
  ReviewQueueCard,
  TransitionFeed,
  WorkflowQueueCard,
  WorkflowStageCard,
} from "@/features/workspace/workspace-sections";
import {
  filterWorkspaceCases,
  useWorkspaceView,
  type WorkspaceFilter,
} from "@/features/workspace/use-workspace-view";
import { useWorkspaceFilterSearch } from "@/features/workspace/use-workspace-filter-search";

export function WorkspacePage() {
  const { data: snap, status } = useOperationalSnapshot();
  const selectedObject = useShellStore((s) => s.selectedObject);
  const { activeFilter, setActiveFilter } = useWorkspaceFilterSearch();

  if (status === "pending") {
    return <div className="eden-loading">Loading cases...</div>;
  }

  if (status === "error" || !snap) {
    return <div className="eden-loading">Connection failed</div>;
  }

  const {
    cases,
    reviewCaseCount,
    enterCaseCount,
    reviewCases,
    pinnedCases,
    ownerBuckets,
    reviewerBuckets,
  } = useWorkspaceView(snap);
  const filteredCases = filterWorkspaceCases(cases, activeFilter);
  const filterLabel = workspaceFilterLabel(activeFilter);

  return (
    <div className="eden-surface-layout">
      <div className="eden-surface-layout__main">
        <WorkspaceCapabilityBanner />

        <div className="eden-grid eden-grid--4 eden-section-space">
          <SurfaceKpi label="Cases" value={String(cases.length)} tone="var(--eden-cyan)" />
          <SurfaceKpi
            label="Workflows"
            value={String(snap.workflows.length)}
            tone="var(--eden-green)"
          />
          <SurfaceKpi
            label="Enter"
            value={String(enterCaseCount)}
            tone="var(--eden-purple)"
          />
          <SurfaceKpi
            label="Review"
            value={String(reviewCaseCount)}
            tone="var(--eden-amber)"
          />
        </div>

        <div className="eden-grid eden-grid--2-1">
          <CaseBoard cases={filteredCases} filterLabel={filterLabel} />

          <div className="eden-surface-layout__stack">
            <WorkflowStageCard workflows={snap.workflows} />
            <WorkflowQueueCard workflows={snap.workflows} />
            <OperatorWorkItemsCard />
            <RuntimeTasksCard />
          </div>
        </div>

        <div className="eden-grid eden-grid--3 eden-section-space">
          <ReviewQueueCard
            cases={reviewCases}
            active={activeFilter.kind === "review"}
            onSelect={() => toggleWorkspaceFilter(activeFilter, setActiveFilter, { kind: "review" })}
          />
          <PinnedQueueCard
            cases={pinnedCases}
            active={activeFilter.kind === "pinned"}
            onSelect={() => toggleWorkspaceFilter(activeFilter, setActiveFilter, { kind: "pinned" })}
          />
          <AssignmentBoard
            owners={ownerBuckets}
            reviewers={reviewerBuckets}
            activeFilter={activeFilter}
            onSelectFilter={(next) => toggleWorkspaceFilter(activeFilter, setActiveFilter, next)}
          />
        </div>

        <div className="eden-grid eden-grid--2">
          <TransitionFeed transitions={snap.recent_transitions} />
          <NoticesFeed notices={snap.notices} />
        </div>
      </div>

      <div className="eden-surface-layout__side">
        {selectedObject ? (
          <ObjectInspectorPanel />
        ) : (
          <SelectionHint
            eyebrow="Case Board"
            title="Select a case or workflow"
            body="Workspace now stays anchored on cases and workflows. Open any row to inspect relationships, history, and graph context without leaving the board."
          />
        )}
      </div>
    </div>
  );
}

function toggleWorkspaceFilter(
  current: WorkspaceFilter,
  setFilter: (value: WorkspaceFilter) => void,
  next: WorkspaceFilter,
) {
  if (
    current.kind === next.kind &&
    ("label" in current ? current.label : undefined) === ("label" in next ? next.label : undefined)
  ) {
    setFilter({ kind: "all" });
    return;
  }
  setFilter(next);
}

function workspaceFilterLabel(filter: WorkspaceFilter) {
  switch (filter.kind) {
    case "all":
      return null;
    case "review":
      return "review queue";
    case "pinned":
      return "pinned";
    case "owner":
      return `owner:${filter.label}`;
    case "reviewer":
      return `reviewer:${filter.label}`;
    default:
      return null;
  }
}
