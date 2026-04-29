export const LIVE_REFRESH_INTERVAL_MS = 5_000;
export const LIVE_DETAIL_REFRESH_INTERVAL_MS = 7_000;

export function liveRefetchInterval(
  enabled: boolean,
  intervalMs: number,
): number | false {
  if (!enabled) return false;
  if (typeof document !== "undefined" && document.visibilityState !== "visible") {
    return false;
  }
  return intervalMs;
}
