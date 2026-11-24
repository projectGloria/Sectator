import { getCurrentWindow } from "@tauri-apps/api/window";
import { Download, Minus, PanelLeft, Settings, Square, X } from "lucide-react";

const win = getCurrentWindow();

export function Titlebar({
  version,
  onSidebar,
  onSettings,
  downloads,
  active,
  onDownloads,
}: {
  version?: string;
  onSidebar: () => void;
  onSettings: () => void;
  /** Whether the downloads view is showing. */
  downloads: boolean;
  /** Downloads currently transferring, shown as a badge. */
  active: number;
  onDownloads: () => void;
}) {
  return (
    <div
      data-tauri-drag-region
      onDoubleClick={() => win.toggleMaximize()}
      className="chrome flex h-8 shrink-0 items-center border-b border-line bg-panel pl-3"
    >
      <span data-tauri-drag-region className="flex items-center gap-2 text-[12px] text-dim">
        <svg width="12" height="12" viewBox="0 0 512 512" aria-hidden>
          <circle cx="226" cy="226" r="118" fill="none" stroke="#22d3ee" strokeWidth="56" />
          <path d="M312 312 L410 410" stroke="#22d3ee" strokeWidth="64" strokeLinecap="round" />
        </svg>
        <span data-tauri-drag-region className="font-medium text-fg">Sectator</span>
        {version && <span data-tauri-drag-region className="num text-faint">{version}</span>}
      </span>
      <div className="ml-auto flex h-full">
        <TitleButton label="History, presets and bookmarks (Ctrl+B)" onClick={onSidebar}>
          <PanelLeft size={13} />
        </TitleButton>
        <TitleButton label={downloads ? "Back to results (Ctrl+J)" : "Downloads (Ctrl+J)"} pressed={downloads} onClick={onDownloads}>
          <span className="relative">
            <Download size={13} />
            {active > 0 && (
              <span className="num absolute -right-2 -top-1.5 min-w-[13px] rounded-full bg-accent px-[3px] text-center text-[9px] font-semibold leading-[13px] text-bg">{active > 99 ? "99+" : active}</span>
            )}
          </span>
        </TitleButton>
        <TitleButton label="Settings (Ctrl+,)" onClick={onSettings}>
          <Settings size={13} />
        </TitleButton>
        <span className="mx-1 my-2 w-px bg-line" />
        <TitleButton label="Minimize" onClick={() => win.minimize()}>
          <Minus size={13} />
        </TitleButton>
        <TitleButton label="Maximize" onClick={() => win.toggleMaximize()}>
          <Square size={11} />
        </TitleButton>
        <TitleButton label="Close" danger onClick={() => win.close()}>
          <X size={14} />
        </TitleButton>
      </div>
    </div>
  );
}

function TitleButton(props: { label: string; danger?: boolean; pressed?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      aria-label={props.label}
      title={props.label}
      aria-pressed={props.pressed}
      onClick={props.onClick}
      onDoubleClick={(e) => e.stopPropagation()}
      className={`flex h-full w-11 items-center justify-center transition-colors duration-150 hover:text-fg ${props.pressed ? "text-accent" : "text-dim"} ${
        props.danger ? "hover:bg-[#c42b1c] active:bg-[#a52314]" : "hover:bg-raised active:bg-line"
      }`}
    >
      {props.children}
    </button>
  );
}
