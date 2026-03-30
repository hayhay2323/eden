import type { PropsWithChildren } from "react";

export function AppShell({ children }: PropsWithChildren) {
  return <div className="app-shell app-shell--minimal">{children}</div>;
}
