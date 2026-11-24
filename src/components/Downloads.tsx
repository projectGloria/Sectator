// The built-in downloader: live list of libtorrent downloads with per-file selection.
import { useEffect, useState } from "react";
import { Check, Download as DownloadIcon, FolderOpen, Pause, Play, RotateCw, Trash2, X } from "lucide-react";
import { api } from "../lib/api";
import { formatSize } from "../lib/format";
import type { Download, DownloadFile, DownloadState } from "../lib/types";
import { Button } from "./ui";

const STATE: Record<DownloadState, [string, string]> = {
  checking: ["Checking", "text-warn"],
  metadata: ["Fetching metadata", "text-warn"],
  downloading: ["Downloading", "text-accent"],
  seeding: ["Seeding", "text-good"],
  paused: ["Paused", "text-faint"],
  completed: ["Completed", "text-good"],
  error: ["Error", "text-bad"],
};

const BAR: Record<DownloadState, string> = {
  checking: "bg-warn",
  metadata: "bg-warn",
  downloading: "bg-accent",
  seeding: "bg-good",
  paused: "bg-faint",
  completed: "bg-good",
  error: "bg-bad",
};

export function formatRate(bps: number): string {
  return bps > 0 ? `${formatSize(bps)}/s` : "—";
}

export function formatEta(secs: number | null): string {
  if (secs == null) return "—";
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${String(secs % 60).padStart(2, "0")}s`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ${String(Math.floor((secs % 3600) / 60)).padStart(2, "0")}m`;
  return `${Math.floor(secs / 86400)}d ${Math.floor((secs % 86400) / 3600)}h`;
}

const running = (d: Download) => d.state !== "paused" && d.state !== "completed" && d.state !== "error";

interface Props {
  items: Download[];
  onChanged: () => void;
  onNotify: (msg: string) => void;
}

const COLS = "grid-cols-[minmax(0,1fr)_76px_52px_84px_84px_76px_64px_52px_120px_104px]";

