import { useEffect, useMemo, useState } from "react";

import { buildOperationalGraphNodeEndpoint } from "@/lib/api/client";
import { useGraphNode } from "@/lib/query/graph";
import { useHistoryRefData } from "@/lib/query/history";
import { useSelectedNavigation, useSelectedNeighborhood } from "@/lib/query/navigation";
import { useOperationalSnapshot } from "@/lib/query/operational";
import type {
  CaseContract,
  MacroEventContract,
  RecommendationContract,
  SymbolStateContract,
  WorkflowContract,
} from "@/lib/api/types";
import { useShellStore } from "@/state/shell-store";
import {
  CaseDetail,
  MacroEventDetail,
  RecommendationDetail,
  SymbolDetail,
  WorkflowDetail,
} from "./object-inspector/detail-cards";
import { GraphWorkbench } from "./object-inspector/graph-workbench";
import { HistoryWorkbench } from "./object-inspector/history-workbench";
import { ObjectChip, WorkbenchCard, WorkbenchMeta } from "./object-inspector/shared";
import { OperatorControlsCard } from "@/features/workspace/operator-controls";

export function ObjectInspectorPanel() {
  const selectedObject = useShellStore((s) => s.selectedObject);
  const selectedObjectTrail = useShellStore((s) => s.selectedObjectTrail);
  const closeInspector = useShellStore((s) => s.closeInspector);
  const openObject = useShellStore((s) => s.openObject);
  const jumpToObject = useShellStore((s) => s.jumpToObject);
  const market = useShellStore((s) => s.market);
  const { data: snap } = useOperationalSnapshot();
  const { data: navigation } = useSelectedNavigation();
  const { data: neighborhood } = useSelectedNeighborhood();
  const [activeHistoryEndpoint, setActiveHistoryEndpoint] = useState<string | null>(null);
  const [activeGraphEndpoint, setActiveGraphEndpoint] = useState<string | null>(null);

  const selected = useMemo(() => {
    if (!snap || !selectedObject) return null;
    switch (selectedObject.kind) {
      case "symbol_state":
        return snap.symbols.find((item) => item.symbol === selectedObject.id || item.id === selectedObject.id) ?? null;
      case "case":
        return snap.cases.find((item) => item.id === selectedObject.id) ?? null;
      case "recommendation":
        return snap.recommendations.find((item) => item.id === selectedObject.id) ?? null;
      case "macro_event":
        return snap.macro_events.find((item) => item.id === selectedObject.id) ?? null;
      case "workflow":
        return snap.workflows.find((item) => item.id === selectedObject.id) ?? null;
      default:
        return null;
    }
  }, [snap, selectedObject]);

  const selectedCase =
    selected && "workflow_state" in selected ? (selected as CaseContract) : null;
  const selectedWorkflow =
    selected && "stage" in selected
      ? (selected as WorkflowContract)
      : selectedCase?.workflow_id
        ? snap?.workflows.find((item) => item.id === selectedCase.workflow_id) ?? null
        : null;
  const relatedCases = useMemo(() => {
    if (!snap) return [];
    if (selectedCase) return [selectedCase];
    if (!selectedWorkflow) return [];
    const relatedIds = new Set([
      ...(selectedWorkflow.case_ids ?? []),
      ...selectedWorkflow.case_refs.map((ref) => ref.id),
    ]);
    return snap.cases.filter((item) => relatedIds.has(item.id));
  }, [snap, selectedCase, selectedWorkflow]);

  useEffect(() => {
    setActiveHistoryEndpoint(null);
    setActiveGraphEndpoint(null);
  }, [selectedObject?.kind, selectedObject?.id]);

  if (!selectedObject || !selected) return null;

  const title =
    navigation?.self_ref?.label
    ?? selectedObject.label
    ?? ("symbol" in selected ? selected.symbol : "title" in selected ? selected.title : selectedObject.id);
  const activeHistoryRef =
    navigation?.history.find((ref) => ref.endpoint === activeHistoryEndpoint) ?? null;
  const historyQuery = useHistoryRefData(activeHistoryRef);
  const graphEndpoint = activeGraphEndpoint ?? neighborhood?.graph_ref?.endpoint ?? navigation?.graph?.endpoint ?? null;
  const graphQuery = useGraphNode(graphEndpoint);

  return (
    <div className="eden-card eden-workbench-shell eden-panel-block">
      <div className="eden-workbench__topbar">
        <button className="eden-inspector__back" onClick={closeInspector}>
          {selectedObjectTrail.length > 1 ? "← Back" : "← Close"}
        </button>
        <span className="mono eden-workbench__title">{title}</span>
        <span className="text-dim eden-workbench__kind">
          {selectedObject.kind}
        </span>
      </div>

      {selectedObjectTrail.length > 0 && (
        <div className="eden-workbench__trail">
          {selectedObjectTrail.map((item, index) => {
            const active = index === selectedObjectTrail.length - 1;
            return (
              <ObjectChip
                key={`${item.kind}:${item.id}:${index}`}
                active={active}
                onClick={() => !active && jumpToObject(index)}
              >
                {item.label ?? item.id}
              </ObjectChip>
            );
          })}
        </div>
      )}

      <div className="eden-workbench">
        <div className="eden-workbench__primary">
          <WorkbenchCard title="Overview">
            <WorkbenchMeta
              kind={selectedObject.kind}
              selfId={navigation?.self_ref?.id ?? selectedObject.id}
              relationships={navigation?.relationships.length ?? 0}
              history={navigation?.history.length ?? 0}
            />
            {"state" in selected && <SymbolDetail sym={selected} snapshot={snap} />}
            {"recommendation" in selected && <RecommendationDetail rec={selected} snapshot={snap} />}
            {"workflow_state" in selected && <CaseDetail caseItem={selected} snapshot={snap} />}
            {"event" in selected && <MacroEventDetail eventItem={selected} />}
            {"stage" in selected && <WorkflowDetail workflow={selected} />}
          </WorkbenchCard>
          {(selectedCase || selectedWorkflow) && (
            <OperatorControlsCard
              caseItem={selectedCase}
              workflow={selectedWorkflow}
              relatedCases={relatedCases}
            />
          )}
        </div>

        <div className="eden-workbench__column">
          <WorkbenchCard title="Relationships">
            {navigation && navigation.relationships.length > 0 ? (
              navigation.relationships.map((group) => (
                <div key={group.name} className="eden-workbench__group">
                  <div className="eden-workbench__group-title">
                    {group.name}
                  </div>
                  <div className="eden-workbench__chip-list">
                    {group.refs.map((ref) => {
                      const inTrail = selectedObjectTrail.some(
                        (item) => item.kind === ref.kind && item.id === ref.id,
                      );
                      const active =
                        selectedObject?.kind === ref.kind && selectedObject?.id === ref.id;
                      return (
                        <ObjectChip
                          key={`${group.name}:${ref.id}`}
                          active={active}
                          visited={inTrail && !active}
                          onClick={() =>
                            !active &&
                            openObject({
                              kind: ref.kind,
                              id: ref.id,
                              label: ref.label ?? undefined,
                            })
                          }
                        >
                          {ref.label ?? ref.id}
                        </ObjectChip>
                      );
                    })}
                  </div>
                </div>
              ))
            ) : (
              <div className="eden-empty">No relationships</div>
            )}
          </WorkbenchCard>

          <WorkbenchCard title="History">
            {navigation?.history && navigation.history.length > 0 ? (
              <HistoryWorkbench
                refs={navigation.history}
                activeRef={activeHistoryRef}
                onSelect={setActiveHistoryEndpoint}
                records={historyQuery.data}
                status={historyQuery.status}
                errorMessage={
                  historyQuery.error instanceof Error
                    ? historyQuery.error.message
                    : "Failed to load history"
                }
              />
            ) : (
              <div className="eden-empty">No history</div>
            )}
          </WorkbenchCard>
        </div>

        <div className="eden-workbench__column">
          <WorkbenchCard title="Graph">
            {graphEndpoint ? (
              <GraphWorkbench
                market={market}
                endpoint={graphEndpoint}
                status={graphQuery.status}
                errorMessage={
                  graphQuery.error instanceof Error
                    ? graphQuery.error.message
                    : "Failed to load graph node"
                }
                node={graphQuery.data?.node ?? null}
                links={graphQuery.data?.current_links ?? []}
                events={graphQuery.data?.current_events ?? []}
                onOpenNode={(nodeId) =>
                  setActiveGraphEndpoint(buildOperationalGraphNodeEndpoint(market, nodeId))
                }
              />
            ) : (
              <div className="eden-empty">No graph ref</div>
            )}
          </WorkbenchCard>

          <WorkbenchCard title="Traversal">
            {navigation?.neighborhood_endpoint ? (
              <>
                <div className="eden-stat">
                  <span className="eden-stat__label">Root</span>
                  <span className="eden-stat__value mono">{navigation.self_ref?.id ?? selectedObject.id}</span>
                </div>
                <div className="eden-stat">
                  <span className="eden-stat__label">Neighborhood</span>
                  <span className="eden-stat__value mono eden-workbench__mono-small">
                    {navigation.neighborhood_endpoint}
                  </span>
                </div>
                <div className="eden-stat">
                  <span className="eden-stat__label">Edges</span>
                  <span className="eden-stat__value mono">{navigation.relationships.length}</span>
                </div>
              </>
            ) : (
              <div className="eden-empty">No traversal metadata</div>
            )}
          </WorkbenchCard>
        </div>
      </div>
    </div>
  );
}
