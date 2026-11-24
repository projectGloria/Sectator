import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { AlertTriangle, ArrowDown, ArrowUp, BadgeCheck, FileSearch, Radio } from "lucide-react";
import { formatAge, formatCount, formatSize } from "../lib/format";
import { leechersOf, seedersOf, type TorrentResult } from "../lib/types";

export type SortKey = "score" | "title" | "uploader" | "size" | "seeders" | "leechers" | "age" | "sources";

// Sorts after any real name, so unknown uploaders land last in ascending order.
const LAST = String.fromCharCode(0xffff);
export interface Sort {
  key: SortKey;
  desc: boolean;
}

const COLUMNS: { key: SortKey; label: string; className: string }[] = [
  { key: "title", label: "Name", className: "flex-1 min-w-0" },
  { key: "uploader", label: "Uploader", className: "w-[132px]" },
  { key: "size", label: "Size", className: "w-[84px] justify-end" },
  { key: "seeders", label: "Seed", className: "w-[64px] justify-end" },
  { key: "leechers", label: "Leech", className: "w-[64px] justify-end" },
  { key: "age", label: "Age", className: "w-[56px] justify-end" },
  { key: "sources", label: "Src", className: "w-[78px] justify-end" },
  { key: "score", label: "Score", className: "w-[60px] justify-end" },
];

export function sortResults(rows: TorrentResult[], sort: Sort): TorrentResult[] {
  const val = (r: TorrentResult): number | string => {
    switch (sort.key) {
      case "score":
        return r.score;
      case "title":
        return r.title.toLowerCase();
      case "uploader":
        return r.uploader?.toLowerCase() ?? LAST;
      case "size":
        return r.size;
      case "seeders":
        return seedersOf(r) ?? -1;
      case "leechers":
        return leechersOf(r) ?? -1;
      case "age":
        return -(r.uploadedAt ?? -Infinity);
      case "sources":
        return r.sources.length;
    }
  };
  const dir = sort.desc ? -1 : 1;
  return [...rows].sort((a, b) => {
    const x = val(a);
    const y = val(b);
    if (x < y) return -dir;
    if (x > y) return dir;
    return b.score - a.score;
  });
}

const ROW = 30;

interface Props {
  rows: TorrentResult[];
  selected: string | null;
  sort: Sort;
  onSort: (s: Sort) => void;
  onSelect: (hash: string) => void;
  onActivate: (r: TorrentResult) => void;
  empty: React.ReactNode;
}

