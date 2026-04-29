import type { ReactNode } from "react";

export function Stat({
  label,
  value,
  cls,
}: {
  label: string;
  value: string;
  cls?: string;
}) {
  return (
    <div className="eden-stat">
      <span className="eden-stat__label">{label}</span>
      <span className={`eden-stat__value ${cls ?? ""}`}>{value}</span>
    </div>
  );
}

export function WorkbenchCard({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="eden-card eden-workbench__card">
      <div className="eden-card__title eden-workbench__stat-block">{title}</div>
      {children}
    </div>
  );
}

export function WorkbenchMeta({
  kind,
  selfId,
  relationships,
  history,
}: {
  kind: string;
  selfId: string;
  relationships: number;
  history: number;
}) {
  return (
    <div className="eden-workbench__meta">
      <div className="eden-workbench__meta-cell">
        <div className="mono text-muted eden-kicker">KIND</div>
        <div className="mono">{kind}</div>
      </div>
      <div className="eden-workbench__meta-cell">
        <div className="mono text-muted eden-kicker">ID</div>
        <div className="mono eden-workbench__value-wrap">{selfId}</div>
      </div>
      <div className="eden-workbench__meta-cell">
        <div className="mono text-muted eden-kicker">REL</div>
        <div className="mono">{relationships}</div>
      </div>
      <div className="eden-workbench__meta-cell">
        <div className="mono text-muted eden-kicker">HIST</div>
        <div className="mono">{history}</div>
      </div>
    </div>
  );
}

export function ObjectChip({
  active,
  visited,
  onClick,
  children,
}: {
  active?: boolean;
  visited?: boolean;
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <button
      className={`eden-object-chip${active ? " eden-object-chip--active" : ""}${visited ? " eden-object-chip--visited" : ""}`}
      onClick={onClick}
      disabled={active}
    >
      {children}
    </button>
  );
}
