import { useEffect, useRef, useState } from "react";
import { Check, Database, Download, Globe, ListTree, Minus, Palette, Plug, Plus, Settings as SettingsIcon, Trash2, Waypoints, X } from "lucide-react";
import { api } from "../lib/api";
import { formatAge } from "../lib/format";
import { THEMES, applyTheme } from "../lib/theme";
import { ZOOM_MAX, ZOOM_MIN, ZOOM_STEP, applyZoom, clampZoom, zoomLabel } from "../lib/zoom";
import type { AppSettings, ClientConfig, ClientKind, IndexerConfig, IndexerHealth, ProxyOverride } from "../lib/types";
import { Button, Field, NumberInput, Segmented, TextInput, Toggle } from "./ui";

type Tab = "indexers" | "network" | "downloads" | "clients" | "cache" | "appearance";

interface Props {
  initial: AppSettings;
  onSaved: (s: AppSettings) => void;
  onClose: () => void;
  onNotify: (msg: string) => void;
}

const uid = (p: string) => `${p}-${Math.random().toString(36).slice(2, 8)}`;
const orNull = (s: string) => (s.trim() ? s : null);

export function SettingsModal({ initial, onSaved, onClose, onNotify }: Props) {
  const [tab, setTab] = useState<Tab>("indexers");
  const [s, setS] = useState<AppSettings>(initial);
  const [saving, setSaving] = useState(false);
  const dirty = JSON.stringify(s) !== JSON.stringify(initial);
  const saved = useRef(false);

  // Preview the picked theme and zoom live; closing without saving puts the stored ones back.
  useEffect(() => applyTheme(s.theme), [s.theme]);
  useEffect(() => applyZoom(s.uiZoom), [s.uiZoom]);
  useEffect(
    () => () => {
      if (!saved.current) {
        applyTheme(initial.theme);
        applyZoom(initial.uiZoom);
      }
    },
    [initial.theme, initial.uiZoom],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const save = () => {
    setSaving(true);
    api.saveSettings(s).then(
      (stored) => {
        saved.current = true;
        setSaving(false);
        onSaved(stored);
        onNotify("Settings saved");
        onClose();
      },
      (e) => {
        setSaving(false);
        onNotify(`Couldn't save: ${e}`);
      },
    );
  };

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/50" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        role="dialog"
        aria-label="Settings"
        className="flex h-[min(620px,90%)] w-[min(820px,94%)] flex-col overflow-hidden rounded-lg border border-line bg-panel shadow-2xl shadow-black/60"
      >
        <header className="chrome flex h-9 shrink-0 items-center border-b border-line pl-3">
          <span className="flex items-center gap-1.5 text-[12.5px] font-medium">
            <SettingsIcon size={13} className="text-accent" /> Settings
          </span>
          <div className="ml-4">
            <Segmented<Tab>
              value={tab}
              onChange={setTab}
              options={[
                ["indexers", "Indexers", ListTree],
                ["network", "Network", Globe],
                ["downloads", "Downloads", Download],
                ["clients", "Clients", Waypoints],
                ["cache", "Cache", Database],
                ["appearance", "Appearance", Palette],
              ]}
            />
          </div>
          <button
            aria-label="Close settings"
            onClick={onClose}
            className="ml-auto flex h-full w-10 items-center justify-center text-faint transition-colors duration-150 hover:bg-raised hover:text-fg"
          >
            <X size={14} />
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto p-4" style={{ scrollbarGutter: "stable" }}>
          {tab === "indexers" && <IndexersTab s={s} setS={setS} />}
          {tab === "network" && <NetworkTab s={s} setS={setS} />}
          {tab === "downloads" && <DownloadsTab s={s} setS={setS} />}
          {tab === "clients" && <ClientsTab s={s} setS={setS} onNotify={onNotify} />}
          {tab === "cache" && <CacheTab s={s} setS={setS} onNotify={onNotify} />}
          {tab === "appearance" && <AppearanceTab s={s} setS={setS} />}
        </div>
        <footer className="chrome flex h-11 shrink-0 items-center justify-end gap-2 border-t border-line px-3">
          <span className="mr-auto text-[11px] text-faint">Stored locally in your app data folder. Nothing leaves this machine.</span>
          <Button tone="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button tone="primary" disabled={!dirty || saving} onClick={save}>
            {saving ? "Saving…" : "Save"}
          </Button>
        </footer>
      </div>
    </div>
  );
}

type TabProps = { s: AppSettings; setS: React.Dispatch<React.SetStateAction<AppSettings>> };