export function ResultsTable({ rows, selected, sort, onSort, onSelect, onActivate, empty }: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const virt = useVirtualizer({ count: rows.length, getScrollElement: () => scroller.current, estimateSize: () => ROW, overscan: 12 });

  const selectedIndex = selected ? rows.findIndex((r) => r.infoHash === selected) : -1;
  useEffect(() => {
    if (selectedIndex >= 0) virt.scrollToIndex(selectedIndex, { align: "auto" });
  }, [selectedIndex, virt]);

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className="chrome flex h-7 shrink-0 items-center border-b border-line bg-panel pr-[6px] text-[11px] uppercase tracking-wide text-faint">
        {COLUMNS.map((c) => {
          const active = sort.key === c.key;
          return (
            <button
              key={c.key}
              onClick={() => onSort({ key: c.key, desc: active ? !sort.desc : c.key !== "title" && c.key !== "uploader" })}
              className={`flex h-full items-center gap-1 px-2.5 transition-colors duration-150 hover:bg-raised hover:text-fg ${c.className} ${active ? "text-fg" : ""}`}
            >
              {c.label}
              {active && (sort.desc ? <ArrowDown size={11} /> : <ArrowUp size={11} />)}
            </button>
          );
        })}
      </div>
      <div ref={scroller} className="relative min-h-0 flex-1 overflow-y-auto" style={{ scrollbarGutter: "stable" }}>
        {rows.length === 0 ? (
          empty
        ) : (
          <div style={{ height: virt.getTotalSize(), position: "relative" }}>
            {virt.getVirtualItems().map((v) => {
              const r = rows[v.index];
              return (
                <Row
                  key={r.infoHash}
                  r={r}
                  top={v.start}
                  odd={v.index % 2 === 1}
                  selected={r.infoHash === selected}
                  onClick={() => onSelect(r.infoHash)}
                  onDoubleClick={() => onActivate(r)}
                />
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

function Row({ r, top, odd, selected, onClick, onDoubleClick }: { r: TorrentResult; top: number; odd: boolean; selected: boolean; onClick: () => void; onDoubleClick: () => void }) {
  const seeds = seedersOf(r);
  const leech = leechersOf(r);
  const flagged = r.flags.some((f) => f !== "noSeedData");
  const tags = [r.meta.resolution, r.meta.source, r.meta.codec, ...(r.meta.dynamicRange ?? [])].filter(Boolean) as string[];
  const kinds = new Set(r.sources.map((s) => s.sourceType));
  return (
    <div
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      className={`chrome absolute left-0 right-0 flex cursor-default items-center text-[12.5px] transition-colors duration-100 ${
        selected ? "bg-accent/12 shadow-[inset_2px_0_0_var(--color-accent)]" : odd ? "bg-white/[0.012] hover:bg-raised" : "hover:bg-raised"
      } ${flagged ? "opacity-60" : ""}`}
      style={{ top, height: ROW }}
    >
      <div className="flex min-w-0 flex-1 items-center gap-1.5 px-2.5">
        {flagged && <AlertTriangle size={12} className="shrink-0 text-warn" />}
        {r.fileMatch && <FileSearch size={12} className="shrink-0 text-[#60a5fa]" />}
        <span className="truncate" title={r.title}>
          {r.title}
        </span>
        {tags.slice(0, 3).map((t) => (
          <span key={t} className="num shrink-0 rounded bg-line/70 px-1 text-[10px] leading-4 text-dim">
            {t}
          </span>
        ))}
      </div>
      <div className="flex w-[132px] min-w-0 items-center gap-1 px-2.5 text-dim">
        {r.uploader ? (
          <>
            <span className="truncate" title={r.verifiedUploader ? `${r.uploader} (trusted/VIP on the site)` : r.uploader}>
              {r.uploader}
            </span>
            {r.verifiedUploader && <BadgeCheck size={12} className="shrink-0 text-good" aria-label="Trusted uploader" />}
          </>
        ) : (
          <span className="text-faint">—</span>
        )}
      </div>
      <div className="num w-[84px] px-2.5 text-right text-dim">{formatSize(r.size)}</div>
      <div className={`num flex w-[64px] items-center justify-end gap-1 px-2.5 ${seeds == null ? "text-faint" : seeds > 0 ? "text-good" : "text-bad"}`}>
        {r.live && <Radio size={10} className="text-accent" aria-label="Live tracker count" />}
        {formatCount(seeds)}
      </div>
      <div className="num w-[64px] px-2.5 text-right text-dim">{formatCount(leech)}</div>
      <div className="num w-[56px] px-2.5 text-right text-dim">{formatAge(r.uploadedAt)}</div>
      <div className="num flex w-[78px] items-center justify-end gap-1 px-2.5 text-dim">
        {(["curated", "torznab", "dht"] as const).map((k) =>
          kinds.has(k) ? <span key={k} title={k} className={`h-1.5 w-1.5 rounded-full ${KIND_DOT[k]}`} /> : null,
        )}
        <span className="ml-0.5">{r.sources.length}</span>
      </div>
      <div className="num w-[60px] px-2.5 text-right text-faint">{r.score.toFixed(2)}</div>
    </div>
  );
}

export const KIND_DOT: Record<string, string> = { curated: "bg-accent", torznab: "bg-good", dht: "bg-[#a78bfa]" };
