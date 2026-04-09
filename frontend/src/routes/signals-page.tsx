import { ObjectInspectorPanel } from "@/features/desk/object-inspector-panel";
import { SignalsControls } from "@/features/signals/signals-controls";
import {
  MacroEventBoard,
  SectorFlowBoard,
  SymbolBoard,
} from "@/features/signals/signals-sections";
import { useSignalsView } from "@/features/signals/use-signals-view";
import { SelectionHint, SurfaceKpi } from "@/features/workbench/surface-support";
import { useOperationalSnapshot } from "@/lib/query/operational";
import { useShellStore } from "@/state/shell-store";

export function SignalsPage() {
  const { data: snap, status } = useOperationalSnapshot();
  const selectedObject = useShellStore((s) => s.selectedObject);

  if (status === "pending") {
    return <div className="eden-loading">Loading signals...</div>;
  }

  if (status === "error" || !snap) {
    return <div className="eden-loading">Connection failed</div>;
  }

  const {
    sortKey,
    setSortKey,
    filterSector,
    setFilterSector,
    symbols,
    sectors,
    sectorSupport,
  } = useSignalsView(snap);

  return (
    <div className="eden-surface-layout">
      <div className="eden-surface-layout__main">
        <div className="eden-grid eden-grid--4 eden-section-space">
          <SurfaceKpi label="Symbols" value={String(symbols.length)} tone="var(--eden-blue)" />
          <SurfaceKpi label="Sectors" value={String(sectors.length)} tone="var(--eden-amber)" />
          <SurfaceKpi
            label="Macro"
            value={String(snap.macro_events.length)}
            tone="var(--eden-purple)"
          />
          <SurfaceKpi
            label="Flows"
            value={String(snap.sidecars.sector_flows.length)}
            tone="var(--eden-cyan)"
          />
        </div>

        <div className="eden-grid eden-grid--2-1 eden-section-space">
          <SectorFlowBoard flows={snap.sidecars.sector_flows} sectorSupport={sectorSupport} />
          <MacroEventBoard events={snap.macro_events} symbolRows={symbols} />
        </div>

        <SignalsControls
          sortKey={sortKey}
          setSortKey={setSortKey}
          filterSector={filterSector}
          setFilterSector={setFilterSector}
          sectors={sectors}
        />

        <SymbolBoard symbols={symbols} />
      </div>

      <div className="eden-surface-layout__side">
        {selectedObject ? (
          <ObjectInspectorPanel />
        ) : (
          <SelectionHint
            eyebrow="Signals"
            title="Select a symbol or macro event"
            body="Signals is now anchored on sectors, macro, and symbols. Open any row to inspect navigation, history, and graph context without leaving the signal board."
          />
        )}
      </div>
    </div>
  );
}