function IndexersTab({ s, setS }: TabProps) {
  const [health, setHealth] = useState<Record<string, IndexerHealth>>({});
  useEffect(() => {
    api.indexerHealth().then(
      (h) => setHealth(Object.fromEntries(h.map((x) => [x.indexerId, x]))),
      () => {},
    );
  }, []);
  const edit = (id: string, patch: Partial<IndexerConfig>) => setS((p) => ({ ...p, indexers: p.indexers.map((c) => (c.id === id ? { ...c, ...patch } : c)) }));
  const addTorznab = () =>
    setS((p) => ({
      ...p,
      indexers: [
        ...p.indexers,
        {
          id: uid("torznab"),
          name: "Torznab feed",
          kind: "torznab",
          enabled: true,
          baseUrls: ["http://127.0.0.1:9117/api/v2.0/indexers/all/results/torznab"],
          proxy: { mode: "direct" },
          timeoutMs: null,
          apiKey: "",
        },
      ],
    }));

  return (
    <div className="space-y-2">
      {s.indexers.map((c) => {
        const h = health[c.id];
        return (
          <div key={c.id} className="rounded-md border border-line p-2.5">
            <div className="flex items-center gap-2">
              <Toggle checked={c.enabled} onChange={(v) => edit(c.id, { enabled: v })} label={`Enable ${c.name}`} />
              <TextInput value={c.name} onChange={(e) => edit(c.id, { name: e.target.value })} className="w-48 font-medium" />
              <span className="num text-[11px] text-faint">{c.kind}</span>
              {h && (
                <span className="num ml-auto text-[10.5px] text-faint" title={h.lastError ?? undefined}>
                  {h.searches} runs · {h.searches ? Math.round((100 * (h.searches - h.failures)) / h.searches) : 0}% ok
                  {h.avgLatencyMs != null && ` · ${Math.round(h.avgLatencyMs)}ms`}
                  {h.lastOkAt && ` · ok ${formatAge(h.lastOkAt)} ago`}
                </span>
              )}
              {c.id.startsWith("torznab-") && (
                <Button
                  tone="ghost"
                  aria-label="Remove feed"
                  className={h ? "" : "ml-auto"}
                  onClick={() => setS((p) => ({ ...p, indexers: p.indexers.filter((x) => x.id !== c.id) }))}
                >
                  <Trash2 size={12} />
                </Button>
              )}
            </div>
            {c.enabled && (
              <div className="mt-2 pl-9">
                <Field label="Mirrors" hint="Tried in order; one URL per line">
                  <textarea
                    spellCheck={false}
                    value={c.baseUrls.join("\n")}
                    rows={Math.max(1, c.baseUrls.length)}
                    onChange={(e) =>
                      edit(c.id, {
                        baseUrls: e.target.value
                          .split("\n")
                          .map((u) => u.trim())
                          .filter(Boolean),
                      })
                    }
                    className="num min-w-0 flex-1 resize-none select-text rounded-md border border-line bg-bg px-2 py-1 text-[11.5px] outline-none transition-colors duration-150 hover:border-edge focus-visible:border-accent/60"
                    style={{ outline: "none" }}
                  />
                </Field>
                <Field label="Route" hint="Inherit uses the global proxy; Tor needs socks5h://127.0.0.1:9050">
                  <Segmented<ProxyOverride["mode"]>
                    value={c.proxy.mode}
                    onChange={(m) =>
                      edit(c.id, { proxy: m === "custom" ? { mode: "custom", url: c.proxy.mode === "custom" ? c.proxy.url : "socks5h://127.0.0.1:9050" } : { mode: m } })
                    }
                    options={[
                      ["inherit", "Global"],
                      ["direct", "Direct"],
                      ["custom", "Custom proxy"],
                    ]}
                  />
                  {c.proxy.mode === "custom" && (
                    <TextInput mono value={c.proxy.url} onChange={(e) => edit(c.id, { proxy: { mode: "custom", url: e.target.value } })} className="flex-1" />
                  )}
                </Field>
                <Field label="Timeout (ms)" hint="Empty uses the clearnet/Tor default">
                  <TextInput
                    mono
                    value={c.timeoutMs ?? ""}
                    placeholder="default"
                    onChange={(e) => edit(c.id, { timeoutMs: e.target.value ? Number(e.target.value.replace(/\D/g, "")) || null : null })}
                    className="w-24 text-right"
                  />
                </Field>
                {c.kind === "torznab" && (
                  <Field label="API key">
                    <TextInput mono type="password" value={c.apiKey ?? ""} onChange={(e) => edit(c.id, { apiKey: e.target.value })} className="flex-1" />
                  </Field>
                )}
              </div>
            )}
          </div>
        );
      })}
      <Button onClick={addTorznab}>
        <Plus size={12} /> Add Torznab feed (Jackett / Prowlarr)
      </Button>
    </div>
  );
}

