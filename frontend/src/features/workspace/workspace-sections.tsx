import { useShellStore } from "@/state/shell-store";
import type { CaseContract, AgentNotice, AgentTransition, WorkflowContract } from "@/lib/api/types";
import type { WorkspaceAssignmentBucket, WorkspaceFilter } from "./use-workspace-view";

function fmt(v: number | null | undefined): string {
  return v != null ? Number(v).toFixed(3) : "-";
}

function actionClass(action: string): string {
  const a = (action || "").toLowerCase();
  if (a.includes("enter") || a.includes("buy")) return "action--enter";
  if (a.includes("monitor") || a.includes("watch")) return "action--monitor";
  if (a.includes("review")) return "action--review";
  if (a.includes("trim") || a.includes("exit")) return "action--trim";
  return "action--ignore";
}

function workflowCaseCount(workflow: {
  case_ids?: string[] | null;
  case_refs?: unknown[] | null;
  relationships?: { cases?: unknown[] | null } | null;
}): number {
  return (
    workflow.case_ids?.length ??
    workflow.case_refs?.length ??
    workflow.relationships?.cases?.length ??
    0
  );
}

function workflowRecommendationCount(workflow: {
  recommendation_ids?: string[] | null;
  recommendation_refs?: unknown[] | null;
  relationships?: { recommendations?: unknown[] | null } | null;
}): number {
  return (
    workflow.recommendation_ids?.length ??
    workflow.recommendation_refs?.length ??
    workflow.relationships?.recommendations?.length ??
    0
  );
}

export function CaseBoard({
  cases,
  filterLabel,
}: {
  cases: CaseContract[];
  filterLabel?: string | null;
}) {
  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Cases</span>
        <span className="eden-inline-row eden-inline-row--tight">
          {filterLabel && (
            <span className="eden-card__badge eden-card__badge--purple">{filterLabel}</span>
          )}
          <span className="eden-card__badge eden-card__badge--cyan">{cases.length}</span>
        </span>
      </div>

      <div className="eden-case-row eden-case-row__header">
        <span>SYMBOL</span>
        <span>STAGE</span>
        <span>TITLE</span>
        <span>ACTION</span>
        <span>CONF</span>
        <span>POLICY</span>
      </div>

      {cases.length === 0 ? (
        <div className="eden-empty">No active cases</div>
      ) : (
        cases.map((c) => <CaseRow key={c.id} c={c} />)
      )}
    </div>
  );
}

function CaseRow({ c }: { c: CaseContract }) {
  const openObject = useShellStore((s) => s.openObject);
  const policyLabel = c.policy_primary || c.governance_reason_code || c.execution_policy || "-";
  const policyDetail = c.multi_horizon_gate_reason || c.policy_reason || c.governance_reason || null;

  return (
    <div className="eden-case-row" onClick={() => openObject({ kind: "case", id: c.id, label: c.title })}>
      <span className="eden-case-row__sym">{c.symbol}</span>
      <span className={`eden-stage stage--${c.workflow_state}`}>{c.workflow_state}</span>
      <span className="text-dim eden-case-row__title eden-text-truncate">
        {c.title}
        {policyDetail && <span className="eden-focus-row__subtitle">{policyDetail}</span>}
      </span>
      <span className={`eden-proposal__action eden-case-row__action ${actionClass(c.action)}`}>
        {c.action || "-"}
      </span>
      <span className="mono eden-case-row__conf">{fmt(c.confidence)}</span>
      <span className="mono text-muted eden-case-row__policy">{policyLabel}</span>
    </div>
  );
}

export function WorkflowStageCard({
  workflows,
}: {
  workflows: WorkflowContract[];
}) {
  const workflowStageCounts = workflows.reduce<Record<string, number>>((acc, workflow) => {
    acc[workflow.stage] = (acc[workflow.stage] ?? 0) + 1;
    return acc;
  }, {});

  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Workflow Stages</span>
        <span className="eden-card__badge eden-card__badge--green">{workflows.length}</span>
      </div>
      {Object.entries(workflowStageCounts).length > 0 ? (
        Object.entries(workflowStageCounts).map(([stage, count]) => (
          <div className="eden-stat" key={stage}>
            <span className="eden-stat__label">
              <span className={`eden-stage stage--${stage}`}>{stage}</span>
            </span>
            <span className="eden-stat__value mono">{count}</span>
          </div>
        ))
      ) : (
        <div className="eden-empty">No workflows</div>
      )}
    </div>
  );
}

