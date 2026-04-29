import type {
  OperationalHistoryRecord,
  OperationalHistoryRef,
} from "@/lib/api/types";

export function HistoryWorkbench({
  refs,
  activeRef,
  onSelect,
  records,
  status,
  errorMessage,
}: {
  refs: OperationalHistoryRef[];
  activeRef: OperationalHistoryRef | null;
  onSelect: (endpoint: string) => void;
  records?: OperationalHistoryRecord[];
  status: "pending" | "error" | "success";
  errorMessage: string;
}) {
  return (
    <div>
      <div className="eden-history__refs">
        {refs.map((ref) => {
          const active = activeRef?.endpoint === ref.endpoint;
          return (
            <button
              key={ref.endpoint}
              className={`eden-history__ref ${active ? "eden-history__ref--active" : ""}`}
              onClick={() => onSelect(ref.endpoint)}
            >
              <span>{ref.key}</span>
              <span className="mono">
                {ref.count ?? 0}
                {ref.latest_at ? ` · ${new Date(ref.latest_at).toLocaleDateString()}` : ""}
              </span>
            </button>
          );
        })}
      </div>

      {!activeRef ? (
        <div className="eden-empty">Select a history feed</div>
      ) : status === "pending" ? (
        <div className="eden-loading eden-workbench__loading">Loading history…</div>
      ) : status === "error" ? (
        <div className="eden-empty">{errorMessage}</div>
      ) : !records || records.length === 0 ? (
        <div className="eden-empty">No history records</div>
      ) : (
        <div className="eden-history__timeline">
          {records.map((record, index) => {
            const item = summarizeHistoryRecord(record, index);
            return (
              <div key={item.key} className="eden-history__item">
                <div className="eden-history__time mono">{item.timestamp}</div>
                <div className="eden-history__title">{item.title}</div>
                {item.detail && (
                  <div className="eden-history__detail">{item.detail}</div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function summarizeHistoryRecord(record: OperationalHistoryRecord, index: number) {
  const timestamp =
    readString(record, ["timestamp", "recorded_at", "resolved_at", "observed_at"]) ??
    `item-${index + 1}`;
  const fromStage = readString(record, ["from_stage"]);
  const stage = readString(record, ["stage", "status"]);

  let title =
    fromStage || stage
      ? `${fromStage ?? "?"} → ${stage ?? "?"}`
      : readString(record, [
            "headline",
            "title",
            "summary",
            "action",
            "event_type",
            "recommendation_id",
          ]) ?? `Record ${index + 1}`;

  const details: string[] = [];
  const symbol = readString(record, ["symbol"]);
  const actor = readString(record, ["actor"]);
  const governance = readString(record, [
    "governance_reason",
    "governance_reason_code",
  ]);
  const note = readString(record, ["note", "transition_reason"]);
  const confidence = readNumber(record, [
    "confidence",
    "confidence_gap",
    "price_return",
    "net_return",
  ]);

  if (symbol) details.push(symbol);
  if (actor) details.push(actor);
  if (governance) details.push(governance);
  if (note) details.push(note);
  if (confidence != null) details.push(Number(confidence).toFixed(3));

  if (!title || title.trim().length === 0) {
    title = `Record ${index + 1}`;
  }

  return {
    key: `${timestamp}:${title}:${index}`,
    timestamp,
    title,
    detail: details.join(" · "),
  };
}

function readString(record: OperationalHistoryRecord, keys: string[]): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim().length > 0) {
      return value;
    }
  }
  return null;
}

function readNumber(record: OperationalHistoryRecord, keys: string[]): number | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
    if (typeof value === "string" && value.trim().length > 0) {
      const parsed = Number(value);
      if (Number.isFinite(parsed)) {
        return parsed;
      }
    }
  }
  return null;
}
