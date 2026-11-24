import { useMemo, useState } from "react";
import { AlertTriangle, Bookmark, BookmarkCheck, ChevronDown, ChevronRight, Copy, File, Folder, Magnet, Radio, Send, X } from "lucide-react";
import { FLAG_LABEL, formatAge, formatCount, formatSize } from "../lib/format";
import { leechersOf, seedersOf, type TorrentFile, type TorrentResult } from "../lib/types";
import { KIND_DOT } from "./ResultsTable";

interface Props {
  r: TorrentResult;
  onClose: () => void;
  onOpen: (r: TorrentResult) => void;
  onCopy: (text: string, what: string) => void;
  bookmarked: boolean;
  onBookmark: (r: TorrentResult) => void;
  /** Name of the default torrent client, if one is configured. */
  client: string | null;
  onSend: (r: TorrentResult) => void;
}

export function Inspector({ r, onClose, onOpen, onCopy, bookmarked, onBookmark, client, onSend }: Props) {
  const seeds = seedersOf(r);
  const leech = leechersOf(r);
  const tags = [
    r.meta.resolution,
    r.meta.source,
    r.meta.codec,
    ...r.meta.dynamicRange,
    ...r.meta.audio,
    ...r.meta.format,
    r.meta.season != null ? `S${String(r.meta.season).padStart(2, "0")}${r.meta.episode != null ? `E${String(r.meta.episode).padStart(2, "0")}` : ""}` : null,
    r.meta.complete ? "complete" : null,
    r.meta.year ? String(r.meta.year) : null,
    r.meta.version,
    r.meta.os,
    r.meta.arch,
    r.meta.group ? `-${r.meta.group}` : null,
  ].filter(Boolean) as string[];

  return (
    <aside className="flex w-[380px] shrink-0 flex-col border-l border-line bg-panel">
      <div className="chrome flex h-7 shrink-0 items-center border-b border-line pl-3 text-[11px] uppercase tracking-wide text-faint">
        Inspector
        <button aria-label="Close inspector" onClick={onClose} className="ml-auto flex h-full w-8 items-center justify-center transition-colors duration-150 hover:bg-raised hover:text-fg">
          <X size={13} />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3" style={{ scrollbarGutter: "stable" }}>
        <h2 className="select-text break-words text-[13.5px] font-medium leading-snug">{r.title}</h2>
        {tags.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">
            {tags.map((t) => (
              <span key={t} className="num rounded bg-line px-1.5 py-px text-[10.5px] text-dim">
                {t}
              </span>
            ))}
          </div>
        )}

        <div className="mt-3 flex gap-2">
          <button
            onClick={() => onOpen(r)}
            className="chrome flex h-8 flex-1 items-center justify-center gap-1.5 rounded-md bg-accent text-[12px] font-medium text-bg transition-colors duration-150 hover:bg-[#4fe0f5] active:bg-[#1bb8d1]"
          >
            <Magnet size={13} /> Open magnet
          </button>
          <button
            onClick={() => onCopy(r.magnet, "Magnet link")}
            className="chrome flex h-8 items-center gap-1.5 rounded-md border border-line px-3 text-[12px] text-dim transition-colors duration-150 hover:border-[#2e333d] hover:text-fg active:bg-raised"
          >
            <Copy size={12} /> Copy
          </button>
          <button
            onClick={() => onBookmark(r)}
            aria-label={bookmarked ? "Remove bookmark" : "Bookmark"}
            title={bookmarked ? "Remove bookmark" : "Bookmark"}
            className={`chrome flex h-8 w-8 items-center justify-center rounded-md border border-line transition-colors duration-150 hover:border-[#2e333d] active:bg-raised ${bookmarked ? "text-accent" : "text-dim hover:text-fg"}`}
          >
            {bookmarked ? <BookmarkCheck size={13} /> : <Bookmark size={13} />}
          </button>
        </div>
        {client && (
          <button
            onClick={() => onSend(r)}
            className="chrome mt-2 flex h-7 w-full items-center justify-center gap-1.5 rounded-md border border-line text-[12px] text-dim transition-colors duration-150 hover:border-[#2e333d] hover:text-fg active:bg-raised"
          >
            <Send size={12} /> Send to {client}
          </button>
        )}

        {r.flags.length > 0 && (
          <div className="mt-3 space-y-1 rounded-md border border-warn/25 bg-warn/5 p-2">
            {r.flags.map((f) => (
              <div key={f} className="flex items-center gap-1.5 text-[11.5px] text-warn">
                <AlertTriangle size={11} /> {FLAG_LABEL[f] ?? f}
              </div>
            ))}
          </div>
        )}

        <dl className="mt-3 grid grid-cols-[88px_1fr] gap-y-1 text-[12px]">
          <Stat label="Size" value={formatSize(r.size)} />
          <Stat
            label="Seed / Leech"
            value={
              <span className="inline-flex items-center gap-1">
                <span className={seeds ? "text-good" : "text-bad"}>{formatCount(seeds)}</span>/<span>{formatCount(leech)}</span>
                {r.live && (
                  <span className="ml-1 inline-flex items-center gap-1 text-[10.5px] text-accent" title={`${r.live.trackersOk} tracker(s) answered`}>
                    <Radio size={10} /> live
                  </span>
                )}
              </span>
            }
          />
          {(r.live?.completed ?? r.completed) != null && <Stat label="Completed" value={formatCount(r.live?.completed ?? r.completed)} />}
          <Stat label="Uploaded" value={r.uploadedAt ? `${new Date(r.uploadedAt * 1000).toISOString().slice(0, 10)} · ${formatAge(r.uploadedAt)} ago` : "—"} />
          {r.uploader && <Stat label="Uploader" value={`${r.uploader}${r.verifiedUploader ? " ✓" : ""}`} />}
          <Stat label="Category" value={r.category ?? "—"} />
          <Stat label="Score" value={r.score.toFixed(3)} />
          <dt className="text-faint">Infohash</dt>
          <dd>
            <button onClick={() => onCopy(r.infoHash, "Infohash")} title="Copy infohash" className="num break-all text-left text-[11px] text-dim transition-colors duration-150 hover:text-accent">
              {r.infoHash}
            </button>
          </dd>
        </dl>

        <Section title={`Sources (${r.sources.length})`}>
          {r.sources.map((s) => (
            <div key={s.indexerId} className="flex items-center gap-2 py-0.5 text-[12px]">
              <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${KIND_DOT[s.sourceType]}`} title={s.sourceType} />
              <span className="flex-1 truncate">{s.indexerName}</span>
              <span className="num text-[11px] text-faint">
                {formatCount(s.seeders)}/{formatCount(s.leechers)}
              </span>
            </div>
          ))}
        </Section>

        <Section title={`Files (${r.files.length || "unknown"})`} defaultOpen={r.files.length > 0 && r.files.length <= 200}>
          {r.files.length ? <FileTree files={r.files} /> : <p className="text-[11.5px] text-faint">This source doesn't list files.</p>}
        </Section>

        <Section title={`Trackers (${r.trackers.length})`} defaultOpen={false}>
          {r.trackers.map((t) => (
            <div key={t} className="num select-text truncate py-px text-[11px] text-dim" title={t}>
              {t}
            </div>
          ))}
        </Section>
      </div>
    </aside>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <>
      <dt className="text-faint">{label}</dt>
      <dd className="num text-fg">{value}</dd>
    </>
  );
}

function Section({ title, children, defaultOpen = true }: { title: string; children: React.ReactNode; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className="mt-3 border-t border-line pt-2">
      <button onClick={() => setOpen(!open)} className="chrome flex w-full items-center gap-1 text-[11px] uppercase tracking-wide text-faint transition-colors duration-150 hover:text-fg">
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {title}
      </button>
      {open && <div className="mt-1.5">{children}</div>}
    </section>
  );
}

interface Node {
  name: string;
  size: number;
  children: Map<string, Node>;
  file: boolean;
}

function buildTree(files: TorrentFile[]): Node {
  const root: Node = { name: "", size: 0, children: new Map(), file: false };
  for (const f of files) {
    const parts = f.path.split(/[\\/]/).filter(Boolean);
    let n = root;
    n.size += f.size;
    parts.forEach((p, i) => {
      let c = n.children.get(p);
      if (!c) {
        c = { name: p, size: 0, children: new Map(), file: i === parts.length - 1 };
        n.children.set(p, c);
      }
      c.size += f.size;
      n = c;
    });
  }
  return root;
}

function FileTree({ files }: { files: TorrentFile[] }) {
  const tree = useMemo(() => buildTree(files), [files]);
  return (
    <div className="text-[11.5px]">
      {[...tree.children.values()].map((n) => (
        <TreeNode key={n.name} n={n} depth={0} />
      ))}
    </div>
  );
}

function TreeNode({ n, depth }: { n: Node; depth: number }) {
  const [open, setOpen] = useState(depth < 1);
  const kids = [...n.children.values()].sort((a, b) => Number(a.file) - Number(b.file) || a.name.localeCompare(b.name));
  return (
    <div>
      <div
        onClick={() => !n.file && setOpen(!open)}
        className={`flex items-center gap-1 rounded py-px pr-1 ${n.file ? "" : "cursor-pointer hover:bg-raised"}`}
        style={{ paddingLeft: depth * 12 }}
      >
        {n.file ? <File size={11} className="shrink-0 text-faint" /> : <Folder size={11} className="shrink-0 text-accent/70" />}
        <span className="flex-1 select-text truncate" title={n.name}>
          {n.name}
        </span>
        <span className="num shrink-0 text-faint">{formatSize(n.size)}</span>
      </div>
      {!n.file && open && kids.map((c) => <TreeNode key={c.name} n={c} depth={depth + 1} />)}
    </div>
  );
}
