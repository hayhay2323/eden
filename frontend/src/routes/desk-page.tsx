import { useOperationalSnapshot } from "@/lib/query/operational";
import { useShellStore } from "@/state/shell-store";
import { ActionsPanel } from "@/features/desk/actions-panel";
import { FocusPanel } from "@/features/desk/focus-panel";
import { MarketSessionPanel } from "@/features/desk/market-session-panel";
import { ObjectInspectorPanel } from "@/features/desk/object-inspector-panel";
import { ReasoningPanel } from "@/features/desk/reasoning-panel";
import { WorldReflectionPanel } from "@/features/desk/world-reflection-panel";
import { SelectionHint, SurfaceKpi } from "@/features/workbench/surface-support";

export function DeskPage() {
  const { data: snap, status } = useOperationalSnapshot();
  const selectedObject = useShellStore((s) => s.selectedObject);

  if (status === "pending") {
    return <div className="eden-loading">Connecting to Eden...</div>;
  }

  if (status === "error" || !snap) {
    return <div className="eden-loading">Connection failed — make sure eden-api is running</div>;
  }

  const focusCount = snap.market_session.focus_symbols.length;
  const recommendationCount = snap.recommendations.length;
  const evidenceCount = snap.sidecars.backward_investigations.filter((item) => item.leading_cause).length;
  const macroCount = snap.macro_events.length;

  return (
    <div className="eden-surface-layout">
      <div className="eden-surface-layout__main">
        <div className="eden-grid eden-grid--4 eden-section-space">
          <SurfaceKpi label="Focus" value={String(focusCount)} tone="var(--eden-cyan)" />
          <SurfaceKpi label="Actions" value={String(recommendationCount)} tone="var(--eden-purple)" />
          <SurfaceKpi label="Evidence" value={String(evidenceCount)} tone="var(--eden-green)" />
          <SurfaceKpi label="Macro" value={String(macroCount)} tone="var(--eden-amber)" />
        </div>

        <MarketSessionPanel snap={snap} />

        <div className="eden-grid eden-grid--2 eden-section-space">
          <FocusPanel snap={snap} />
          <ActionsPanel snap={snap} />
        </div>

        <div className="eden-surface-layout__stack">
          <WorldReflectionPanel />
          <ReasoningPanel snap={snap} />
        </div>
      </div>

      <div className="eden-surface-layout__side">
        {selectedObject ? (
          <ObjectInspectorPanel />
        ) : (
          <SelectionHint
            eyebrow="Market Desk"
            title="Select a focus object"
            body="Desk now keeps the market surface visible while the inspector stays on the right. Open any focus case, recommendation, macro event, or symbol to inspect relationships, history, and graph context without leaving the desk."
          />
        )}
      </div>
    </div>
  );
}
