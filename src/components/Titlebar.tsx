import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";

const win = getCurrentWindow();

export function Titlebar({ version }: { version?: string }) {
  return (
    <div data-tauri-drag-region onDoubleClick={() => win.toggleMaximize()} className="chrome flex h-8 shrink-0 items-center border-b border-line bg-panel pl-3">
      <span data-tauri-drag-region className="flex items-center gap-2 text-[12px] text-dim">
        <svg width="12" height="12" viewBox="0 0 512 512" aria-hidden className="text-accent">
          <circle cx="226" cy="226" r="118" fill="none" stroke="currentColor" strokeWidth="56" />
          <path d="M312 312 L410 410" stroke="currentColor" strokeWidth="64" strokeLinecap="round" />
        </svg>
        <span data-tauri-drag-region className="font-medium text-fg">
          Sectator
        </span>
        {version && (
          <span data-tauri-drag-region className="num text-faint">
            {version}
          </span>
        )}
      </span>
      <div className="ml-auto flex h-full">
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

function TitleButton(props: { label: string; danger?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      aria-label={props.label}
      title={props.label}
      onClick={props.onClick}
      onDoubleClick={(e) => e.stopPropagation()}
      className={`flex h-full w-11 items-center justify-center transition-colors duration-150 text-dim ${
        props.danger ? "hover:bg-[#c42b1c] hover:text-white active:bg-[#a52314]" : "hover:bg-raised hover:text-fg active:bg-line"
      }`}
    >
      {props.children}
    </button>
  );
}
