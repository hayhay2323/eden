import { AppShell } from "@/shell/app-shell";

function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }

  return "Unexpected route error";
}

function ErrorPanel({
  error,
  reset,
  title,
}: {
  error: unknown;
  reset: () => void;
  title: string;
}) {
  const message = getErrorMessage(error);
  const path = typeof window !== "undefined" ? window.location.pathname : "/";

  return (
    <div className="eden-route-error">
      <div className="eden-route-error__eyebrow">Route Error</div>
      <div className="eden-route-error__title">{title}</div>
      <div className="eden-route-error__message">{message}</div>
      <div className="eden-route-error__meta">
        <span>Path</span>
        <span className="mono">{path}</span>
      </div>
      <div className="eden-route-error__actions">
        <button className="eden-topbar__market-btn" onClick={reset}>
          Retry
        </button>
        <button
          className="eden-topbar__market-btn"
          onClick={() => window.location.assign("/")}
        >
          Go To Desk
        </button>
        <button
          className="eden-topbar__market-btn"
          onClick={() => window.location.reload()}
        >
          Hard Reload
        </button>
      </div>
    </div>
  );
}

export function RouteErrorState({
  error,
  reset,
}: {
  error: unknown;
  reset: () => void;
}) {
  return (
    <ErrorPanel
      error={error}
      reset={reset}
      title="This workspace view hit an unexpected shape."
    />
  );
}

export function RootRouteErrorState({
  error,
  reset,
}: {
  error: unknown;
  reset: () => void;
}) {
  return (
    <AppShell>
      <ErrorPanel
        error={error}
        reset={reset}
        title="Eden recovered into safe mode."
      />
    </AppShell>
  );
}
