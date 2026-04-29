import { useMemo } from "react";

import { WorkbenchCard } from "@/features/desk/object-inspector/shared";
import type { OperatorWorkItem } from "@/lib/api/types";
import { useOperatorWorkItemsQuery } from "@/lib/query/operator-work-items";
import { useRuntimeTasksQuery } from "@/lib/query/runtime-tasks";
import { useShellStore } from "@/state/shell-store";

function formatUpdated(value?: string | null): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString();
}

export function RuntimeTasksCard() {
  const { data, status } = useRuntimeTasksQuery();
  const tasks = data ?? [];

  return (
    <WorkbenchCard title="Runtime Tasks">
      <div className="eden-card__header">
        <span className="eden-card__badge eden-card__badge--green">{tasks.length}</span>
      </div>
      {status === "pending" ? (
        <div className="eden-empty">Loading runtime tasks…</div>
      ) : tasks.length === 0 ? (
        <div className="eden-empty">No runtime tasks</div>
      ) : (
        tasks.slice(0, 8).map((task) => (
          <div className="eden-feed-item" key={task.id}>
            <span className="eden-feed-item__kind">{task.status}</span>
            <span className="eden-feed-item__label">
              {task.label}
              {task.detail && (
                <span className="eden-focus-row__subtitle">{task.detail}</span>
              )}
            </span>
            <span className="text-muted mono eden-feed-item__meta">
              {task.kind} · {formatUpdated(task.updated_at)}
            </span>
          </div>
        ))
      )}
    </WorkbenchCard>
  );
}

export function OperatorWorkItemsCard() {
  const openObject = useShellStore((s) => s.openObject);
  const { data, status } = useOperatorWorkItemsQuery();
  const items = data ?? [];
  const sortedItems = useMemo(() => items.slice(0, 10), [items]);

  return (
    <WorkbenchCard title="Operator Queue">
      <div className="eden-card__header">
        <span className="eden-card__badge eden-card__badge--amber">{items.length}</span>
      </div>
      {status === "pending" && items.length === 0 ? (
        <div className="eden-empty">Loading operator queue…</div>
      ) : sortedItems.length === 0 ? (
        <div className="eden-empty">No operator queue pressure</div>
      ) : (
        sortedItems.map((item) => {
          const target =
            item.navigation.self_ref ??
            item.case_ref ??
            item.workflow_ref ??
            item.object_ref ??
            item.source_refs[0] ??
            null;
          return (
            <div
              className={`eden-feed-item${target ? " eden-clickable" : ""}`}
              key={item.id}
              onClick={() => {
                if (!target) return;
                openObject({
                  kind: target.kind,
                  id: target.id,
                  label: target.label,
                });
              }}
            >
              <span className="eden-feed-item__kind">{item.lane}</span>
              <span className="eden-feed-item__label">
                {item.symbol && (
                  <span className="mono eden-feed-item__symbol">{item.symbol}</span>
                )}
                {item.title}
                <span className="eden-focus-row__subtitle">
                  {item.summary}
                  {item.blocker ? ` · ${item.blocker}` : ""}
                </span>
              </span>
              <span className="text-muted mono eden-feed-item__meta">
                {item.execution_policy ?? item.status}
                {item.queue_pin ? ` · pin:${item.queue_pin}` : ""}
              </span>
            </div>
          );
        })
      )}
    </WorkbenchCard>
  );
}
