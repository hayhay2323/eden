import { useHealthReportQuery } from "@/lib/query/health";

export function WorkspaceCapabilityBanner() {
  const { data: health } = useHealthReportQuery();

  if (!health) return null;

  if (!health.api.persistence_enabled) {
    return (
      <div className="eden-capability-banner">
        <span className="eden-capability-banner__label">Read-only Workspace</span>
        <span className="eden-capability-banner__body">
          eden-api is running without <span className="mono">persistence</span>, so
          workflow writes are unavailable even if the UI shows operator controls.
        </span>
      </div>
    );
  }

  return null;
}
