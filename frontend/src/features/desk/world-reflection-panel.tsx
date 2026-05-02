import { useWorldReflection } from "@/lib/query/world-reflection";
import type {
  DecimalLike,
  WorldIntentReflectionBucketSummary,
  WorldIntentReflectionRecord,
  WorldIntentReflectionSummary,
} from "@/lib/api/types";

function toNumber(value: DecimalLike | null | undefined): number | null {
  if (value == null) return null;
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : null;
}

function pctLike(value: DecimalLike | null | undefined): string {
  const numeric = toNumber(value);
  if (numeric == null) return "-";
  return `${(numeric * 100).toFixed(1)}%`;
}

function decimalLike(value: DecimalLike | null | undefined, digits = 3): string {
  const numeric = toNumber(value);
  if (numeric == null) return "-";
  return numeric.toFixed(digits);
}

function signedLike(value: DecimalLike | null | undefined): string {
  const numeric = toNumber(value);
  if (numeric == null) return "-";
  return `${numeric >= 0 ? "+" : ""}${numeric.toFixed(3)}`;
}

function valueClass(value: DecimalLike | null | undefined): string {
  const numeric = toNumber(value);
  if (numeric == null) return "";
  return numeric > 0 ? "text-positive" : numeric < 0 ? "text-negative" : "";
}

function inverseValueClass(value: DecimalLike | null | undefined): string {
  const numeric = toNumber(value);
  if (numeric == null) return "";
  return valueClass(-numeric);
}

function label(value: string): string {
  return value.replace(/_/g, " ");
}

export function WorldReflectionPanel() {
  const { data, status } = useWorldReflection({ limit: 6 });

  if (status === "error" && !data) return null;

  const summary = data?.summary ?? null;
  const buckets = data?.buckets ?? [];
  const recent = data?.recent ?? [];
  const resolvedCount = summary?.resolved_count ?? 0;

  return (
    <div className="eden-card eden-panel-block">
      <div className="eden-card__header">
        <span className="eden-card__title">World Intent Memory</span>
        <span className="eden-card__badge eden-card__badge--green">
          {resolvedCount}
        </span>
      </div>

      {status === "pending" && !data ? (
        <div className="eden-empty">Loading reflection ledger</div>
      ) : !summary ? (
        <div className="eden-empty">No resolved intent reflections</div>
      ) : (
        <>
          <SummaryMetrics summary={summary} />

          {(summary.best_bucket || summary.worst_bucket) && (
            <div className="eden-grid eden-grid--2 eden-section-space">
              {summary.best_bucket && (
                <BucketRow labelText="Most reliable" bucket={summary.best_bucket} />
              )}
              {summary.worst_bucket && (
                <BucketRow labelText="Most fragile" bucket={summary.worst_bucket} />
              )}
            </div>
          )}

          {buckets.length > 0 && (
            <div className="eden-note-list eden-note-list--compact">
              {buckets.slice(0, 5).map((bucket) => (
                <div key={bucket.key} className="eden-note-item">
                  <span className="mono eden-market-session__bar-symbol">
                    {label(bucket.kind)}
                  </span>
                  <span className="mono text-muted eden-section-meta">
                    {bucket.direction}
                  </span>
                  <span className={valueClass(bucket.reliability)}>
                    rel {pctLike(bucket.reliability)}
                  </span>
                  <span className="text-dim">
                    n {bucket.resolved_count}
                  </span>
                </div>
              ))}
            </div>
          )}

          {recent.length > 0 && (
            <div className="eden-focus-events">
              <div className="mono text-muted eden-kicker">RECENT RESOLUTIONS</div>
              {recent.map((record) => (
                <ResolutionRow key={record.record_id} record={record} />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

function SummaryMetrics({ summary }: { summary: WorldIntentReflectionSummary }) {
  return (
    <div className="eden-metric-row">
      <Metric
        label="RELIABILITY"
        value={pctLike(summary.reliability)}
        cls={valueClass(summary.reliability)}
      />
      <Metric
        label="VIOLATION"
        value={pctLike(summary.violation_rate)}
        cls={inverseValueClass(summary.violation_rate)}
      />
      <Metric label="CONF" value={pctLike(summary.mean_confidence)} />
      <Metric
        label="CAL GAP"
        value={signedLike(summary.calibration_gap)}
        cls={valueClass(summary.calibration_gap)}
      />
    </div>
  );
}

function BucketRow({
  labelText,
  bucket,
}: {
  labelText: string;
  bucket: WorldIntentReflectionBucketSummary;
}) {
  return (
    <div className="eden-stat">
      <span className="eden-stat__label">
        <span className="mono text-muted eden-section-meta">{labelText}</span>
        <span className="eden-focus-row__subtitle">
          {label(bucket.kind)} / {bucket.direction}
        </span>
      </span>
      <span className="eden-stat__value eden-stat__value--compact">
        {pctLike(bucket.reliability)} | n {bucket.resolved_count}
      </span>
    </div>
  );
}

function ResolutionRow({ record }: { record: WorldIntentReflectionRecord }) {
  const violated = record.violation_count > 0;
  const directionChanged = record.predicted_direction !== record.realized_direction;

  return (
    <div className="eden-focus-event">
      <span className={`eden-focus-event__kind ${violated ? "text-negative" : "text-positive"}`}>
        {violated ? "violated" : "confirmed"}
      </span>
      <span>
        {label(record.predicted_kind)} / {record.predicted_direction}
        {" -> "}
        {label(record.realized_kind)} / {record.realized_direction}
      </span>
      <span className="mono text-muted eden-section-meta">
        t{record.tick_predicted_at}-t{record.tick_resolved_at}
      </span>
      <span className={inverseValueClass(record.violation_magnitude)}>
        mag {decimalLike(record.violation_magnitude)}
      </span>
      {directionChanged && (
        <span className="mono text-muted eden-section-meta">direction changed</span>
      )}
    </div>
  );
}

function Metric({
  label,
  value,
  cls,
}: {
  label: string;
  value: string;
  cls?: string;
}) {
  return (
    <div className="eden-metric">
      <div className="mono text-muted eden-metric__label">{label}</div>
      <div className={`mono eden-metric__value ${cls ?? ""}`}>{value}</div>
    </div>
  );
}
