// App-style navigation rail: one icon per view, settings pinned to the bottom.
import { Download, Library, Search, Settings } from "lucide-react";

export type View = "results" | "downloads";

interface Props {
  view: View;
  onView: (v: View) => void;
  library: boolean;
  onLibrary: () => void;
  onSettings: () => void;
  /** Downloads currently transferring, shown as a badge. */
  active: number;
}

export function NavRail({ view, onView, library, onLibrary, onSettings, active }: Props) {
  return (
    <nav aria-label="Main" className="chrome flex w-12 shrink-0 flex-col items-center gap-1 border-r border-line bg-panel py-2">
      <RailButton label="Search (Ctrl+K)" pressed={view === "results"} onClick={() => onView("results")}>
        <Search size={17} />
      </RailButton>
      <RailButton label="Downloads (Ctrl+J)" pressed={view === "downloads"} onClick={() => onView("downloads")}>
        <span className="relative">
          <Download size={17} />
          {active > 0 && (
            <span className="num absolute -right-2.5 -top-2 min-w-[15px] rounded-full bg-accent px-[3px] text-center text-[9.5px] font-semibold leading-[15px] text-on-accent">
              {active > 99 ? "99+" : active}
            </span>
          )}
        </span>
      </RailButton>
      <RailButton label="History, presets and bookmarks (Ctrl+B)" pressed={library} onClick={onLibrary}>
        <Library size={17} />
      </RailButton>
      <div className="mt-auto" />
      <RailButton label="Settings (Ctrl+,)" onClick={onSettings}>
        <Settings size={17} />
      </RailButton>
    </nav>
  );
}

function RailButton(props: { label: string; pressed?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-label={props.label}
      title={props.label}
      aria-pressed={props.pressed}
      onClick={props.onClick}
      className={`relative flex h-9 w-9 items-center justify-center rounded-md transition-colors duration-150 focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent ${
        props.pressed ? "bg-accent/12 text-accent" : "text-dim hover:bg-raised hover:text-fg active:bg-line"
      }`}
    >
      {props.pressed && <span className="absolute -left-[6px] top-2 bottom-2 w-[2px] rounded-full bg-accent" />}
      {props.children}
    </button>
  );
}
