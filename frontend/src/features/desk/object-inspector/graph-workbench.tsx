import type {
  OperationalGraphEventStateRecord,
  OperationalGraphLinkStateRecord,
} from "@/lib/api/types";

import { Stat } from "./shared";

export function GraphWorkbench({
  market,
  endpoint,
  status,
  errorMessage,
  node,
  links,
  events,
  onOpenNode,
}: {
  market: string;
  endpoint: string;
  status: "pending" | "error" | "success";
  errorMessage: string;
  node: {
    node_id: string;
    node_kind: string;
    label: string;
    latest_tick_number: number;
    last_seen_at: string;
  } | null;
  links: OperationalGraphLinkStateRecord[];
  events: OperationalGraphEventStateRecord[];
  onOpenNode: (nodeId: string) => void;
}) {
  const endpointNodeId = decodeURIComponent(endpoint.split("/").pop() ?? "");

  if (status === "pending") {
    return <div className="eden-loading eden-workbench__loading">Loading graph…</div>;
  }

  if (status === "error") {
    return <div className="eden-empty">{errorMessage}</div>;
  }

  return (
    <div className="eden-graph">
      <div className="eden-graph__header">
        <div className="mono eden-graph__node-id">{node?.node_id ?? endpointNodeId}</div>
        <div className="text-dim eden-graph__node-kind">{node?.node_kind ?? "node"}</div>
      </div>

      <Stat label="Label" value={node?.label ?? "-"} />
      <Stat label="Market" value={market} cls="mono" />
      <Stat
        label="Last Seen"
        value={node?.last_seen_at ? new Date(node.last_seen_at).toLocaleString() : "-"}
        cls="mono"
      />

      <div className="eden-graph__section">
        <div className="eden-card__title eden-workbench__stat-block">Current Links</div>
        {links.length > 0 ? (
          links.slice(0, 12).map((link) => {
            const currentIsSource = link.source_node_id === (node?.node_id ?? endpointNodeId);
            const neighborId = currentIsSource ? link.target_node_id : link.source_node_id;
            const neighborLabel = currentIsSource ? link.target_label : link.source_label;
            return (
              <div key={link.link_id} className="eden-graph__edge">
                <button className="eden-graph__neighbor" onClick={() => onOpenNode(neighborId)}>
                  {neighborLabel}
                </button>
                <div className="eden-graph__edge-meta">
                  <span>{String(link.relation)}</span>
                  <span className="mono">{formatMaybeNumber(link.confidence)}</span>
                </div>
              </div>
            );
          })
        ) : (
          <div className="eden-empty">No current links</div>
        )}
      </div>

      <div className="eden-graph__section">
        <div className="eden-card__title eden-workbench__stat-block">Current Events</div>
        {events.length > 0 ? (
          events.slice(0, 8).map((event) => {
            const neighborId =
              event.subject_node_id === (node?.node_id ?? endpointNodeId)
                ? event.object_node_id
                : event.subject_node_id;
            const neighborLabel =
              event.subject_node_id === (node?.node_id ?? endpointNodeId)
                ? event.object_label
                : event.subject_label;
            return (
              <div key={event.event_id} className="eden-graph__event">
                <div className="eden-graph__event-main">
                  <span>{String(event.kind)}</span>
                  <span className="mono">{formatMaybeNumber(event.confidence)}</span>
                </div>
                {neighborId && (
                  <button className="eden-graph__neighbor" onClick={() => onOpenNode(neighborId)}>
                    {neighborLabel ?? neighborId}
                  </button>
                )}
              </div>
            );
          })
        ) : (
          <div className="eden-empty">No current events</div>
        )}
      </div>
    </div>
  );
}

function formatMaybeNumber(value: number | string | null | undefined) {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value.toFixed(3);
  }
  if (typeof value === "string" && value.trim().length > 0) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed.toFixed(3) : value;
  }
  return "-";
}
