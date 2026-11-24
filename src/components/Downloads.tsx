// The built-in downloader: live list of libtorrent downloads with per-file selection.
import { useEffect, useRef, useState } from "react";
import {
  AlertCircle,
  ArrowDown,
  ArrowUp,
  Check,
  CheckCircle2,
  Clock,
  Download as DownloadIcon,
  FolderOpen,
  HardDrive,
  Loader2,
  Pause,
  Play,
  RotateCw,
  Scale,
  Trash2,
  Upload,
  Users,
  X,
  type LucideIcon,
} from "lucide-react";
import { api } from "../lib/api";
import { formatSize } from "../lib/format";
import type { Download, DownloadFile, DownloadState } from "../lib/types";
import { FileIcon, KindIcon } from "./KindIcon";
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

/** Icon, colour and whether it spins, per download state. */
export const STATE_ICON: Record<DownloadState, [LucideIcon, string, boolean]> =
  {
    checking: [Loader2, "text-warn", true],
    metadata: [Loader2, "text-warn", true],
    downloading: [ArrowDown, "text-accent", false],
    seeding: [ArrowUp, "text-good", false],
    paused: [Pause, "text-faint", false],
    completed: [CheckCircle2, "text-good", false],
    error: [AlertCircle, "text-bad", false],
  };

export const BAR: Record<DownloadState, string> = {
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
  if (secs < 3600)
    return `${Math.floor(secs / 60)}m ${String(secs % 60).padStart(2, "0")}s`;
  if (secs < 86400)
    return `${Math.floor(secs / 3600)}h ${String(Math.floor((secs % 3600) / 60)).padStart(2, "0")}m`;
  return `${Math.floor(secs / 86400)}d ${Math.floor((secs % 86400) / 3600)}h`;
}

const running = (d: Download) =>
  d.state !== "paused" && d.state !== "completed" && d.state !== "error";

interface Props {
  items: Download[];
  /** Download whose details are open (owned by App so the bottom bar can open one). */
  selected: string | null;
  onSelect: (hash: string | null) => void;
  onChanged: () => void;
  onNotify: (msg: string) => void;
}

const COLS =
  "grid-cols-[minmax(0,1fr)_76px_52px_84px_84px_76px_64px_52px_120px_104px]";