export function Downloads({ items, onChanged, onNotify }: Props) {
  const [selected, setSelected] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const current = items.find((d) => d.infoHash === selected) ?? null;

  const act = (p: Promise<unknown>, ok?: string) =>
    p.then(
      () => {
        onChanged();
        if (ok) onNotify(ok);
      },
      (e) => onNotify(String(e)),
    );

  const remove = (d: Download, deleteFiles: boolean) => {
    setConfirm(null);
    if (selected === d.infoHash) setSelected(null);
    act(api.removeDownload(d.infoHash, deleteFiles), deleteFiles ? "Removed with its files" : "Removed — files kept");
  };

  if (!items.length) {
    return (
      <div className="chrome flex min-w-0 flex-1 flex-col items-center justify-center gap-3 text-dim">
        <DownloadIcon size={22} className="text-faint" />
        <p className="text-[12.5px]">No downloads yet. Select a result and press Download (or D).</p>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className={`chrome grid ${COLS} h-7 shrink-0 items-center gap-2 border-b border-line bg-panel px-3 text-[11px] uppercase tracking-wide text-faint`}>
        <span>Name</span>
        <span className="text-right">Size</span>
        <span className="text-right">Done</span>
        <span className="text-right">Down</span>
        <span className="text-right">Up</span>
        <span className="text-right">ETA</span>
        <span className="text-right" title="Connected seeds / peers">Peers</span>
        <span className="text-right">Ratio</span>
        <span>State</span>
        <span />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto" style={{ scrollbarGutter: "stable" }}>
        {items.map((d) => {
          const [label, tone] = STATE[d.state];
          const sel = d.infoHash === selected;
          return (
            <div
              key={d.infoHash}
              onClick={() => setSelected(sel ? null : d.infoHash)}
              className={`grid ${COLS} min-h-[38px] cursor-default items-center gap-2 border-b border-line/60 px-3 text-[12px] transition-colors duration-150 ${sel ? "bg-raised" : "hover:bg-raised/60"}`}
            >
              <div className="min-w-0">
                <div className="truncate" title={d.name}>
                  {d.name}
                </div>
                <div className="mt-1 h-[3px] overflow-hidden rounded-full bg-line">
                  <div className={`h-full rounded-full transition-[width] duration-500 ${BAR[d.state]}`} style={{ width: `${Math.round(d.progress * 1000) / 10}%` }} />
                </div>
              </div>
              <span className="num text-right text-dim">{formatSize(d.size)}</span>
              <span className="num text-right">{d.state === "metadata" ? "—" : `${Math.floor(d.progress * 100)}%`}</span>
              <span className="num text-right text-accent">{formatRate(d.downRate)}</span>
              <span className="num text-right text-good">{formatRate(d.upRate)}</span>
              <span className="num text-right text-dim">{d.state === "downloading" ? formatEta(d.etaSecs) : "—"}</span>
              <span className="num text-right text-dim" title={`Swarm: ${d.swarmSeeds} seeds, ${d.swarmPeers} peers`}>
                {running(d) ? `${d.seeds}/${d.peers}` : "—"}
              </span>
              <span className="num text-right text-dim">{d.ratio.toFixed(2)}</span>
              <span className={`truncate ${tone}`} title={d.error ?? label}>
                {d.error ? d.error : label}
              </span>
              <div className="flex justify-end gap-0.5" onClick={(e) => e.stopPropagation()}>
                {confirm === d.infoHash ? (
                  <>
                    <RowButton label="Remove, keep files" onClick={() => remove(d, false)}>
                      <Check size={13} />
                    </RowButton>
                    <RowButton label="Remove and delete files" danger onClick={() => remove(d, true)}>
                      <Trash2 size={13} />
                    </RowButton>
                    <RowButton label="Cancel" onClick={() => setConfirm(null)}>
                      <X size={13} />
                    </RowButton>
                  </>
                ) : (
                  <>
                    {running(d) ? (
                      <RowButton label="Pause" onClick={() => act(api.pauseDownload(d.infoHash))}>
                        <Pause size={13} />
                      </RowButton>
                    ) : (
                      <RowButton label={d.state === "completed" ? "Seed again" : "Resume"} onClick={() => act(api.resumeDownload(d.infoHash))}>
                        <Play size={13} />
                      </RowButton>
                    )}
                    <RowButton label="Show in folder" disabled={d.state === "metadata"} onClick={() => act(api.revealDownload(d.infoHash))}>
                      <FolderOpen size={13} />
                    </RowButton>
                    <RowButton label="Remove…" onClick={() => setConfirm(d.infoHash)}>
                      <Trash2 size={13} />
                    </RowButton>
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>
      {current && <Details d={current} onChanged={onChanged} onNotify={onNotify} act={act} />}
    </div>
  );
}

function Details({ d, act }: { d: Download; onChanged: () => void; onNotify: (m: string) => void; act: (p: Promise<unknown>, ok?: string) => void }) {
  const [files, setFiles] = useState<DownloadFile[] | null>(null);
  const hash = d.infoHash;
  const hasMeta = d.state !== "metadata";

  useEffect(() => {
    setFiles(null);
    if (!hasMeta) return;
    let live = true;
    const load = () => api.downloadFiles(hash).then((f) => live && setFiles(f), () => {});
    load();
    const t = window.setInterval(load, 2000);
    return () => {
      live = false;
      window.clearInterval(t);
    };
  }, [hash, hasMeta]);

  const toggle = (changes: [number, boolean][]) => {
    setFiles((fs) => fs && fs.map((f) => (changes.some(([i]) => i === f.index) ? { ...f, wanted: changes.find(([i]) => i === f.index)![1] } : f)));
    act(api.setWanted(hash, changes));
  };
  const allWanted = !!files?.length && files.every((f) => f.wanted);

  return (
    <div className="flex h-[38%] min-h-[140px] shrink-0 flex-col border-t border-line bg-panel">
      <div className="chrome flex h-8 shrink-0 items-center gap-3 border-b border-line px-3 text-[11.5px] text-dim">
        <span className="num truncate text-faint" title={d.savePath}>
          {d.savePath}
        </span>
        <span className="num ml-auto shrink-0">
          ↓ {formatSize(d.downloaded)} · ↑ {formatSize(d.uploaded)}
        </span>
        <Button tone="ghost" className="shrink-0" disabled={!hasMeta} onClick={() => act(api.recheckDownload(hash), "Rechecking files")}>
          <RotateCw size={12} /> Recheck
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto" style={{ scrollbarGutter: "stable" }}>
        {!hasMeta ? (
          <p className="chrome p-3 text-[12px] text-faint">Waiting for peers to send the torrent's metadata…</p>
        ) : !files ? null : (
          <>
            <FileRow
              name={`${files.length} file${files.length === 1 ? "" : "s"}`}
              size={files.reduce((n, f) => n + f.size, 0)}
              done={files.reduce((n, f) => n + f.done, 0)}
              wanted={allWanted}
              head
              onToggle={() => toggle(files.map((f) => [f.index, !allWanted]))}
            />
            {files.map((f) => (
              <FileRow key={f.index} name={f.path} size={f.size} done={f.done} wanted={f.wanted} onToggle={() => toggle([[f.index, !f.wanted]])} />
            ))}
          </>
        )}
      </div>
    </div>
  );
}

function FileRow({ name, size, done, wanted, head, onToggle }: { name: string; size: number; done: number; wanted: boolean; head?: boolean; onToggle: () => void }) {
  const pct = size ? Math.floor((done / size) * 100) : 100;
  return (
    <div className={`grid grid-cols-[22px_minmax(0,1fr)_80px_48px] items-center gap-2 px-3 py-1 text-[12px] ${head ? "chrome border-b border-line/60 text-faint" : wanted ? "" : "text-faint"}`}>
      <button
        type="button"
        role="checkbox"
        aria-checked={wanted}
        aria-label={head ? "Download all files" : `Download ${name}`}
        onClick={onToggle}
        className={`flex h-3.5 w-3.5 items-center justify-center rounded-[3px] border transition-colors duration-150 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-1 focus-visible:outline-accent ${
          wanted ? "border-accent bg-accent text-bg hover:bg-[#4fe0f5]" : "border-line hover:border-[#3a404c] active:bg-line"
        }`}
      >
        {wanted && <Check size={10} strokeWidth={3} />}
      </button>
      <span className="num truncate select-text" title={name}>
        {name}
      </span>
      <span className="num text-right text-dim">{formatSize(size)}</span>
      <span className="num text-right text-dim">{pct}%</span>
    </div>
  );
}

function RowButton(props: { label: string; danger?: boolean; disabled?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-label={props.label}
      title={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
      className={`flex h-6 w-6 items-center justify-center rounded text-dim transition-colors duration-150 hover:text-fg focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent disabled:pointer-events-none disabled:opacity-40 ${
        props.danger ? "hover:bg-bad/15 hover:text-bad active:bg-bad/25" : "hover:bg-line active:bg-[#2e333d]"
      }`}
    >
      {props.children}
    </button>
  );
}
