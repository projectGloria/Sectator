// Pre-download confirmation modal: fetches magnet metadata, lets the user pick files,
// toggle sequential download order, and choose the save folder before starting.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertCircle, Check, CheckCircle2, Download, FolderOpen, ListOrdered, Loader2, Search, X } from "lucide-react";
import { api } from "../lib/api";
import { formatSize } from "../lib/format";
import type { DownloadPreview, TorrentResult } from "../lib/types";
import { FileIcon } from "./KindIcon";
import { Button, TextInput, Toggle } from "./ui";

interface Props {
  result: TorrentResult;
  defaultDir: string | null;
  onStarted: (infoHash: string, updated: boolean) => void;
  onClose: () => void;
  onNotify: (msg: string) => void;
}

export function DownloadModal({ result, defaultDir, onStarted, onClose, onNotify }: Props) {
  const [infoHash, setInfoHash] = useState<string | null>(null);
  const [alreadyExists, setAlreadyExists] = useState(false);
  const [preview, setPreview] = useState<DownloadPreview | null>(null);
  const [startError, setStartError] = useState<string | null>(null);
  const [savePath, setSavePath] = useState(defaultDir ?? "");
  const [sequential, setSequential] = useState(false);
  const [wanted, setWanted] = useState<Record<number, boolean>>({});
  const [filter, setFilter] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const hashRef = useRef<string | null>(null);
  const existsRef = useRef(false);
  const confirmedRef = useRef(false);
  const initializedFilesRef = useRef(false);
  const initializedOptsRef = useRef(false);

  // Stage the magnet on mount so libtorrent fetches its metadata without downloading payload.
  useEffect(() => {
    let alive = true;
    api
      .downloadPreviewStart(result.magnet, defaultDir ?? undefined)
      .then((res) => {
        if (!alive) {
          if (!res.alreadyExists) api.downloadPreviewCancel(res.infoHash).catch(() => {});
          return;
        }
        hashRef.current = res.infoHash;
        existsRef.current = res.alreadyExists;
        setInfoHash(res.infoHash);
        setAlreadyExists(res.alreadyExists);
      })
      .catch((e) => {
        if (alive) setStartError(String(e));
      });
    return () => {
      alive = false;
      if (hashRef.current && !confirmedRef.current && !existsRef.current) {
        api.downloadPreviewCancel(hashRef.current).catch(() => {});
      }
    };
  }, [result.magnet, defaultDir]);

  // Poll metadata & files until fetched (and while open, keep peer counts fresh before metadata arrives).
  useEffect(() => {
    if (!infoHash) return;
    let alive = true;
    const poll = () => {
      api
        .downloadPreviewStatus(infoHash)
        .then((p) => {
          if (!alive) return;
          setPreview(p);
          if (!initializedOptsRef.current) {
            initializedOptsRef.current = true;
            if (p.savePath && !defaultDir) setSavePath(p.savePath);
            if (existsRef.current) setSequential(p.sequential);
          }
          if (p.hasMetadata && !initializedFilesRef.current && p.files.length > 0) {
            initializedFilesRef.current = true;
            const next: Record<number, boolean> = {};
            for (const f of p.files) {
              // Staged magnets start with priority 0 (default_dont_download), so select all by default.
              // Existing downloads keep their current wanted state.
              next[f.index] = existsRef.current ? f.wanted : true;
            }
            setWanted(next);
          }
        })
        .catch(() => {});
    };
    poll();
    const t = window.setInterval(poll, 500);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [infoHash, defaultDir]);

  const cancel = useCallback(() => {
    onClose();
  }, [onClose]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [cancel]);

  const files = preview?.files ?? [];
  const hasMeta = !!preview?.hasMetadata;

  const filteredFiles = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return files;
    return files.filter((f) => f.path.toLowerCase().includes(q));
  }, [files, filter]);

  const { selectedCount, selectedSize, totalSize, allSelected } = useMemo(() => {
    let sc = 0;
    let ss = 0;
    let ts = 0;
    for (const f of files) {
      ts += f.size;
      if (wanted[f.index] ?? true) {
        sc += 1;
        ss += f.size;
      }
    }
    return {
      selectedCount: sc,
      selectedSize: ss,
      totalSize: ts,
      allSelected: files.length > 0 && sc === files.length,
    };
  }, [files, wanted]);

  const toggleAll = () => {
    const nextVal = !allSelected;
    setWanted((prev) => {
      const next = { ...prev };
      for (const f of filteredFiles) next[f.index] = nextVal;
      return next;
    });
  };

  const toggleOne = (index: number) => {
    setWanted((prev) => ({ ...prev, [index]: !(prev[index] ?? true) }));
  };

  const confirm = () => {
    if (!infoHash || submitting) return;
    if (hasMeta && files.length > 0 && selectedCount === 0) return;
    setSubmitting(true);
    const pairs: [number, boolean][] | null =
      hasMeta && files.length > 0 ? files.map((f) => [f.index, wanted[f.index] ?? true]) : null;
    api
      .downloadPreviewConfirm(infoHash, savePath.trim() || null, pairs, sequential)
      .then(
        () => {
          confirmedRef.current = true;
          onStarted(infoHash, alreadyExists);
          onClose();
        },
        (e) => {
          setSubmitting(false);
          onNotify(`Couldn't start download: ${e}`);
        },
      );
  };

  const displayName = preview?.name && preview.name !== infoHash ? preview.name : result.title;
  const displayError = startError ?? preview?.error ?? null;
  const canStart = !!infoHash && !startError && !submitting && (!hasMeta || files.length === 0 || selectedCount > 0);

  return (
    <div
      className="absolute inset-0 z-20 flex items-center justify-center bg-black/55"
      onMouseDown={(e) => e.target === e.currentTarget && cancel()}
    >
      <div
        role="dialog"
        aria-label="Download torrent"
        className="flex h-[min(560px,88%)] w-[min(680px,94%)] flex-col overflow-hidden rounded-lg border border-line bg-panel shadow-2xl shadow-black/60"
      >
        {/* Header */}
        <header className="chrome flex h-9 shrink-0 items-center gap-2 border-b border-line pl-3 pr-1">
          <Download size={13} className="shrink-0 text-accent" />
          <span className="truncate text-[12.5px] font-medium text-fg">
            {alreadyExists ? "Configure download" : "Download torrent"}
          </span>
          <span className="ml-auto flex items-center gap-2">
            {displayError ? (
              <span className="flex items-center gap-1 rounded bg-bad/15 px-2 py-0.5 text-[11px] text-bad">
                <AlertCircle size={11} /> Error
              </span>
            ) : hasMeta ? (
              <span className="flex items-center gap-1 rounded bg-good/15 px-2 py-0.5 text-[11px] text-good">
                <CheckCircle2 size={11} /> Metadata fetched
              </span>
            ) : (
              <span className="flex items-center gap-1 rounded bg-warn/15 px-2 py-0.5 text-[11px] text-warn">
                <Loader2 size={11} className="animate-spin" /> Fetching metadata…
              </span>
            )}
            <button
              type="button"
              aria-label="Close"
              onClick={cancel}
              className="flex h-7 w-7 items-center justify-center rounded text-dim transition-colors duration-150 hover:bg-raised hover:text-fg"
            >
              <X size={14} />
            </button>
          </span>
        </header>

        {/* Torrent title & summary */}
        <div className="shrink-0 border-b border-line px-3.5 py-2.5">
          <div className="select-text break-words text-[13px] font-medium leading-snug text-fg" title={displayName}>
            {displayName}
          </div>
          <div className="chrome num mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-dim">
            <span>Size: {formatSize(hasMeta && totalSize > 0 ? totalSize : result.size)}</span>
            {hasMeta && <span>{files.length} file{files.length === 1 ? "" : "s"}</span>}
            {!hasMeta && preview && (preview.seeds > 0 || preview.peers > 0) && (
              <span>
                Connected: {preview.seeds} seeds · {preview.peers} peers
              </span>
            )}
            {alreadyExists && <span className="text-accent">Already in Downloads</span>}
          </div>
          {displayError && (
            <div className="mt-2 rounded-md border border-bad/30 bg-bad/10 px-2.5 py-1.5 text-[11.5px] text-bad">
              {displayError}
            </div>
          )}
        </div>

        {/* File selection list */}
        <div className="flex min-h-0 flex-1 flex-col">
          {hasMeta && files.length > 0 ? (
            <>
              <div className="chrome flex h-8 shrink-0 items-center gap-2 border-b border-line/70 bg-bg/40 px-3 text-[11.5px]">
                <Checkbox
                  checked={allSelected}
                  label={allSelected ? "Deselect all files" : "Select all files"}
                  onChange={toggleAll}
                />
                <span className="num text-dim">
                  {selectedCount} of {files.length} selected · {formatSize(selectedSize)}
                </span>
                {files.length > 8 && (
                  <div className="relative ml-auto w-44">
                    <Search size={11} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-faint" />
                    <input
                      value={filter}
                      onChange={(e) => setFilter(e.target.value)}
                      placeholder="Filter files…"
                      className="h-6 w-full rounded border border-line bg-bg pl-6 pr-2 text-[11px] text-fg outline-none transition-colors duration-150 placeholder:text-faint hover:border-edge focus-visible:border-accent/60"
                    />
                  </div>
                )}
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto" style={{ scrollbarGutter: "stable" }}>
                {filteredFiles.map((f) => {
                  const checked = wanted[f.index] ?? true;
                  const base = f.path.split(/[\\/]/).pop() ?? f.path;
                  const dir = /[\\/]/.test(f.path) ? f.path.replace(/[\\/][^\\/]*$/, "") : null;
                  return (
                    <div
                      key={f.index}
                      onClick={() => toggleOne(f.index)}
                      className={`chrome flex cursor-pointer items-center gap-2 border-b border-line/40 px-3 py-1.5 text-[12px] transition-colors duration-150 hover:bg-raised/70 ${
                        checked ? "text-fg" : "text-faint"
                      }`}
                    >
                      <Checkbox
                        checked={checked}
                        label={`Download ${f.path}`}
                        onChange={() => toggleOne(f.index)}
                        stopPropagation
                      />
                      <FileIcon path={f.path} size={15} />
                      <span className="num min-w-0 flex-1 truncate" title={f.path}>
                        {base}
                        {dir && <span className="ml-2 text-faint">{dir}</span>}
                      </span>
                      <span className="num shrink-0 text-[11.5px] text-dim">{formatSize(f.size)}</span>
                    </div>
                  );
                })}
              </div>
            </>
          ) : (
            <div className="chrome flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center text-dim">
              {displayError ? (
                <>
                  <AlertCircle size={20} className="text-bad" />
                  <p className="text-[12px]">Couldn't fetch metadata for this magnet.</p>
                </>
              ) : (
                <>
                  <Loader2 size={20} className="animate-spin text-warn" />
                  <p className="text-[12.5px] text-fg">Fetching torrent metadata from peers & DHT…</p>
                  <p className="max-w-[400px] text-[11.5px] text-faint">
                    Wait a moment to pick individual files, or click Start Download now to download all files as soon as metadata arrives.
                  </p>
                </>
              )}
            </div>
          )}
        </div>

        {/* Options */}
        <div className="chrome shrink-0 space-y-2 border-t border-line bg-bg/40 px-3.5 py-2.5">
          <div className="flex items-center justify-between gap-3">
            <label className="flex cursor-pointer items-center gap-2 text-[12px] text-fg" onClick={() => setSequential((v) => !v)}>
              <ListOrdered size={13} className={sequential ? "text-accent" : "text-dim"} />
              <span>Download in sequential order</span>
              <span className="text-[11px] text-faint">— fetch pieces from start to finish</span>
            </label>
            <Toggle checked={sequential} onChange={setSequential} label="Download in sequential order" />
          </div>
          <div className="flex items-center gap-2">
            <span className="flex shrink-0 items-center gap-1.5 text-[11.5px] text-dim">
              <FolderOpen size={12} /> Save to
            </span>
            <TextInput
              mono
              value={savePath}
              onChange={(e) => setSavePath(e.target.value)}
              placeholder="Default Downloads folder"
              className="flex-1"
            />
          </div>
        </div>

        {/* Footer */}
        <footer className="chrome flex h-11 shrink-0 items-center justify-between border-t border-line px-3.5">
          <span className="num text-[11px] text-faint">
            {hasMeta && files.length > 0 && selectedCount === 0
              ? "Select at least one file to download"
              : hasMeta
                ? `Ready · ${selectedCount} file${selectedCount === 1 ? "" : "s"} (${formatSize(selectedSize)})`
                : "Metadata pending…"}
          </span>
          <div className="flex items-center gap-2">
            <Button tone="ghost" onClick={cancel}>
              Cancel
            </Button>
            <Button tone="primary" disabled={!canStart} onClick={confirm}>
              <Download size={12} />
              {submitting ? "Starting…" : alreadyExists ? "Apply & Resume" : "Start Download"}
            </Button>
          </div>
        </footer>
      </div>
    </div>
  );
}

function Checkbox({
  checked,
  label,
  onChange,
  stopPropagation,
}: {
  checked: boolean;
  label: string;
  onChange: () => void;
  stopPropagation?: boolean;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={label}
      onClick={(e) => {
        if (stopPropagation) e.stopPropagation();
        onChange();
      }}
      className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-[3px] border transition-colors duration-150 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-1 focus-visible:outline-accent ${
        checked ? "border-accent bg-accent text-on-accent hover:bg-accent-hi" : "border-line hover:border-edge active:bg-line"
      }`}
    >
      {checked && <Check size={10} strokeWidth={3} />}
    </button>
  );
}