export function Downloads({
  items,
  selected,
  onSelect: setSelected,
  onChanged,
  onNotify,
}: Props) {
  const list = useRef<HTMLDivElement>(null);
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

  // Opened from the bottom bar: bring the row into view.
  useEffect(() => {
    if (!selected) return;
    list.current
      ?.querySelector(`[data-hash="${selected}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const remove = (d: Download, deleteFiles: boolean) => {
    setConfirm(null);
    if (selected === d.infoHash) setSelected(null);
    act(
      api.removeDownload(d.infoHash, deleteFiles),
      deleteFiles ? "Removed with its files" : "Removed — files kept",
    );
  };

  if (!items.length) {
    return (
      <div className="chrome flex min-w-0 flex-1 flex-col items-center justify-center gap-3 text-dim">
        <DownloadIcon size={22} className="text-faint" />
        <p className="text-[12.5px]">
          No downloads yet. Select a result and press Download (or D).
        </p>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div
        className={`chrome grid ${COLS} h-7 shrink-0 items-center gap-2 border-b border-line bg-panel px-3 text-[11px] uppercase tracking-wide text-faint`}
      >
        <span>Name</span>
        <Head icon={HardDrive} label="Size" />
        <Head icon={CheckCircle2} label="Done" />
        <Head icon={ArrowDown} label="Download rate" />
        <Head icon={ArrowUp} label="Upload rate" />
        <Head icon={Clock} label="Time left" />
        <Head icon={Users} label="Connected seeds / peers" />
        <Head icon={Scale} label="Share ratio" />
        <span>State</span>
        <span />
      </div>
      <div
        ref={list}
        className="min-h-0 flex-1 overflow-y-auto"
        style={{ scrollbarGutter: "stable" }}
      >
        {items.map((d) => {
          const [label, tone] = STATE[d.state];
          const sel = d.infoHash === selected;
          return (
            <div
              key={d.infoHash}
              data-hash={d.infoHash}
              onClick={() => setSelected(sel ? null : d.infoHash)}
              className={`grid ${COLS} min-h-[38px] cursor-default items-center gap-2 border-b border-line/60 px-3 text-[12px] transition-colors duration-150 ${sel ? "bg-raised" : "hover:bg-raised/60"}`}
            >
              <div className="flex min-w-0 items-center gap-2.5">
                <KindIcon kind={d.kind} ext={d.mainExt} size={20} />
                <div className="min-w-0 flex-1">
                  <div className="truncate" title={d.name}>
                    {d.name}
                  </div>
                  <div className="mt-1 h-[3px] overflow-hidden rounded-full bg-line">
                    <div
                      className={`h-full rounded-full transition-[width] duration-500 ${BAR[d.state]}`}
                      style={{
                        width: `${Math.round(d.progress * 1000) / 10}%`,
                      }}
                    />
                  </div>
                </div>
              </div>
              <span className="num text-right text-dim">
                {formatSize(d.size)}
              </span>
              <span className="num text-right">
                {d.state === "metadata"
                  ? "—"
                  : `${Math.floor(d.progress * 100)}%`}
              </span>
              <span className="num text-right text-accent">
                {formatRate(d.downRate)}
              </span>
              <span className="num text-right text-good">
                {formatRate(d.upRate)}
              </span>
              <span className="num text-right text-dim">
                {d.state === "downloading" ? formatEta(d.etaSecs) : "—"}
              </span>
              <span
                className="num text-right text-dim"
                title={`Swarm: ${d.swarmSeeds} seeds, ${d.swarmPeers} peers`}
              >
                {running(d) ? `${d.seeds}/${d.peers}` : "—"}
              </span>
              <span className="num text-right text-dim">
                {d.ratio.toFixed(2)}
              </span>
              <StateLabel d={d} label={label} tone={tone} />
              <div
                className="flex justify-end gap-0.5"
                onClick={(e) => e.stopPropagation()}
              >
                {confirm === d.infoHash ? (
                  <>
                    <RowButton
                      label="Remove, keep files"
                      onClick={() => remove(d, false)}
                    >
                      <Check size={13} />
                    </RowButton>
                    <RowButton
                      label="Remove and delete files"
                      danger
                      onClick={() => remove(d, true)}
                    >
                      <Trash2 size={13} />
                    </RowButton>
                    <RowButton label="Cancel" onClick={() => setConfirm(null)}>
                      <X size={13} />
                    </RowButton>
                  </>
                ) : (
                  <>
                    {running(d) ? (
                      <RowButton
                        label="Pause"
                        onClick={() => act(api.pauseDownload(d.infoHash))}
                      >
                        <Pause size={13} />
                      </RowButton>
                    ) : (
                      <RowButton
                        label={
                          d.state === "completed" ? "Seed again" : "Resume"
                        }
                        onClick={() => act(api.resumeDownload(d.infoHash))}
                      >
                        <Play size={13} />
                      </RowButton>
                    )}
                    <RowButton
                      label="Show in folder"
                      disabled={d.state === "metadata"}
                      onClick={() => act(api.revealDownload(d.infoHash))}
                    >
                      <FolderOpen size={13} />
                    </RowButton>
                    <RowButton
                      label="Remove…"
                      onClick={() => setConfirm(d.infoHash)}
                    >
                      <Trash2 size={13} />
                    </RowButton>
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>
      {current && (
        <Details d={current} act={act} onClose={() => setSelected(null)} />
      )}
    </div>
  );
}

function Details({
  d,
  act,
  onClose,
}: {
  d: Download;
  act: (p: Promise<unknown>, ok?: string) => void;
  onClose: () => void;
}) {
  const [files, setFiles] = useState<DownloadFile[] | null>(null);
  const hash = d.infoHash;
  const hasMeta = d.state !== "metadata";
  const [label, tone] = STATE[d.state];

  useEffect(() => {
    setFiles(null);
    if (!hasMeta) return;
    let live = true;
    const load = () =>
      api.downloadFiles(hash).then(
        (f) => live && setFiles(f),
        () => {},
      );
    load();
    const t = window.setInterval(load, 1500);
    return () => {
      live = false;
      window.clearInterval(t);
    };
  }, [hash, hasMeta]);

  const toggle = (changes: [number, boolean][]) => {
    setFiles(
      (fs) =>
        fs &&
        fs.map((f) =>
          changes.some(([i]) => i === f.index)
            ? { ...f, wanted: changes.find(([i]) => i === f.index)![1] }
            : f,
        ),
    );
    act(api.setWanted(hash, changes));
  };
  const allWanted = !!files?.length && files.every((f) => f.wanted);
  const finished = files?.filter((f) => f.wanted && f.done >= f.size).length ?? 0;
  const wanted = files?.filter((f) => f.wanted).length ?? 0;
  const pct = Math.round(d.progress * 1000) / 10;

  return (
    <div className="flex h-[46%] min-h-[200px] shrink-0 flex-col border-t border-line bg-panel">
      <div className="chrome flex shrink-0 items-center gap-2.5 px-3 pt-2.5">
        <KindIcon kind={d.kind} ext={d.mainExt} size={22} />
        <span className="min-w-0 flex-1 truncate text-[13px] text-fg select-text" title={d.name}>
          {d.name}
        </span>
        <StateLabel d={d} label={label} tone={tone} />
        <Button
          tone="ghost"
          className="shrink-0"
          disabled={!hasMeta}
          onClick={() => act(api.recheckDownload(hash), "Rechecking files")}
        >
          <RotateCw size={12} /> Recheck
        </Button>
        <RowButton label="Close details" onClick={onClose}>
          <X size={13} />
        </RowButton>
      </div>

      <div className="chrome shrink-0 px-3 pt-2">
        <div className="h-[5px] overflow-hidden rounded-full bg-line">
          <div
            className={`h-full rounded-full transition-[width] duration-500 ${BAR[d.state]}`}
            style={{ width: `${pct}%` }}
          />
        </div>
        <div className="num mt-1 flex justify-between text-[11px] text-dim">
          <span>
            {d.state === "metadata"
              ? "Waiting for metadata"
              : `${formatSize(d.done)} of ${formatSize(d.size)}`}
          </span>
          <span>{d.state === "metadata" ? "—" : `${pct}%`}</span>
        </div>
      </div>

      <div className="chrome grid shrink-0 grid-cols-4 gap-x-4 gap-y-1.5 border-b border-line px-3 py-2 text-[11.5px] xl:grid-cols-8">
        <Stat icon={ArrowDown} label="Download" tone="text-accent" value={formatRate(d.downRate)} />
        <Stat icon={ArrowUp} label="Upload" tone="text-good" value={formatRate(d.upRate)} />
        <Stat
          icon={Clock}
          label="Time left"
          value={d.state === "downloading" ? formatEta(d.etaSecs) : "—"}
        />
        <Stat
          icon={Users}
          label="Seeds / peers"
          value={`${d.seeds} / ${d.peers}`}
          title={`Connected ${d.seeds} seeds and ${d.peers} peers; the swarm reports ${d.swarmSeeds} seeds and ${d.swarmPeers} peers`}
        />
        <Stat icon={DownloadIcon} label="Downloaded" value={formatSize(d.downloaded)} />
        <Stat icon={Upload} label="Uploaded" value={formatSize(d.uploaded)} />
        <Stat icon={Scale} label="Ratio" value={d.ratio.toFixed(2)} />
        <Stat
          icon={FolderOpen}
          label="Saved to"
          value={d.savePath}
          title={d.savePath}
        />
      </div>

      <div
        className="min-h-0 flex-1 overflow-y-auto"
        style={{ scrollbarGutter: "stable" }}
      >
        {!hasMeta ? (
          <p className="chrome p-3 text-[12px] text-faint">
            Waiting for peers to send the torrent's metadata…
          </p>
        ) : !files ? null : (
          <>
            <FileRow
              name={`${files.length} file${files.length === 1 ? "" : "s"} · ${finished} of ${wanted} done`}
              size={files.reduce((n, f) => n + f.size, 0)}
              done={files.reduce((n, f) => n + f.done, 0)}
              wanted={allWanted}
              head
              onToggle={() => toggle(files.map((f) => [f.index, !allWanted]))}
            />
            {files.map((f) => (
              <FileRow
                key={f.index}
                name={f.path}
                size={f.size}
                done={f.done}
                wanted={f.wanted}
                state={d.state}
                onToggle={() => toggle([[f.index, !f.wanted]])}
              />
            ))}
          </>
        )}
      </div>
    </div>
  );
}

function Stat({
  icon: Icon,
  label,
  value,
  tone = "text-fg",
  title,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  tone?: string;
  title?: string;
}) {
  return (
    <div className="min-w-0" title={title}>
      <div className="flex items-center gap-1 text-[10.5px] uppercase tracking-wide text-faint">
        <Icon size={11} /> {label}
      </div>
      <div className={`num truncate ${tone}`}>{value}</div>
    </div>
  );
}

function FileRow({
  name,
  size,
  done,
  wanted,
  head,
  state,
  onToggle,
}: {
  name: string;
  size: number;
  done: number;
  wanted: boolean;
  head?: boolean;
  /** The download's state, to tell a file in progress from one waiting. */
  state?: DownloadState;
  onToggle: () => void;
}) {
  const pct = size ? Math.floor((done / size) * 100) : 100;
  const complete = done >= size;
  const status = head
    ? null
    : !wanted
      ? "Skipped"
      : complete
        ? "Done"
        : state === "downloading"
          ? done > 0
            ? "Downloading"
            : "Queued"
          : state === "paused"
            ? "Paused"
            : done > 0
              ? "Partial"
              : "Waiting";
  return (
    <div
      className={`grid grid-cols-[22px_16px_minmax(0,1fr)_120px_80px_86px] items-center gap-2 px-3 py-1 text-[12px] ${head ? "chrome border-b border-line/60 text-faint" : wanted ? "" : "text-faint"}`}
    >
      <button
        type="button"
        role="checkbox"
        aria-checked={wanted}
        aria-label={head ? "Download all files" : `Download ${name}`}
        onClick={onToggle}
        className={`flex h-3.5 w-3.5 items-center justify-center rounded-[3px] border transition-colors duration-150 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-1 focus-visible:outline-accent ${
          wanted
            ? "border-accent bg-accent text-on-accent hover:bg-accent-hi"
            : "border-line hover:border-edge active:bg-line"
        }`}
      >
        {wanted && <Check size={10} strokeWidth={3} />}
      </button>
      {head ? <span /> : <FileIcon path={name} size={16} />}
      <span className="num truncate select-text" title={name}>
        {head ? name : name.split(/[\\/]/).pop()}
        {!head && /[\\/]/.test(name) ? (
          <span className="ml-2 text-faint">{name.replace(/[\\/][^\\/]*$/, "")}</span>
        ) : null}
      </span>
      <span className="flex items-center gap-2">
        <span className="h-[3px] flex-1 overflow-hidden rounded-full bg-line">
          <span
            className={`block h-full rounded-full transition-[width] duration-500 ${!wanted ? "bg-faint" : complete ? "bg-good" : "bg-accent"}`}
            style={{ width: `${pct}%` }}
          />
        </span>
        <span className="num w-9 text-right text-dim">{pct}%</span>
      </span>
      <span className="num text-right text-dim">{formatSize(size)}</span>
      <span
        className={`truncate text-right text-[11.5px] ${status === "Done" ? "text-good" : status === "Downloading" ? "text-accent" : "text-faint"}`}
      >
        {status}
      </span>
    </div>
  );
}

function Head({ icon: Icon, label }: { icon: LucideIcon; label: string }) {
  return (
    <span className="flex justify-end" title={label} aria-label={label}>
      <Icon size={12} />
    </span>
  );
}

function StateLabel({
  d,
  label,
  tone,
}: {
  d: Download;
  label: string;
  tone: string;
}) {
  const [Icon, , spin] = STATE_ICON[d.state];
  // Seeding only uploads when a peer asks for pieces; say so instead of a bare "Seeding".
  const idle = d.state === "seeding" && d.upRate === 0;
  if (idle) {
    label = "Seeding · idle";
    tone = "text-dim";
  }
  const hint = idle
    ? `Complete and shared. Nobody is requesting pieces right now (${d.peers} peer${d.peers === 1 ? "" : "s"} connected, ${d.swarmPeers} in the swarm), so upload is 0.`
    : label;
  return (
    <span
      className={`flex min-w-0 shrink-0 items-center gap-1.5 ${tone}`}
      title={d.error ?? hint}
    >
      <Icon size={12} className={`shrink-0 ${spin ? "animate-spin" : ""}`} />
      <span className="truncate">{d.error ? d.error : label}</span>
    </span>
  );
}

function RowButton(props: {
  label: string;
  danger?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={props.label}
      title={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
      className={`flex h-6 w-6 items-center justify-center rounded text-dim transition-colors duration-150 hover:text-fg focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent disabled:pointer-events-none disabled:opacity-40 ${
        props.danger
          ? "hover:bg-bad/15 hover:text-bad active:bg-bad/25"
          : "hover:bg-line active:bg-edge"
      }`}
    >
      {props.children}
    </button>
  );
}
