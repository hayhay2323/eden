import type { CSSProperties } from "react";
import { useEffect, useMemo, useState } from "react";

import type { CaseContract, CaseWorkflowState, WorkflowContract } from "@/lib/api/types";
import { useHealthReportQuery } from "@/lib/query/health";
import { useShellStore } from "@/state/shell-store";
import { ObjectChip, WorkbenchCard } from "@/features/desk/object-inspector/shared";
import { useCaseWorkflowActions } from "./use-case-workflow-actions";

function allowedTransitions(currentStage: string | null | undefined): string[] {
  switch (currentStage) {
    case "suggest":
      return ["confirm", "review"];
    case "confirm":
      return ["execute", "review"];
    case "execute":
      return ["monitor", "review"];
    case "monitor":
      return ["review"];
    case "review":
      return [];
    default:
      return ["suggest"];
  }
}

function deriveWorkflowStage(
  caseItem?: CaseContract | null,
  workflow?: WorkflowContract | null,
): string | null {
  if (
    caseItem?.governance_reason_code === "workflow_not_created" ||
    workflow?.governance_reason_code === "workflow_not_created"
  ) {
    return null;
  }

  return caseItem?.workflow_state ?? workflow?.stage ?? null;
}

export function OperatorControlsCard({
  caseItem,
  workflow,
  relatedCases,
}: {
  caseItem?: CaseContract | null;
  workflow?: WorkflowContract | null;
  relatedCases: CaseContract[];
}) {
  const openObject = useShellStore((s) => s.openObject);
  const { data: health } = useHealthReportQuery();
  const resolvedCase = caseItem ?? (relatedCases.length === 1 ? relatedCases[0] : null);
  const stage = deriveWorkflowStage(caseItem, workflow);
  const transitions = allowedTransitions(stage);
  const { transition, assign, queuePin, latestSuccess, latestAction, isPending } = useCaseWorkflowActions(
    resolvedCase?.setup_id ?? null,
  );

  const [owner, setOwner] = useState(resolvedCase?.owner ?? workflow?.owner ?? "");
  const [reviewer, setReviewer] = useState(
    resolvedCase?.reviewer ?? workflow?.reviewer ?? "",
  );
  const [pinLabel, setPinLabel] = useState(
    resolvedCase?.queue_pin ?? workflow?.queue_pin ?? "frontend-review-list",
  );

  useEffect(() => {
    setOwner(resolvedCase?.owner ?? workflow?.owner ?? "");
    setReviewer(resolvedCase?.reviewer ?? workflow?.reviewer ?? "");
    setPinLabel(resolvedCase?.queue_pin ?? workflow?.queue_pin ?? "frontend-review-list");
  }, [
    resolvedCase?.id,
    resolvedCase?.owner,
    resolvedCase?.reviewer,
    resolvedCase?.queue_pin,
    workflow?.id,
    workflow?.owner,
    workflow?.reviewer,
    workflow?.queue_pin,
  ]);

  useEffect(() => {
    if (!latestSuccess) return;
    setOwner(latestSuccess.owner ?? "");
    setReviewer(latestSuccess.reviewer ?? "");
    setPinLabel(latestSuccess.queue_pin ?? "frontend-review-list");
  }, [latestSuccess]);

  const latestError = transition.error ?? assign.error ?? queuePin.error ?? null;

  const statusMessage = useMemo(() => {
    if (latestError instanceof Error) return latestError.message;
    if (latestSuccess) {
      return `${formatActionLabel(latestAction)} → ${latestSuccess.stage}`;
    }
    return null;
  }, [latestError, latestSuccess, latestAction]);

  const capabilityBlocker = useMemo(() => {
    if (health && !health.api.persistence_enabled) {
      return "Writes unavailable: eden-api is running without `persistence`.";
    }
    if (latestError instanceof Error && latestError.message.includes("API scope")) {
      return "Writes unavailable: current API key is read-only.";
    }
    return null;
  }, [health, latestError]);
  const controlsDisabled = isPending || Boolean(capabilityBlocker);

  return (
    <WorkbenchCard title="Operator Controls">
      {!resolvedCase ? (
        <div className="eden-surface-layout__stack">
          <div className="text-dim eden-selection-hint__body">
            Workflow actions are case-scoped. Select a related case to transition or assign it.
          </div>
          <div className="eden-workbench__chip-list">
            {relatedCases.map((item) => (
              <ObjectChip
                key={item.id}
                onClick={() =>
                  openObject({
                    kind: "case",
                    id: item.id,
                    label: item.title,
                  })
                }
              >
                {item.symbol}
              </ObjectChip>
            ))}
          </div>
        </div>
      ) : (
        <div className="eden-operator">
          <div className="eden-workbench__group">
            <div className="eden-workbench__group-title">Stage Transitions</div>
            <div className="eden-workbench__chip-list">
              {transitions.length === 0 ? (
                <div className="eden-empty">No transitions available</div>
              ) : (
                transitions.map((target) => (
                  <button
                    key={target}
                    className="eden-topbar__market-btn"
                    disabled={controlsDisabled}
                    onClick={() => transition.mutate(target)}
                  >
                    {target}
                  </button>
                ))
              )}
            </div>
          </div>

          <div className="eden-workbench__group">
            <div className="eden-workbench__group-title">Assignments</div>
            <div className="eden-operator__grid">
              <label className="eden-operator__field">
                <span className="eden-kicker">OWNER</span>
                <input
                  className="eden-operator__input"
                  value={owner}
                  onChange={(event) => setOwner(event.target.value)}
                  placeholder="owner"
                  disabled={controlsDisabled}
                />
              </label>
              <label className="eden-operator__field">
                <span className="eden-kicker">REVIEWER</span>
                <input
                  className="eden-operator__input"
                  value={reviewer}
                  onChange={(event) => setReviewer(event.target.value)}
                  placeholder="reviewer"
                  disabled={controlsDisabled}
                />
              </label>
            </div>
            <div className="eden-inline-row eden-inline-row--tight">
              <button
                className="eden-topbar__market-btn"
                disabled={controlsDisabled}
                onClick={() =>
                  assign.mutate({
                    owner: owner.trim() || null,
                    reviewer: reviewer.trim() || null,
                  })
                }
              >
                Save Assignments
              </button>
            </div>
          </div>

          <div className="eden-workbench__group">
            <div className="eden-workbench__group-title">Queue Pin</div>
            <label className="eden-operator__field">
              <span className="eden-kicker">LABEL</span>
              <input
                className="eden-operator__input"
                value={pinLabel}
                onChange={(event) => setPinLabel(event.target.value)}
                placeholder="frontend-review-list"
                disabled={controlsDisabled}
              />
            </label>
            <div className="eden-inline-row eden-inline-row--tight">
              <button
                className="eden-topbar__market-btn"
                disabled={controlsDisabled}
                onClick={() =>
                  queuePin.mutate({
                    pinned: true,
                    label: pinLabel.trim() || "frontend-review-list",
                  })
                }
              >
                Pin
              </button>
              <button
                className="eden-topbar__market-btn"
                disabled={controlsDisabled}
                onClick={() => queuePin.mutate({ pinned: false })}
              >
                Clear Pin
              </button>
            </div>
          </div>

          {capabilityBlocker && (
            <div className="eden-operator__capability">
              {capabilityBlocker}
            </div>
          )}

          {statusMessage && (
            <div className={`eden-operator__status${latestError ? " eden-operator__status--error" : ""}`}>
              {statusMessage}
            </div>
          )}

          {latestSuccess && (
            <LatestOperatorAction action={latestAction} state={latestSuccess} />
          )}
        </div>
      )}
    </WorkbenchCard>
  );
}