function NetworkTab({ s, setS }: TabProps) {
  const net = (patch: Partial<AppSettings["net"]>) => setS((p) => ({ ...p, net: { ...p.net, ...patch } }));
  return (
    <div>
      <Field label="Global proxy" hint="http://, socks5:// or socks5h:// (remote DNS). Indexers set to Global use it.">
        <TextInput mono value={s.net.proxy ?? ""} placeholder="none — direct connection" onChange={(e) => net({ proxy: orNull(e.target.value) })} className="flex-1" />
      </Field>
      <Field label="DNS-over-HTTPS" hint="JSON DoH endpoint used to resolve indexer hosts">
        <TextInput mono value={s.net.dohUrl ?? ""} placeholder="system resolver" onChange={(e) => net({ dohUrl: orNull(e.target.value) })} className="flex-1" />
      </Field>
      <Field label="User agent">
        <TextInput mono value={s.net.userAgent} onChange={(e) => net({ userAgent: e.target.value })} className="flex-1" />
      </Field>
      <Field label="Clearnet timeout (ms)">
        <NumberInput value={s.net.clearnetTimeoutMs} min={1000} max={30000} onChange={(n) => net({ clearnetTimeoutMs: n })} />
      </Field>
      <Field label="Tor timeout (ms)">
        <NumberInput value={s.net.torTimeoutMs} min={2000} max={60000} onChange={(n) => net({ torTimeoutMs: n })} />
      </Field>
    </div>
  );
}

function DownloadsTab({ s, setS }: TabProps) {
  const dl = (patch: Partial<AppSettings["download"]>) => setS((p) => ({ ...p, download: { ...p.download, ...patch } }));
  const [ratio, setRatio] = useState(String(s.download.seedRatio));
  return (
    <div>
      <p className="chrome mb-3 text-[11.5px] text-faint">
        Sectator downloads with its built-in libtorrent engine. Torrents keep seeding until they reach the ratio below; resuming a finished one seeds it without a limit.
      </p>
      <Field label="Save folder" hint="Absolute path, e.g. C:\Users\you\Downloads\Torrents">
        <TextInput mono value={s.download.dir ?? ""} placeholder="your Downloads folder" onChange={(e) => dl({ dir: orNull(e.target.value) })} className="flex-1" />
      </Field>
      <Field label="Listen port" hint="Incoming peers (TCP and uTP); mapped with UPnP / NAT-PMP when the router allows. 0 picks a random port.">
        <NumberInput value={s.download.listenPort} max={65535} onChange={(n) => dl({ listenPort: n })} />
      </Field>
      <Field label="Download limit (KiB/s)" hint="0 = unlimited">
        <NumberInput value={s.download.maxDownloadKib} max={2000000} onChange={(n) => dl({ maxDownloadKib: n })} />
      </Field>
      <Field label="Upload limit (KiB/s)" hint="0 = unlimited">
        <NumberInput value={s.download.maxUploadKib} max={2000000} onChange={(n) => dl({ maxUploadKib: n })} />
      </Field>
      <Field label="Stop seeding at ratio" hint="Uploaded ÷ downloaded; 0 = seed forever">
        <TextInput
          mono
          inputMode="decimal"
          value={ratio}
          onChange={(e) => {
            const v = e.target.value.replace(/[^\d.]/g, "");
            setRatio(v);
            const n = Number(v);
            if (v && Number.isFinite(n)) dl({ seedRatio: Math.min(100, n) });
          }}
          onBlur={() => setRatio(String(s.download.seedRatio))}
          className="w-24 text-right"
        />
      </Field>
    </div>
  );
}