export function WorkflowQueueCard({
  workflows,
}: {
  workflows: WorkflowContract[];
}) {
  const openObject = useShellStore((s) => s.openObject);

  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Workflow Queue</span>
        <span className="eden-card__badge eden-card__badge--purple">{workflows.length}</span>
      </div>
      {workflows.length === 0 ? (
        <div className="eden-empty">No workflows</div>
      ) : (
        workflows.map((w) => (
          <div
            className="eden-stat eden-stat--interactive"
            key={w.id}
            onClick={() => openObject({ kind: "workflow", id: w.id, label: w.stage })}
          >
            <span className="eden-stat__label">
              <span className={`eden-stage stage--${w.stage}`}>{w.stage}</span>
            </span>
            <span className="eden-stat__value eden-stat__value--compact text-dim">
              {workflowCaseCount(w)} cases · {workflowRecommendationCount(w)} recs
              {w.owner && ` · ${w.owner}`}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

export function TransitionFeed({ transitions }: { transitions: AgentTransition[] }) {
  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Recent Transitions</span>
        <span className="eden-card__badge eden-card__badge--purple">{transitions.length}</span>
      </div>
      {transitions.length === 0 ? (
        <div className="eden-empty">No transitions</div>
      ) : (
        transitions.slice(0, 15).map((t, i) => (
          <div className="eden-feed-item" key={i}>
            <span className="eden-feed-item__kind">{t.to_state}</span>
            <span className="eden-feed-item__label">
              {t.symbol && <span className="mono eden-feed-item__symbol">{t.symbol}</span>}
              {(t.from_state ?? "-")} → {t.to_state}
            </span>
            <span className="text-muted mono eden-feed-item__meta">
              {t.transition_reason ?? t.title}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

export function NoticesFeed({ notices }: { notices: AgentNotice[] }) {
  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Notices</span>
        <span className="eden-card__badge eden-card__badge--amber">{notices.length}</span>
      </div>
      {notices.length === 0 ? (
        <div className="eden-empty">No notices</div>
      ) : (
        notices.slice(0, 15).map((n, i) => (
          <div className="eden-feed-item" key={i}>
            <span className="eden-feed-item__kind">{n.kind}</span>
            <span className="eden-feed-item__label">
              {n.symbol && <span className="mono eden-feed-item__symbol">{n.symbol}</span>}
              {n.summary}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

export function ReviewQueueCard({
  cases,
  active,
  onSelect,
}: {
  cases: CaseContract[];
  active: boolean;
  onSelect: () => void;
}) {
  const openObject = useShellStore((s) => s.openObject);

  return (
    <div className={`eden-card${active ? " eden-card--active" : ""}`}>
      <div className="eden-card__header">
        <button className="eden-card__title-button" onClick={onSelect}>Review Queue</button>
        <span className="eden-card__badge eden-card__badge--amber">{cases.length}</span>
      </div>
      {cases.length === 0 ? (
        <div className="eden-empty">No cases in review</div>
      ) : (
        cases.slice(0, 8).map((item) => (
          <div
            key={item.id}
            className="eden-feed-item eden-clickable"
            onClick={() => openObject({ kind: "case", id: item.id, label: item.title })}
          >
            <span className="eden-feed-item__kind">{item.symbol}</span>
            <span className="eden-feed-item__label">{item.title}</span>
            <span className="text-muted mono eden-feed-item__meta">
              {item.reviewer ?? item.owner ?? "unassigned"}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

export function PinnedQueueCard({
  cases,
  active,
  onSelect,
}: {
  cases: CaseContract[];
  active: boolean;
  onSelect: () => void;
}) {
  const openObject = useShellStore((s) => s.openObject);

  return (
    <div className={`eden-card${active ? " eden-card--active" : ""}`}>
      <div className="eden-card__header">
        <button className="eden-card__title-button" onClick={onSelect}>Pinned Queue</button>
        <span className="eden-card__badge eden-card__badge--purple">{cases.length}</span>
      </div>
      {cases.length === 0 ? (
        <div className="eden-empty">No pinned cases</div>
      ) : (
        cases.slice(0, 8).map((item) => (
          <div
            key={item.id}
            className="eden-feed-item eden-clickable"
            onClick={() => openObject({ kind: "case", id: item.id, label: item.title })}
          >
            <span className="eden-feed-item__kind">{item.queue_pin ?? "pin"}</span>
            <span className="eden-feed-item__label">{item.symbol} · {item.title}</span>
          </div>
        ))
      )}
    </div>
  );
}

export function AssignmentBoard({
  owners,
  reviewers,
  activeFilter,
  onSelectFilter,
}: {
  owners: WorkspaceAssignmentBucket[];
  reviewers: WorkspaceAssignmentBucket[];
  activeFilter: WorkspaceFilter;
  onSelectFilter: (filter: WorkspaceFilter) => void;
}) {
  return (
    <div className="eden-card">
      <div className="eden-card__header">
        <span className="eden-card__title">Assignments</span>
        <span className="eden-card__badge eden-card__badge--green">
          {owners.length + reviewers.length}
        </span>
      </div>

      <div className="eden-workbench__group">
        <div className="eden-workbench__group-title">Owners</div>
        {owners.slice(0, 6).map((bucket) => (
          <div
            className={`eden-stat eden-stat--interactive${activeFilter.kind === "owner" && activeFilter.label === bucket.label ? " eden-stat--active" : ""}`}
            key={`owner:${bucket.label}`}
            onClick={() => onSelectFilter({ kind: "owner", label: bucket.label })}
          >
            <span className="eden-stat__label">{bucket.label}</span>
            <span className="eden-stat__value mono">{bucket.count}</span>
          </div>
        ))}
      </div>

      <div className="eden-workbench__group">
        <div className="eden-workbench__group-title">Reviewers</div>
        {reviewers.slice(0, 6).map((bucket) => (
          <div
            className={`eden-stat eden-stat--interactive${activeFilter.kind === "reviewer" && activeFilter.label === bucket.label ? " eden-stat--active" : ""}`}
            key={`reviewer:${bucket.label}`}
            onClick={() => onSelectFilter({ kind: "reviewer", label: bucket.label })}
          >
            <span className="eden-stat__label">{bucket.label}</span>
            <span className="eden-stat__value mono">{bucket.count}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