function LatestOperatorAction({
  action,
  state,
}: {
  action: string | null;
  state: CaseWorkflowState;
}) {
  return (
    <div className="eden-operator__latest">
      <div className="eden-workbench__group-title">Latest Operator Action</div>
      <div className="eden-inline-row eden-inline-row--tight eden-operator__latest-row">
        <span className="eden-tone-badge eden-tone-badge--compact" style={{ "--eden-tone": "var(--eden-cyan)" } as CSSProperties}>
          {formatActionLabel(action)}
        </span>
        <span className="mono text-dim eden-section-meta">
          {state.timestamp ? new Date(state.timestamp).toLocaleString() : "-"}
        </span>
      </div>
      <div className="eden-operator__summary-grid">
        <div className="eden-operator__summary-cell">
          <span className="eden-kicker">STAGE</span>
          <span className="mono">{state.stage}</span>
        </div>
        <div className="eden-operator__summary-cell">
          <span className="eden-kicker">ACTOR</span>
          <span className="mono">{state.actor ?? "-"}</span>
        </div>
        <div className="eden-operator__summary-cell">
          <span className="eden-kicker">OWNER</span>
          <span className="mono">{state.owner ?? "-"}</span>
        </div>
        <div className="eden-operator__summary-cell">
          <span className="eden-kicker">REVIEWER</span>
          <span className="mono">{state.reviewer ?? "-"}</span>
        </div>
      </div>
      <div className="eden-operator__latest-note">
        {state.queue_pin ? `queue pin: ${state.queue_pin}` : "queue pin: -"}
        {state.note ? ` · ${state.note}` : ""}
      </div>
    </div>
  );
}

function formatActionLabel(action: string | null) {
  switch (action) {
    case "transition":
      return "transition";
    case "assignment":
      return "assignment";
    case "queue-pin":
      return "queue pin";
    default:
      return "update";
  }
}
