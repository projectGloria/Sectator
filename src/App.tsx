import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SearchX } from "lucide-react";
import { Titlebar } from "./components/Titlebar";
import { SearchBar } from "./components/SearchBar";
import { ResultsTable, sortResults, type Sort } from "./components/ResultsTable";
import { Inspector } from "./components/Inspector";
import { StatusBar } from "./components/StatusBar";
import { Sidebar } from "./components/Sidebar";
import { SettingsModal } from "./components/SettingsModal";
import { useSearch } from "./hooks/useSearch";
import { api } from "./lib/api";
import type { AppSettings, TorrentResult } from "./lib/types";

const SHORTCUTS: [string, string][] = [
  ["Ctrl K  /", "Focus search"],
  ["Enter", "Search · open magnet"],
  ["↑ ↓  J K", "Move selection"],
  ["Ctrl C", "Copy magnet"],
  ["I", "Toggle inspector"],
  ["Ctrl R  F5", "Search live (skip cache)"],
  ["Ctrl B", "History · presets · bookmarks"],
  ["Ctrl ,", "Settings"],
  ["Esc", "Stop · clear"],
];

export default function App() {
  const [version, setVersion] = useState<string>();
  const [text, setText] = useState("");
  const [sort, setSort] = useState<Sort>({ key: "score", desc: true });
  const [selected, setSelected] = useState<string | null>(null);
  const [inspector, setInspector] = useState(true);
  const [toast, setToast] = useState<string | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebar, setSidebar] = useState(false);
  const [bookmarks, setBookmarks] = useState<Set<string>>(new Set());
  const [libKey, setLibKey] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const { state, search, cancel } = useSearch();
  const busy = state.phase === "searching" || state.phase === "scraping";

  useEffect(() => {
    invoke<{ version: string }>("app_info").then((i) => setVersion(i.version));
    api.getSettings().then(setSettings, () => {});
    api.bookmarks().then((b) => setBookmarks(new Set(b.map((x) => x.infoHash))), () => {});
  }, []);

  // History is written as each search starts; refresh the sidebar once it settles.
  useEffect(() => {
    if (state.phase === "done" || state.phase === "cancelled") setLibKey((k) => k + 1);
  }, [state.phase]);

  const client = useMemo(() => settings?.clients.find((c) => c.id === settings.defaultClient) ?? null, [settings]);

  const rows = useMemo(() => sortResults([...state.results.values()], sort), [state.version, state.results, sort]);
  const current = (selected && state.results.get(selected)) || null;

  const notify = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast((t) => (t === msg ? null : t)), 1800);
  }, []);

  const copy = useCallback(
    (value: string, what: string) => {
      navigator.clipboard.writeText(value).then(
        () => notify(`${what} copied`),
        () => notify("Clipboard unavailable"),
      );
    },
    [notify],
  );

  const openMagnet = useCallback(
    (magnet: string) => {
      api.openMagnet(magnet).then(
        () => notify("Opened in the system torrent handler"),
        (e) => notify(`Couldn't open magnet: ${e}`),
      );
    },
    [notify],
  );
  const open = useCallback((r: TorrentResult) => openMagnet(r.magnet), [openMagnet]);

  const send = useCallback(
    (r: TorrentResult) => {
      if (!client) return;
      api.pushToClient(r.magnet, client.id).then(
        (o) => notify(o === "added" ? `Added to ${client.name}` : `${client.name} already has this torrent`),
        (e) => notify(`${client.name}: ${e}`),
      );
    },
    [client, notify],
  );

  const toggleBookmark = useCallback(
    (r: TorrentResult) => {
      const has = bookmarks.has(r.infoHash);
      (has ? api.removeBookmark(r.infoHash) : api.addBookmark(r)).then(() => {
        setBookmarks((prev) => {
          const next = new Set(prev);
          if (has) next.delete(r.infoHash);
          else next.add(r.infoHash);
          return next;
        });
        setLibKey((k) => k + 1);
        notify(has ? "Bookmark removed" : "Bookmarked");
      }, (e) => notify(String(e)));
    },
    [bookmarks, notify],
  );

  const submit = useCallback(
    (q: string, force = false) => {
      setSelected(null);
      search(q, force);
    },
    [search],
  );

  const run = useCallback(
    (q: string) => {
      setText(q);
      submit(q);
    },
    [submit],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (settingsOpen) return;
      const key = e.key.toLowerCase();
      if ((e.ctrlKey && key === "r") || e.key === "F5") {
        e.preventDefault(); // never reload the webview
        if (text.trim()) submit(text, true);
        return;
      }
      if (e.ctrlKey && key === "b") {
        e.preventDefault();
        setSidebar((v) => !v);
        return;
      }
      if (e.ctrlKey && e.key === ",") {
        e.preventDefault();
        setSettingsOpen(true);
        return;
      }
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement;
      if ((e.ctrlKey && e.key.toLowerCase() === "k") || (!typing && e.key === "/")) {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
        return;
      }
      if (typing) {
        if (e.key === "ArrowDown" && rows.length) {
          e.preventDefault();
          input.current?.blur();
          setSelected(rows[0].infoHash);
        }
        return;
      }
      const idx = selected ? rows.findIndex((r) => r.infoHash === selected) : -1;
      const move = (d: number) => {
        if (!rows.length) return;
        const n = Math.min(rows.length - 1, Math.max(0, idx + d));
        setSelected(rows[n].infoHash);
      };
      switch (e.key) {
        case "ArrowDown":
        case "j":
          e.preventDefault();
          move(1);
          break;
        case "ArrowUp":
        case "k":
          e.preventDefault();
          if (idx <= 0) input.current?.focus();
          else move(-1);
          break;
        case "PageDown":
          e.preventDefault();
          move(20);
          break;
        case "PageUp":
          e.preventDefault();
          move(-20);
          break;
        case "Enter":
          if (current) open(current);
          break;
        case "i":
          setInspector((v) => !v);
          break;
        case "Escape":
          if (busy) cancel();
          else setSelected(null);
          break;
        case "c":
          if (e.ctrlKey && current && !window.getSelection()?.toString()) {
            e.preventDefault();
            copy(current.magnet, "Magnet link");
          }
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [rows, selected, current, busy, cancel, open, copy, settingsOpen, text, submit]);

  const empty = (
    <div className="chrome flex h-full flex-col items-center justify-center gap-4 text-dim">
      {state.phase === "idle" ? (
        <>
          <p className="text-[13px]">Search curated indexers, DHT indexes and your Torznab feeds at once.</p>
          <div className="grid grid-cols-[auto_auto] gap-x-4 gap-y-1 text-[11.5px]">
            {SHORTCUTS.map(([k, v]) => (
              <div key={k} className="contents">
                <kbd className="num text-right text-faint">{k}</kbd>
                <span>{v}</span>
              </div>
            ))}
          </div>
        </>
      ) : busy ? (
        <p className="text-[12.5px] text-faint">Waiting for the first source to answer…</p>
      ) : (
        <>
          <SearchX size={22} className="text-faint" />
          <p className="text-[12.5px]">No results{state.statuses.some((s) => s.state === "error" || s.state === "timeout") ? " — some sources failed, see the status bar" : ""}.</p>
        </>
      )}
    </div>
  );

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <Titlebar version={version} onSidebar={() => setSidebar((v) => !v)} onSettings={() => setSettingsOpen(true)} />
      <SearchBar ref={input} value={text} busy={busy} onChange={setText} onSubmit={submit} onCancel={cancel} />
      <div className="relative flex min-h-0 flex-1">
        {sidebar && <Sidebar refreshKey={libKey} query={text} onRun={run} onOpenMagnet={openMagnet} onNotify={notify} onClose={() => setSidebar(false)} />}
        <ResultsTable rows={rows} selected={selected} sort={sort} onSort={setSort} onSelect={setSelected} onActivate={open} empty={empty} />
        {inspector && current && (
          <Inspector
            r={current}
            onClose={() => setInspector(false)}
            onOpen={open}
            onCopy={copy}
            bookmarked={bookmarks.has(current.infoHash)}
            onBookmark={toggleBookmark}
            client={client?.name ?? null}
            onSend={send}
          />
        )}
        {toast && (
          <div className="chrome pointer-events-none absolute bottom-3 left-1/2 -translate-x-1/2 rounded-md border border-line bg-raised px-3 py-1.5 text-[12px] shadow-lg shadow-black/40">
            {toast}
          </div>
        )}
      </div>
      <StatusBar
        phase={state.phase}
        statuses={state.statuses}
        total={state.results.size}
        shown={rows.length}
        scraping={state.scraping}
        elapsedMs={state.elapsedMs}
        error={state.error}
        origin={state.origin}
      />
      {settingsOpen && settings && <SettingsModal initial={settings} onSaved={setSettings} onClose={() => setSettingsOpen(false)} onNotify={notify} />}
    </div>
  );
}