function ClientsTab({ s, setS, onNotify }: TabProps & { onNotify: (m: string) => void }) {
  const [testing, setTesting] = useState<string | null>(null);
  const edit = (id: string, patch: Partial<ClientConfig>) => setS((p) => ({ ...p, clients: p.clients.map((c) => (c.id === id ? { ...c, ...patch } : c)) }));
  const add = (kind: ClientKind) => {
    const c: ClientConfig = {
      id: uid(kind),
      kind,
      name: kind === "qbittorrent" ? "qBittorrent" : "Transmission",
      url: kind === "qbittorrent" ? "http://127.0.0.1:8080" : "http://127.0.0.1:9091",
      username: null,
      password: null,
      savePath: null,
      category: null,
      paused: false,
    };
    setS((p) => ({ ...p, clients: [...p.clients, c], defaultClient: p.defaultClient ?? c.id }));
  };
  const test = (c: ClientConfig) => {
    setTesting(c.id);
    api.testClient(c).then(
      (v) => {
        setTesting(null);
        onNotify(`Connected: ${v}`);
      },
      (e) => {
        setTesting(null);
        onNotify(String(e));
      },
    );
  };

  return (
    <div className="space-y-2">
      <p className="text-[11.5px] text-dim">
        Send magnets straight to a client's Web UI instead of the system magnet handler. Clients are always reached directly, never through the indexer proxy.
      </p>
      {s.clients.map((c) => (
        <div key={c.id} className="rounded-md border border-line p-2.5">
          <div className="flex items-center gap-2">
            <TextInput value={c.name} onChange={(e) => edit(c.id, { name: e.target.value })} className="w-48 font-medium" />
            <span className="num text-[11px] text-faint">{c.kind}</span>
            <label className="chrome ml-auto flex cursor-pointer items-center gap-1.5 text-[11.5px] text-dim">
              <input
                type="radio"
                name="default-client"
                checked={s.defaultClient === c.id}
                onChange={() => setS((p) => ({ ...p, defaultClient: c.id }))}
                className="accent-[var(--color-accent)]"
              />
              Default
            </label>
            <Button onClick={() => test(c)} disabled={testing === c.id}>
              <Plug size={12} /> {testing === c.id ? "Testing…" : "Test"}
            </Button>
            <Button
              tone="ghost"
              aria-label="Remove client"
              onClick={() => setS((p) => ({ ...p, clients: p.clients.filter((x) => x.id !== c.id), defaultClient: p.defaultClient === c.id ? null : p.defaultClient }))}
            >
              <Trash2 size={12} />
            </Button>
          </div>
          <div className="mt-2">
            <Field label="Web UI URL">
              <TextInput mono value={c.url} onChange={(e) => edit(c.id, { url: e.target.value })} className="flex-1" />
            </Field>
            <Field label="Username / password" hint="Leave empty if the client bypasses auth for localhost">
              <TextInput value={c.username ?? ""} placeholder="username" onChange={(e) => edit(c.id, { username: orNull(e.target.value) })} className="w-40" />
              <TextInput type="password" value={c.password ?? ""} placeholder="password" onChange={(e) => edit(c.id, { password: orNull(e.target.value) })} className="w-40" />
            </Field>
            <Field label="Save path">
              <TextInput mono value={c.savePath ?? ""} placeholder="client default" onChange={(e) => edit(c.id, { savePath: orNull(e.target.value) })} className="flex-1" />
            </Field>
            <Field label={c.kind === "qbittorrent" ? "Category" : "Label"}>
              <TextInput value={c.category ?? ""} placeholder="none" onChange={(e) => edit(c.id, { category: orNull(e.target.value) })} className="w-40" />
              <span className="ml-4 text-[11.5px] text-dim">Add paused</span>
              <Toggle checked={c.paused} onChange={(v) => edit(c.id, { paused: v })} label="Add paused" />
            </Field>
          </div>
        </div>
      ))}
      <div className="flex gap-2">
        <Button onClick={() => add("qbittorrent")}>
          <Plus size={12} /> qBittorrent
        </Button>
        <Button onClick={() => add("transmission")}>
          <Plus size={12} /> Transmission
        </Button>
      </div>
    </div>
  );
}

function CacheTab({ s, setS, onNotify }: TabProps & { onNotify: (m: string) => void }) {
  const [stats, setStats] = useState<{ torrents: number; searches: number } | null>(null);
  const refresh = () => api.cacheStats().then(setStats, () => {});
  useEffect(() => {
    refresh();
  }, []);
  return (
    <div>
      <Field label="Replay window (min)" hint="Repeat searches within this window are served from the local cache. 0 disables. Ctrl+R forces a live search.">
        <NumberInput value={Math.round(s.cacheTtlSecs / 60)} max={10080} onChange={(n) => setS((p) => ({ ...p, cacheTtlSecs: n * 60 }))} />
      </Field>
      <Field label="Live-scrape top N" hint="Results that get a live tracker scrape for fresh seeders. 0 disables.">
        <NumberInput value={s.scrapeTop} max={500} onChange={(n) => setS((p) => ({ ...p, scrapeTop: n }))} />
      </Field>
      <Field label="Local index">
        <span className="num text-[12px] text-dim">{stats ? `${stats.torrents} torrents · ${stats.searches} cached searches` : "…"}</span>
        <Button
          tone="danger"
          className="ml-auto"
          onClick={() =>
            api.clearCache().then(
              () => {
                refresh();
                onNotify("Cache cleared");
              },
              (e) => onNotify(String(e)),
            )
          }
        >
          <Trash2 size={12} /> Clear cache
        </Button>
      </Field>
      <p className="mt-2 text-[11.5px] text-faint">
        When every source fails, Sectator searches this local index instead so past results stay reachable offline. Bookmarks and history are kept when the cache is cleared.
      </p>
    </div>
  );
}

