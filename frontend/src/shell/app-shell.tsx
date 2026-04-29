import type { PropsWithChildren } from "react";
import { useNavigate, useLocation } from "@tanstack/react-router";

import { useShellStore } from "@/state/shell-store";
import { useOperationalSnapshot, useRefreshSnapshot } from "@/lib/query/operational";
import { SystemStatus } from "@/features/status/system-status";

const tabs = [
  { path: "/", label: "Market Desk" },
  { path: "/workspace", label: "Case Board" },
  { path: "/signals", label: "Signals" },
] as const;

function Topbar() {
  const { market, setMarket, liveRefreshEnabled, setLiveRefreshEnabled } = useShellStore();
  const { data: snap, status, isFetching } = useOperationalSnapshot();

  const session = snap?.market_session;
  const regime = session?.market_regime;
  const tick = snap?.source_tick;
  const headline = session?.wake_headline;
  const observedAt = session?.observed_at
    ? new Date(session.observed_at).toLocaleTimeString()
    : null;

  const connectionStatus =
    status === "error" ? "error" : status === "pending" ? "stale" : "live";

  return (
    <div className="eden-topbar">
      <span className="eden-topbar__brand">EDEN</span>

      <div className="eden-topbar__market-group">
        {(["hk", "us"] as const).map((m) => (
          <button
            key={m}
            className={`eden-topbar__market-btn ${market === m ? "eden-topbar__market-btn--active" : ""}`}
            onClick={() => setMarket(m)}
          >
            {m.toUpperCase()}
          </button>
        ))}
      </div>

      {regime && (
        <span className={`eden-topbar__regime regime--${regime.bias}`}>
          {regime.bias}
        </span>
      )}

      {headline && (
        <span className="eden-topbar__headline">{headline}</span>
      )}

      <span className="eden-topbar__spacer" />

      <button
        className={`eden-topbar__market-btn ${liveRefreshEnabled ? "eden-topbar__market-btn--active" : ""}`}
        onClick={() => setLiveRefreshEnabled(!liveRefreshEnabled)}
      >
        {liveRefreshEnabled ? "LIVE" : "PAUSED"}
      </button>

      <RefreshButton />

      {tick != null && (
        <span className="eden-topbar__tick">
          tick {tick}{observedAt ? ` · ${observedAt}` : ""}
        </span>
      )}

      {isFetching && <span className="eden-topbar__fetch">sync</span>}
      <span className={`eden-topbar__status eden-topbar__status--${connectionStatus}`} />
    </div>
  );
}

function RefreshButton() {
  const refresh = useRefreshSnapshot();
  const { isFetching } = useOperationalSnapshot();

  return (
    <button
      className={`eden-topbar__market-btn${isFetching ? " eden-topbar__market-btn--busy" : ""}`}
      onClick={refresh}
      disabled={isFetching}
    >
      {isFetching ? "..." : "Refresh"}
    </button>
  );
}

function TabBar() {
  const navigate = useNavigate();
  const location = useLocation();

  return (
    <div className="eden-tabs">
      {tabs.map((tab) => (
        <button
          key={tab.path}
          className={`eden-tabs__btn ${location.pathname === tab.path ? "eden-tabs__btn--active" : ""}`}
          onClick={() => navigate({ to: tab.path })}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}

export function AppShell({ children }: PropsWithChildren) {
  return (
    <div className="eden-shell bp5-dark">
      <Topbar />
      <TabBar />
      <div className="eden-main">{children}</div>
      <SystemStatus />
    </div>
  );
}
