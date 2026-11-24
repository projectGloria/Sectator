import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SearchX } from "lucide-react";
import { Titlebar } from "./components/Titlebar";
import { SearchBar } from "./components/SearchBar";
import { ResultsTable, sortResults, type Sort } from "./components/ResultsTable";
import { Inspector } from "./components/Inspector";
import { Downloads } from "./components/Downloads";
import { StatusBar } from "./components/StatusBar";
import { DownloadBar } from "./components/DownloadBar";
import { NavRail, type View } from "./components/NavRail";
import { Sidebar } from "./components/Sidebar";
import { SettingsModal } from "./components/SettingsModal";
import { DownloadModal } from "./components/DownloadModal";
import { useSearch } from "./hooks/useSearch";
import { api } from "./lib/api";
import { setOperator } from "./lib/dsl";
import { applyTheme } from "./lib/theme";
import { ZOOM_STEP, applyZoom, clampZoom, zoomLabel } from "./lib/zoom";
import { seedersOf, type AppSettings, type Download, type TorrentResult } from "./lib/types";

const SHORTCUTS: [string, string][] = [
  ["Ctrl K  /", "Focus search"],
  ["Enter  D", "Download with Sectator"],
  ["Ctrl J", "Downloads"],
  ["↑ ↓  J K", "Move selection"],
  ["Ctrl C", "Copy magnet"],
  ["I", "Toggle inspector"],
  ["Ctrl R  F5", "Search live (skip cache)"],
  ["Ctrl B", "History · presets · bookmarks"],
  ["Ctrl ,", "Settings"],
  ["Ctrl + − 0", "Zoom in · out · reset"],
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
  const [view, setView] = useState<View>("results");
  const [downloads, setDownloads] = useState<Download[]>([]);
  const [dlSelected, setDlSelected] = useState<string | null>(null);
  const [pendingDownload, setPendingDownload] = useState<TorrentResult | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const { state, search, cancel } = useSearch();
  const busy = state.phase === "searching" || state.phase === "scraping";

  useEffect(() => {
    invoke<{ version: string }>("app_info").then((i) => setVersion(i.version));
    api.getSettings().then(setSettings, () => {});
    api.bookmarks().then(
      (b) => setBookmarks(new Set(b.map((x) => x.infoHash))),
      () => {},
    );
  }, []);

  useEffect(() => {
    if (settings) {
      applyTheme(settings.theme);
      applyZoom(settings.uiZoom);
    }
  }, [settings]);

  // History is written as each search starts; refresh the sidebar once it settles.
  useEffect(() => {
    if (state.phase === "done" || state.phase === "cancelled") setLibKey((k) => k + 1);
  }, [state.phase]);

  // The engine starts lazily, so listing is free until something has been downloaded.
  const refreshDownloads = useCallback(() => {
    api.downloads().then(setDownloads, () => {});
  }, []);
  useEffect(() => {
    refreshDownloads();
    const t = window.setInterval(refreshDownloads, view === "downloads" ? 1000 : 3000);
    return () => window.clearInterval(t);
  }, [refreshDownloads, view]);
  const activeDownloads = downloads.filter((d) => d.state === "downloading" || d.state === "metadata" || d.state === "checking").length;

  const client = useMemo(() => settings?.clients.find((c) => c.id === settings.defaultClient) ?? null, [settings]);

  const hideDead = settings?.hideDead ?? false;
  const rows = useMemo(() => {
    const all = [...state.results.values()];
    const visible = hideDead ? all.filter((r) => !r.flags.includes("deadSwarm") && seedersOf(r) !== 0) : all;
    return sortResults(visible, sort);
  }, [state.version, state.results, sort, hideDead]);
  const current = (selected && state.results.get(selected)) || null;

  const notify = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast((t) => (t === msg ? null : t)), 1800);
  }, []);

  const toggleDead = useCallback(() => {
    if (!settings) return;
    const next = { ...settings, hideDead: !settings.hideDead };
    setSettings(next);
    api.saveSettings(next).catch(() => {});
    notify(next.hideDead ? "Hiding dead torrents" : "Showing dead torrents");
  }, [settings, notify]);

  // Ctrl +/-/0 and Ctrl+wheel zoom the whole UI; the new factor is saved after a short pause.
  const zoomSave = useRef<number | undefined>(undefined);
  const zoomBy = useCallback(
    (step: number | null) => {
      const cur = settings;
      if (!cur) return;
      const uiZoom = clampZoom(step == null ? 1 : cur.uiZoom + step);
      if (uiZoom === cur.uiZoom) return;
      const next = { ...cur, uiZoom };
      setSettings(next);
      notify(`Zoom ${zoomLabel(uiZoom)}`);
      window.clearTimeout(zoomSave.current);
      zoomSave.current = window.setTimeout(() => api.saveSettings(next).catch(() => {}), 500);
    },
    [settings, notify],
  );
  useEffect(() => {
    if (settingsOpen) return; // the modal edits its own copy of the settings
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey) return;
      const step = e.key === "=" || e.key === "+" ? ZOOM_STEP : e.key === "-" || e.key === "_" ? -ZOOM_STEP : e.key === "0" ? null : undefined;
      if (step === undefined) return;
      e.preventDefault();
      zoomBy(step);
    };
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey || !e.deltaY) return;
      e.preventDefault();
      zoomBy(e.deltaY < 0 ? ZOOM_STEP : -ZOOM_STEP);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("wheel", onWheel);
    };
  }, [settingsOpen, zoomBy]);

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

  const download = useCallback((r: TorrentResult) => {
    setPendingDownload(r);
  }, []);

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
      (has ? api.removeBookmark(r.infoHash) : api.addBookmark(r)).then(
        () => {
          setBookmarks((prev) => {
            const next = new Set(prev);
            if (has) next.delete(r.infoHash);
            else next.add(r.infoHash);
            return next;
          });
          setLibKey((k) => k + 1);
          notify(has ? "Bookmark removed" : "Bookmarked");
        },
        (e) => notify(String(e)),
      );
    },
    [bookmarks, notify],
  );

  const submit = useCallback(
    (q: string, force = false) => {
      setSelected(null);
      setView("results");
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
      if (settingsOpen || pendingDownload) return;
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
      if (e.ctrlKey && key === "j") {
        e.preventDefault();
        setView((v) => (v === "downloads" ? "results" : "downloads"));
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
      if (view === "downloads") {
        if (!typing && e.key === "Escape") setView("results");
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
          if (current) download(current);
          break;
        case "i":
          setInspector((v) => !v);
          break;
        case "d":
          if (current && !e.ctrlKey && !e.altKey) download(current);
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
  }, [rows, selected, current, busy, cancel, open, copy, settingsOpen, pendingDownload, text, submit, view, download]);

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
      <Titlebar version={version} />
      <div className="flex min-h-0 flex-1">
        <NavRail
          view={view}
          onView={(v) => {
            setView(v);
            if (v === "results") input.current?.focus();
          }}
          library={sidebar}
          onLibrary={() => setSidebar((v) => !v)}
          onSettings={() => setSettingsOpen(true)}
          active={activeDownloads}
        />
        <div className="flex min-w-0 flex-1 flex-col">
          <SearchBar ref={input} value={text} busy={busy} hideDead={hideDead} onToggleDead={toggleDead} onChange={setText} onSubmit={submit} onCancel={cancel} />
          <div className="relative flex min-h-0 flex-1">
            {sidebar && <Sidebar refreshKey={libKey} query={text} onRun={run} onOpenMagnet={openMagnet} onNotify={notify} onClose={() => setSidebar(false)} />}
            {view === "downloads" ? (
              <Downloads items={downloads} selected={dlSelected} onSelect={setDlSelected} onChanged={refreshDownloads} onNotify={notify} />
            ) : (
              <ResultsTable rows={rows} selected={selected} sort={sort} onSort={setSort} onSelect={setSelected} onActivate={download} empty={empty} />
            )}
            {view === "results" && inspector && current && (
              <Inspector
                r={current}
                onClose={() => setInspector(false)}
                onOpen={open}
                onDownload={download}
                onCopy={copy}
                bookmarked={bookmarks.has(current.infoHash)}
                onBookmark={toggleBookmark}
                client={client?.name ?? null}
                onSend={send}
                onBy={(name) => run(setOperator(text, "by", name))}
              />
            )}
            {toast && (
              <div className="chrome pointer-events-none absolute bottom-3 left-1/2 -translate-x-1/2 rounded-md border border-line bg-raised px-3 py-1.5 text-[12px] shadow-lg shadow-black/40">
                {toast}
              </div>
            )}
          </div>
        </div>
      </div>
      <DownloadBar
        items={downloads}
        open={view === "downloads"}
        onToggle={() => setView((v) => (v === "downloads" ? "results" : "downloads"))}
        onPick={(hash) => {
          setDlSelected(hash);
          setView("downloads");
        }}
      />
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
      {pendingDownload && (
        <DownloadModal
          result={pendingDownload}
          defaultDir={settings?.download.dir ?? null}
          onStarted={(_hash, updated) => {
            refreshDownloads();
            notify(updated ? "Download updated" : "Download started");
          }}
          onClose={() => setPendingDownload(null)}
          onNotify={notify}
        />
      )}
    </div>
  );
}
