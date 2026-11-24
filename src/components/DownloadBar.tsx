// Always-visible download dock above the status bar: overall progress, live rates and the busiest transfers.
import {
  ArrowDown,
  ArrowUp,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Clock,
} from "lucide-react";
import type { Download } from "../lib/types";
import { BAR, STATE_ICON, formatEta, formatRate } from "./Downloads";
import { KindIcon } from "./KindIcon";

const busy = (d: Download) =>
  d.state === "downloading" || d.state === "metadata" || d.state === "checking";

interface Props {
  items: Download[];
  /** Whether the full downloads view is showing. */
  open: boolean;
  onToggle: () => void;
  /** Open one download's details. */
  onPick: (hash: string) => void;
}

export function DownloadBar({ items, open, onToggle, onPick }: Props) {
  if (!items.length) return null;
  const active = items.filter(busy);
  const seeding = items.filter((d) => d.state === "seeding").length;
  const down = items.reduce((n, d) => n + d.downRate, 0);
  const up = items.reduce((n, d) => n + d.upRate, 0);
  const size = active.reduce((n, d) => n + d.size, 0);
  const done = active.reduce((n, d) => n + d.done, 0);
  const pct = size ? Math.min(100, Math.floor((done / size) * 100)) : 0;
  const eta = active.reduce<number | null>(
    (m, d) => (d.etaSecs == null ? m : Math.max(m ?? 0, d.etaSecs)),
    null,
  );
  const shown = (active.length ? active : items).slice(0, 3);
  const more = (active.length ? active.length : items.length) - shown.length;

  return (
    <div
      role="button"
      tabIndex={0}
      title={open ? "Back to results (Ctrl+J)" : "Show downloads (Ctrl+J)"}
      onClick={onToggle}
      onKeyDown={(e) =>
        (e.key === "Enter" || e.key === " ") && (e.preventDefault(), onToggle())
      }
      className="chrome relative flex h-9 shrink-0 cursor-pointer items-center gap-4 border-t border-line bg-panel px-3 text-[11.5px] transition-colors duration-150 hover:bg-raised active:bg-line focus-visible:outline focus-visible:outline-1 focus-visible:-outline-offset-1 focus-visible:outline-accent"
    >
      {active.length > 0 && (
        <div className="absolute inset-x-0 top-0 h-[2px]">
          <div
            className="h-full bg-accent transition-[width] duration-500"
            style={{ width: `${pct}%` }}
          />
        </div>
      )}

      <span className="flex shrink-0 items-center gap-1.5">
        {active.length ? (
          <>
            <span className="relative flex h-2 w-2">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-50" />
              <span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
            </span>
            <span className="text-fg">{active.length} downloading</span>
            <span className="num text-dim">{pct}%</span>
          </>
        ) : (
          <>
            <CheckCircle2 size={13} className="text-good" />
            <span className="text-fg">
              {seeding ? `${seeding} seeding` : "All done"}
            </span>
          </>
        )}
      </span>

      <div className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden">
        {shown.map((d) => (
          <Chip key={d.infoHash} d={d} onPick={onPick} />
        ))}
        {more > 0 && <span className="num shrink-0 text-faint">+{more}</span>}
      </div>

      <span className="num flex shrink-0 items-center gap-3 text-dim">
        {eta != null && (
          <span
            className="flex items-center gap-1"
            title="Time left for the slowest download"
          >
            <Clock size={11} /> {formatEta(eta)}
          </span>
        )}
        <span
          className="flex items-center gap-1 text-accent"
          title="Total download rate"
        >
          <ArrowDown size={12} /> {formatRate(down)}
        </span>
        <span
          className="flex items-center gap-1 text-good"
          title="Total upload rate"
        >
          <ArrowUp size={12} /> {formatRate(up)}
        </span>
      </span>
      <span className="shrink-0 text-faint">
        {open ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
      </span>
    </div>
  );
}

function Chip({ d, onPick }: { d: Download; onPick: (hash: string) => void }) {
  const [Icon, tone, spin] = STATE_ICON[d.state];
  const pct = Math.floor(d.progress * 100);
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onPick(d.infoHash);
      }}
      onKeyDown={(e) => e.stopPropagation()}
      className="flex w-[200px] min-w-[90px] shrink items-center gap-1.5 rounded-md border border-line bg-bg/60 px-2 py-1 text-left transition-colors duration-150 hover:border-edge hover:bg-bg active:bg-lift focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent"
      title={`${d.name} — ${pct}% · click for details`}
    >
      <KindIcon kind={d.kind} size={14} />
      <span className="min-w-0 flex-1">
        <span className="block truncate leading-tight">{d.name}</span>
        <span className="mt-0.5 block h-[2px] overflow-hidden rounded-full bg-line">
          <span
            className={`block h-full rounded-full transition-[width] duration-500 ${BAR[d.state]}`}
            style={{ width: `${pct}%` }}
          />
        </span>
      </span>
      <Icon
        size={10}
        className={`shrink-0 ${tone} ${spin ? "animate-spin" : ""}`}
      />
      <span className="num shrink-0 text-dim">
        {d.state === "metadata" ? "—" : `${pct}%`}
      </span>
    </button>
  );
}
