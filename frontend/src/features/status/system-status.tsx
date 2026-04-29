import { Tag, Intent } from "@blueprintjs/core";

import { useContextStatus } from "@/lib/query/status";

export function SystemStatus() {
  const { data } = useContextStatus();
  if (!data) return null;

  return (
    <div style={{ display: "flex", gap: 4, padding: "4px 8px", fontSize: 11, opacity: 0.7 }}>
      {data.tool_registry_available && <Tag minimal intent={Intent.SUCCESS}>Registry</Tag>}
      {data.context_layers_available && <Tag minimal intent={Intent.SUCCESS}>Context</Tag>}
      {data.coordinator_available && <Tag minimal intent={Intent.PRIMARY}>Coordinator</Tag>}
      {data.task_lifecycle_available && <Tag minimal intent={Intent.SUCCESS}>Tasks</Tag>}
      {data.runtime_features.length > 0 && (
        <Tag minimal intent={Intent.WARNING}>{data.runtime_features.length} runtime gates</Tag>
      )}
    </div>
  );
}
