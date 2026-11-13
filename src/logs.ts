// Logs window: renders the ring log snapshot and subscribes to live entries.

import { invoke, listen } from "./tauri";

type LogEntry = { ts: number; level: "info" | "warn" | "error"; message: string };

const view = document.getElementById("log-view")!;

function fmtTs(ms: number): string {
  const d = new Date(ms);
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function renderEntry(e: LogEntry): string {
  const esc = (s: string) =>
    s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return `<div class="log-line ${e.level}"><span class="t">${fmtTs(e.ts)}</span><span class="m">[${e.level.toUpperCase()}] ${esc(e.message)}</span></div>`;
}

function append(e: LogEntry, scroll: boolean) {
  view.insertAdjacentHTML("beforeend", renderEntry(e));
  // Cap DOM size (ring buffer already caps at 500 server-side).
  while (view.childElementCount > 500) view.firstElementChild?.remove();
  if (scroll) view.scrollTop = view.scrollHeight;
}

// Subscribe BEFORE fetching the snapshot so entries emitted in between are
// buffered, not lost; dedupe by (ts, message) against the snapshot.
const key = (e: LogEntry) => `${e.ts}|${e.message}`;
const seen = new Set<string>();
const pending: LogEntry[] = [];
let live = false;

listen<any>("app://log", (ev) => {
  // Event payload carries { event: "log", entry: {...} }.
  const entry: LogEntry | undefined = ev.payload?.entry ?? ev.payload;
  if (!entry?.ts) return;
  if (live) {
    append(entry, true);
  } else {
    pending.push(entry);
  }
});

invoke<LogEntry[]>("get_logs").then((entries) => {
  entries.forEach((e) => {
    seen.add(key(e));
    append(e, false);
  });
  live = true;
  for (const e of pending) {
    if (!seen.has(key(e))) append(e, false);
  }
  pending.length = 0;
  view.scrollTop = view.scrollHeight;
});

document.getElementById("btn-clear")!.addEventListener("click", () => {
  invoke("clear_logs");
  view.textContent = "";
});
