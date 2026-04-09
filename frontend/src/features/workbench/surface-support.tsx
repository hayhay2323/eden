import type { CSSProperties } from "react";

export function SurfaceKpi({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone: string;
}) {
  return (
    <div className="eden-card eden-surface-kpi">
      <div className="eden-card__title eden-surface-kpi__label">{label}</div>
      <div
        className="mono eden-surface-kpi__value"
        style={{ "--eden-tone": tone } as CSSProperties}
      >
        {value}
      </div>
    </div>
  );
}

export function SelectionHint({
  eyebrow,
  title,
  body,
}: {
  eyebrow: string;
  title: string;
  body: string;
}) {
  return (
    <div className="eden-card eden-surface-layout__placeholder">
      <div className="eden-card__title eden-selection-hint__eyebrow">{eyebrow}</div>
      <div className="eden-selection-hint__title">{title}</div>
      <div className="eden-selection-hint__body">{body}</div>
    </div>
  );
}
