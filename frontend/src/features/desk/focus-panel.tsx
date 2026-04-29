import { useShellStore } from "@/state/shell-store";
import type { OperationalSnapshot } from "@/lib/api/types";
import { ToneBadge } from "@/features/workbench/primitives";
import { actionCls, compareCasesByOperationalPriority, pct, supportedFamilyKeys } from "./format";

export function FocusPanel({ snap }: { snap: OperationalSnapshot }) {
  const cases = snap.cases;
  const workflows = snap.workflows;
  const macroEvents = snap.macro_events;
  const threads = snap.threads;
  const openObject = useShellStore((s) => s.openObject);
  const supportedFamilies = supportedFamilyKeys(snap.lineage);

  const topCases = [...cases]
    .sort((a, b) => compareCasesByOperationalPriority(a, b, supportedFamilies))
    .slice(0, 5);

  return (
    <div className="eden-card eden-panel-block">
      <div className="eden-card__header">
        <span className="eden-card__title">Eden Focus</span>
        <div className="eden-inline-row eden-inline-row--tight">
          <CountBadge label="cases" count={cases.length} color="var(--eden-cyan)" />
          <CountBadge label="workflows" count={workflows.length} color="var(--eden-green)" />
          <CountBadge label="macro" count={macroEvents.length} color="var(--eden-amber)" />
          <CountBadge label="threads" count={threads.length} color="var(--eden-purple)" />
        </div>
      </div>

      {topCases.length > 0 && (
        <div className="eden-focus-list">
          {topCases.map((c) => (
            <div
              key={c.id}
              className="eden-focus-row"
              onClick={() => openObject({ kind: "case", id: c.id, label: c.title })}
            >
              <span className="mono eden-focus-row__symbol">
                {c.symbol}
              </span>
              <span className={`eden-stage stage--${c.workflow_state}`}>{c.workflow_state}</span>
              <span className="text-dim eden-focus-row__title">
                {c.title}
                {(c.multi_horizon_gate_reason || c.policy_reason) && (
                  <span className="eden-focus-row__subtitle">
                    {c.multi_horizon_gate_reason ?? c.policy_reason}
                  </span>
                )}
              </span>
              <span className={`eden-proposal__action ${actionCls(c.action)} eden-focus-row__action`}>
                {c.action}
              </span>
              <span className="mono eden-focus-row__score">
                {pct(c.confidence)}
              </span>
            </div>
          ))}
        </div>
      )}

      {threads.length > 0 && (
        <div className="eden-focus-events">
          <div className="mono text-muted eden-kicker">
            ACTIVE THREADS
          </div>
          {threads.slice(0, 4).map((thread) => (
            <div
              key={thread.id}
              className="eden-focus-event"
              onClick={() =>
                openObject({
                  kind: "symbol_state",
                  id: thread.thread.symbol,
                  label: thread.thread.headline ?? thread.thread.symbol,
                })
              }
            >
              <span className="eden-focus-event__kind">
                {thread.thread.workflow_next_step ?? thread.thread.status}
              </span>
              {thread.thread.latest_summary ?? thread.thread.headline ?? thread.thread.symbol}
            </div>
          ))}
        </div>
      )}

      {macroEvents.length > 0 && (
        <div className="eden-focus-events">
          <div className="mono text-muted eden-kicker">
            MACRO EVENTS
          </div>
          {macroEvents.slice(0, 3).map((e) => (
            <div
              key={e.id}
              className="eden-focus-event"
              onClick={() =>
                openObject({
                  kind: "macro_event",
                  id: e.id,
                  label: e.summary?.headline ?? e.event.headline,
                })
              }
            >
              <span className="eden-focus-event__kind">
                {e.summary?.event_type ?? e.event.event_type}
              </span>
              {e.summary?.headline ?? e.event.headline}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function CountBadge({ label, count, color }: { label: string; count: number; color: string }) {
  if (count === 0) return null;
  return (
    <ToneBadge tone={color} className="eden-tone-badge--compact">
      {count} {label}
    </ToneBadge>
  );
}
