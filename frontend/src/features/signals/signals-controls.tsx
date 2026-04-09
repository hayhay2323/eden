import type { SignalsSortKey } from "./use-signals-view";

const sortKeys: SignalsSortKey[] = [
  "support",
  "composite",
  "flow",
  "momentum",
  "volume",
  "symbol",
];

export function SignalsControls({
  sortKey,
  setSortKey,
  filterSector,
  setFilterSector,
  sectors,
}: {
  sortKey: SignalsSortKey;
  setSortKey: (value: SignalsSortKey) => void;
  filterSector: string | null;
  setFilterSector: (value: string | null) => void;
  sectors: string[];
}) {
  return (
    <div className="eden-toolbar">
      <span className="eden-card__title eden-toolbar__label">Sort:</span>
      {sortKeys.map((key) => (
        <button
          key={key}
          className={`eden-topbar__market-btn ${sortKey === key ? "eden-topbar__market-btn--active" : ""}`}
          onClick={() => setSortKey(key)}
        >
          {key}
        </button>
      ))}

      <span className="eden-toolbar__divider" />

      <span className="eden-card__title eden-toolbar__label--minor">Sector:</span>
      <button
        className={`eden-topbar__market-btn ${filterSector === null ? "eden-topbar__market-btn--active" : ""}`}
        onClick={() => setFilterSector(null)}
      >
        ALL
      </button>
      {sectors.map((sector) => (
        <button
          key={sector}
          className={`eden-topbar__market-btn ${filterSector === sector ? "eden-topbar__market-btn--active" : ""}`}
          onClick={() => setFilterSector(sector)}
        >
          {sector}
        </button>
      ))}
    </div>
  );
}
