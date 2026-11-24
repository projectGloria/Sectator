import { useCallback, useEffect, useState } from "react";
import { Bookmark as BookmarkIcon, Clock, Magnet, Plus, Star, Trash2, X } from "lucide-react";
import { api } from "../lib/api";
import { formatAge, formatSize } from "../lib/format";
import type { Bookmark, HistoryEntry, Preset } from "../lib/types";
import { Button, TextInput } from "./ui";

type Tab = "history" | "presets" | "bookmarks";

interface Props {
  /** Bumped by the parent whenever history/bookmarks may have changed. */
  refreshKey: number;
  query: string;
  onRun: (query: string) => void;
  onOpenMagnet: (magnet: string) => void;
  onNotify: (msg: string) => void;
  onClose: () => void;
}

export function Sidebar({ refreshKey, query, onRun, onOpenMagnet, onNotify, onClose }: Props) {
  const [tab, setTab] = useState<Tab>("history");
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [presets, setPresets] = useState<Preset[]>([]);
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  const [name, setName] = useState("");

  const load = useCallback(() => {
    api.history().then(setHistory, () => {});
    api.presets().then(setPresets, () => {});
    api.bookmarks().then(setBookmarks, () => {});
  }, []);
  useEffect(load, [load, refreshKey]);

  const fail = (e: unknown) => onNotify(String(e));

  return (
    <aside className="chrome flex w-[260px] shrink-0 flex-col border-r border-line bg-panel">
      <div className="flex h-7 shrink-0 items-center border-b border-line text-[11px]">
        {(
          [
            ["history", "History", Clock],
            ["presets", "Presets", Star],
            ["bookmarks", "Saved", BookmarkIcon],
          ] as const
        ).map(([id, label, Icon]) => (
          <button
            key={id}
            onClick={() => setTab(id)}
            className={`flex h-full items-center gap-1 px-2.5 uppercase tracking-wide transition-colors duration-150 ${tab === id ? "text-fg shadow-[inset_0_-1px_0_var(--color-accent)]" : "text-faint hover:text-fg"}`}
          >
            <Icon size={11} /> {label}
          </button>
        ))}
        <button aria-label="Close sidebar" onClick={onClose} className="ml-auto flex h-full w-8 items-center justify-center text-faint transition-colors duration-150 hover:bg-raised hover:text-fg">
          <X size={13} />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-1.5">
        {tab === "history" && (
          <>
            {history.map((h) => (
              <Row key={h.query} onClick={() => onRun(h.query)} onDelete={() => api.deleteHistory(h.query).then(load, fail)} title={`${h.uses} use(s)`}>
                <span className="num flex-1 truncate text-[12px]">{h.query}</span>
                <span className="num shrink-0 text-[10.5px] text-faint">{formatAge(h.lastUsed)}</span>
              </Row>
            ))}
            {history.length > 0 ? (
              <Button tone="ghost" className="mt-1 w-full" onClick={() => api.deleteHistory().then(load, fail)}>
                <Trash2 size={11} /> Clear history
              </Button>
            ) : (
              <Empty>Searches you run show up here.</Empty>
            )}
          </>
        )}

        {tab === "presets" && (
          <>
            <form
              className="mb-1.5 flex gap-1"
              onSubmit={(e) => {
                e.preventDefault();
                api.savePreset(name, query).then(() => {
                  setName("");
                  load();
                }, fail);
              }}
            >
              <TextInput value={name} onChange={(e) => setName(e.target.value)} placeholder="Save current query as…" className="flex-1" />
              <Button type="submit" disabled={!name.trim() || !query.trim()} aria-label="Save preset">
                <Plus size={12} />
              </Button>
            </form>
            {presets.map((p) => (
              <Row key={p.name} onClick={() => onRun(p.query)} onDelete={() => api.deletePreset(p.name).then(load, fail)} title={p.query}>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[12px]">{p.name}</span>
                  <span className="num block truncate text-[10.5px] text-faint">{p.query}</span>
                </span>
              </Row>
            ))}
            {presets.length === 0 && <Empty>Name a query to keep it one click away.</Empty>}
          </>
        )}

        {tab === "bookmarks" && (
          <>
            {bookmarks.map((b) => (
              <Row key={b.infoHash} onClick={() => onOpenMagnet(b.magnet)} onDelete={() => api.removeBookmark(b.infoHash).then(load, fail)} title={`${b.title}\n${b.infoHash}`}>
                <Magnet size={11} className="shrink-0 text-accent/70" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[12px]">{b.title}</span>
                  <span className="num block text-[10.5px] text-faint">{formatSize(b.size)}</span>
                </span>
              </Row>
            ))}
            {bookmarks.length === 0 && <Empty>Bookmark a result from the inspector to keep it here.</Empty>}
          </>
        )}
      </div>
    </aside>
  );
}

function Row({ children, onClick, onDelete, title }: { children: React.ReactNode; onClick: () => void; onDelete: () => void; title?: string }) {
  return (
    <div
      role="button"
      tabIndex={0}
      title={title}
      onClick={onClick}
      onKeyDown={(e) => e.key === "Enter" && onClick()}
      className="group flex cursor-pointer items-center gap-2 rounded px-2 py-1 transition-colors duration-150 hover:bg-raised active:bg-line"
    >
      {children}
      <button
        aria-label="Remove"
        onClick={(e) => {
          e.stopPropagation();
          onDelete();
        }}
        className="shrink-0 rounded p-0.5 text-faint opacity-0 transition-opacity duration-150 hover:text-bad focus-visible:opacity-100 group-hover:opacity-100"
      >
        <X size={11} />
      </button>
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="px-2 py-3 text-[11.5px] text-faint">{children}</p>;
}
