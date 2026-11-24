import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import appIcon from "../assets/sectator.png";

const win = getCurrentWindow();

export function Titlebar({ version }: { version?: string }) {
  return (
    <div data-tauri-drag-region onDoubleClick={() => win.toggleMaximize()} className="chrome flex h-8 shrink-0 items-center border-b border-line bg-panel pl-3">
      <span data-tauri-drag-region className="flex items-center gap-2 text-[12px] text-dim">
        <img src={appIcon} alt="" width={16} height={16} draggable={false} data-tauri-drag-region className="h-4 w-4 rounded-[3px]" />
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