function AppearanceTab({ s, setS }: TabProps) {
  const zoom = (z: number) => setS((p) => ({ ...p, uiZoom: clampZoom(z) }));
  return (
    <div>
      <div className="mb-5">
        <div className="mb-2 text-[12px] text-dim">Zoom. Also Ctrl + / Ctrl − / Ctrl 0, or Ctrl + mouse wheel, anywhere in the app.</div>
        <div className="flex items-center gap-2">
          <Button aria-label="Zoom out" disabled={s.uiZoom <= ZOOM_MIN} onClick={() => zoom(s.uiZoom - ZOOM_STEP)}>
            <Minus size={13} />
          </Button>
          <input
            type="range"
            aria-label="Zoom"
            min={ZOOM_MIN * 100}
            max={ZOOM_MAX * 100}
            step={5}
            value={Math.round(s.uiZoom * 100)}
            onChange={(e) => zoom(Number(e.target.value) / 100)}
            className="zoom-range w-56"
          />
          <Button aria-label="Zoom in" disabled={s.uiZoom >= ZOOM_MAX} onClick={() => zoom(s.uiZoom + ZOOM_STEP)}>
            <Plus size={13} />
          </Button>
          <span className="num w-12 text-right text-[12px] text-fg">{zoomLabel(s.uiZoom)}</span>
          <Button tone="ghost" disabled={s.uiZoom === 1} onClick={() => zoom(1)}>
            Reset
          </Button>
        </div>
      </div>
      <div className="mb-3 text-[12px] text-dim">Theme. Changes preview right away and stick once you save.</div>
      <div role="radiogroup" aria-label="Theme" className="grid grid-cols-3 gap-3">
        {THEMES.map((t) => {
          const on = s.theme === t.id;
          const [bg, panel, line, fg, accent] = t.swatch;
          return (
            <button
              key={t.id}
              type="button"
              role="radio"
              aria-checked={on}
              onClick={() => setS((p) => ({ ...p, theme: t.id }))}
              className={`chrome group overflow-hidden rounded-lg border text-left transition-colors duration-150 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-2 focus-visible:outline-accent ${
                on ? "border-accent ring-1 ring-accent" : "border-line hover:border-edge active:bg-raised"
              }`}
            >
              {/* Miniature of the app shell in this theme's colours. */}
              <div className="flex h-24" style={{ background: bg }}>
                <div className="flex w-6 flex-col items-center gap-1.5 pt-2" style={{ background: panel, borderRight: `1px solid ${line}` }}>
                  <span className="h-2.5 w-2.5 rounded-sm" style={{ background: accent }} />
                  <span className="h-2.5 w-2.5 rounded-sm" style={{ background: line }} />
                  <span className="h-2.5 w-2.5 rounded-sm" style={{ background: line }} />
                </div>
                <div className="flex flex-1 flex-col gap-1.5 p-2">
                  <span className="h-3 rounded-sm" style={{ border: `1px solid ${line}` }} />
                  {[0.8, 0.6, 0.7].map((w, i) => (
                    <span key={i} className="flex items-center gap-1.5">
                      <span className="h-1.5 rounded-full" style={{ width: `${w * 100}%`, background: fg, opacity: i ? 0.35 : 0.7 }} />
                    </span>
                  ))}
                  <span className="mt-auto h-1 rounded-full" style={{ background: line }}>
                    <span className="block h-full w-3/5 rounded-full" style={{ background: accent }} />
                  </span>
                </div>
              </div>
              <div className="flex items-center gap-2 border-t border-line bg-panel px-2.5 py-2">
                <span className="min-w-0 flex-1">
                  <span className="block text-[12.5px] font-medium text-fg">{t.name}</span>
                  <span className="block truncate text-[11px] text-faint">{t.hint}</span>
                </span>
                {on && <Check size={14} className="shrink-0 text-accent" />}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
