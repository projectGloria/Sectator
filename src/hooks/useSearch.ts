import { useCallback, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type { IndexerStatus, SearchEvent, TorrentResult } from "../lib/types";

export type Phase = "idle" | "searching" | "scraping" | "done" | "cancelled" | "error";

export interface SearchState {
  phase: Phase;
  query: string;
  results: Map<string, TorrentResult>;
  /** Bumped whenever `results` mutates, so memoised views recompute. */
  version: number;
  statuses: IndexerStatus[];
  scraping: number;
  elapsedMs: number | null;
  error: string | null;
}

const empty = (): SearchState => ({
  phase: "idle",
  query: "",
  results: new Map(),
  version: 0,
  statuses: [],
  scraping: 0,
  elapsedMs: null,
  error: null,
});

/**
 * Drives `search` over a Tauri Channel. Events are coalesced per animation frame
 * so a burst of indexer batches costs one render, and events from a superseded
 * search are dropped by id.
 */
export function useSearch() {
  const [state, setState] = useState<SearchState>(empty);
  const active = useRef(0);
  const queue = useRef<SearchEvent[]>([]);
  const frame = useRef<number | null>(null);

  const flush = useCallback(() => {
    frame.current = null;
    const events = queue.current;
    queue.current = [];
    setState((prev) => {
      let next = prev;
      for (const ev of events) {
        if (ev.searchId !== active.current) continue;
        next = apply(next, ev);
      }
      return next;
    });
  }, []);

  const search = useCallback(
    (query: string) => {
      const text = query.trim();
      const token = ++active.current;
      queue.current = [];
      setState({ ...empty(), phase: text ? "searching" : "idle", query: text });
      if (!text) {
        invoke("cancel_search").catch(() => {});
        return;
      }
      const channel = new Channel<SearchEvent>();
      let backendId: number | null = null;
      channel.onmessage = (ev) => {
        // The backend's id isn't known until `started`; bind it to our token then.
        if (ev.event === "started" && backendId === null && active.current === token) backendId = ev.searchId;
        if (backendId !== ev.searchId || active.current !== token) return;
        queue.current.push({ ...ev, searchId: token });
        if (frame.current === null) frame.current = requestAnimationFrame(flush);
      };
      invoke<number>("search", { query: text, onEvent: channel }).catch((e) => {
        if (active.current === token) setState((s) => ({ ...s, phase: "error", error: String(e) }));
      });
    },
    [flush],
  );

  const cancel = useCallback(() => {
    invoke("cancel_search").catch(() => {});
  }, []);

  return { state, search, cancel };
}

function apply(s: SearchState, ev: SearchEvent): SearchState {
  switch (ev.event) {
    case "started":
      return { ...s, statuses: ev.indexers };
    case "status":
      return { ...s, statuses: s.statuses.map((x) => (x.indexerId === ev.status.indexerId ? ev.status : x)) };
    case "results": {
      for (const r of ev.results) s.results.set(r.infoHash, r);
      return { ...s, version: s.version + 1 };
    }
    case "removed": {
      for (const h of ev.hashes) s.results.delete(h);
      return { ...s, version: s.version + 1 };
    }
    case "scraping":
      return { ...s, phase: "scraping", scraping: ev.count };
    case "done":
      return { ...s, phase: ev.cancelled ? "cancelled" : "done", elapsedMs: ev.elapsedMs, scraping: 0 };
  }
}
