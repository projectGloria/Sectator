// Colour themes. The palettes themselves live in index.css as `[data-theme]` variable sets;
// this module lists them for the picker and applies the stored choice to <html>.

export type ThemeId = "claude" | "claude-dark" | "midnight";

export interface ThemeInfo {
  id: ThemeId;
  name: string;
  hint: string;
  /** Preview swatches: [background, panel, line, text, accent]. */
  swatch: [string, string, string, string, string];
}

export const DEFAULT_THEME: ThemeId = "claude";

export const THEMES: ThemeInfo[] = [
  { id: "claude", name: "Claude", hint: "Warm light: Pampas, Cloudy and Crail", swatch: ["#ffffff", "#f4f3ee", "#e3e0d6", "#262624", "#c15f3c"] },
  { id: "claude-dark", name: "Claude Dark", hint: "The same warm palette after dark", swatch: ["#1f1e1d", "#262624", "#3a3936", "#f4f3ee", "#d97757"] },
  { id: "midnight", name: "Midnight", hint: "Cool near-black with cyan", swatch: ["#0a0b0d", "#101216", "#22262e", "#e5e7eb", "#22d3ee"] },
];

const CACHE_KEY = "sectator.theme";

export const isTheme = (v: unknown): v is ThemeId => THEMES.some((t) => t.id === v);

/** Set the theme on <html>. The settings store is the source of truth; the local cache only
 *  lets the next launch paint in the right colours before settings load. */
export function applyTheme(id: string | null | undefined) {
  const theme = isTheme(id) ? id : DEFAULT_THEME;
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(CACHE_KEY, theme);
  } catch {
    /* storage unavailable: the theme still applies for this run */
  }
}

/** Apply the theme the last run used (before settings arrive). */
export function applyCachedTheme() {
  let cached: string | null = null;
  try {
    cached = localStorage.getItem(CACHE_KEY);
  } catch {
    /* ignore */
  }
  applyTheme(cached);
}
