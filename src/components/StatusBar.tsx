import { Database, Shield, WifiOff } from "lucide-react";
import type { Phase } from "../hooks/useSearch";
import type { IndexerState, IndexerStatus, Origin } from "../lib/types";

const STATE_COLOR: Record<IndexerState, string> = {
  searching: "bg-accent animate-pulse",
  ok: "bg-good",
  timeout: "bg-warn",
  error: "bg-bad",
  cancelled: "bg-faint",
  skipped: "bg-faint",
};

interface Props {
  phase: Phase;
  statuses: IndexerStatus[];
  total: number;
  shown: number;
  scraping: number;
  elapsedMs: number | null;
  error: string | null;
  origin: Origin | null;
}

export function StatusBar({ phase, statuses, total, shown, scraping, elapsedMs, error, origin }: Props) {
  const summary =
    phase === "error"
      ? error
      : phase === "searching"
        ? `Searching ${statuses.filter((s) => s.state === "searching").length} source(s)…`
        : phase === "scraping"
          ? `Scraping trackers for ${scraping} result(s)…`
          : phase === "cancelled"
            ? "Cancelled"
            : phase === "done"
              ? `${total} result${total === 1 ? "" : "s"}${shown !== total ? ` · ${shown} shown` : ""} · ${((elapsedMs ?? 0) / 1000).toFixed(2)}s`
              : "Ready";
  return (
    <footer className="chrome flex h-6 shrink-0 items-center gap-3 overflow-hidden border-t border-line bg-panel px-3 text-[11px] text-dim">
      <span className={`shrink-0 ${phase === "error" ? "text-bad" : ""}`}>{summary}</span>
      {phase === "done" && origin === "cache" && (
        <span className="flex shrink-0 items-center gap-1 text-accent" title="Replayed from the local cache. Ctrl+R or F5 searches live.">
          <Database size={10} /> cached · Ctrl+R to refresh
        </span>
      )}
      {phase === "done" && origin === "offline" && (
        <span className="flex shrink-0 items-center gap-1 text-warn" title="Every source failed; these results come from the local index.">
          <WifiOff size={10} /> offline · local index
        </span>
      )}
      <div className="ml-auto flex min-w-0 items-center gap-3 overflow-hidden">
        {statuses.map((s) => (
          <span
            key={s.indexerId}
            className="flex shrink-0 items-center gap-1.5"
            title={`${s.name} (${s.sourceType}) — ${s.state}${s.error ? `: ${s.error}` : ""}${s.proxied ? " · via proxy" : ""}`}
          >
            <span className={`h-1.5 w-1.5 rounded-full ${STATE_COLOR[s.state]}`} />
            <span className={s.state === "error" || s.state === "timeout" ? "text-faint line-through decoration-faint/50" : ""}>{s.name}</span>
            {s.proxied && <Shield size={9} className="text-[#a78bfa]" />}
            {s.latencyMs != null && (
              <span className="num text-faint">
                {s.state === "ok" ? `${s.count}·` : ""}
                {s.latencyMs}ms
              </span>
            )}
          </span>
        ))}
      </div>
    </footer>
  );
}
