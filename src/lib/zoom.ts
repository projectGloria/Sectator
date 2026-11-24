// UI zoom: scales the whole webview like browser zoom. Settings hold the value
// (`uiZoom`); the local cache only lets the next launch open at the right size.
import { getCurrentWebview } from "@tauri-apps/api/webview";

export const ZOOM_MIN = 0.5;
export const ZOOM_MAX = 2;
export const ZOOM_STEP = 0.1;

const CACHE_KEY = "sectator.zoom";

export const clampZoom = (z: number) =>
  Number.isFinite(z)
    ? Math.round(Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z)) * 100) / 100
    : 1;

export const zoomLabel = (z: number) => `${Math.round(z * 100)}%`;

export function applyZoom(z: number | null | undefined) {
  const next = clampZoom(z ?? 1);
  try {
    localStorage.setItem(CACHE_KEY, String(next));
  } catch {
    /* storage unavailable: the zoom still applies for this run */
  }
  getCurrentWebview()
    .setZoom(next)
    .catch(() => {});
}

export function applyCachedZoom() {
  let cached: string | null = null;
  try {
    cached = localStorage.getItem(CACHE_KEY);
  } catch {
    /* ignore */
  }
  if (cached != null) applyZoom(Number(cached));
}
