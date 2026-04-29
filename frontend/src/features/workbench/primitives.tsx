import type { CSSProperties, ReactNode } from "react";

export function ToneBadge({
  tone,
  children,
  className,
}: {
  tone: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={`eden-tone-badge${className ? ` ${className}` : ""}`}
      style={{ "--eden-tone": tone } as CSSProperties}
    >
      {children}
    </span>
  );
}
