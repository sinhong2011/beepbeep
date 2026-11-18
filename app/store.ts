// SPDX-License-Identifier: AGPL-3.0-or-later
// The app's state. The download service (Rust, `beepbeep/native`) owns the downloads; this store
// mirrors its snapshot into signals at 4 Hz (plan §23) and keeps the UI's own state. Screens read
// the exported accessors and call the exported actions; the signals stay private.

import { Clipboard, LayoutAnimation, Notifications, batch, reduceMotion, signal } from "nsh";
import { AlertDialog } from "nsh/base";
import { Engine } from "beepbeep/native";

export type Status =
  | "queued" | "resolving" | "probing" | "downloading" | "retrying" | "merging" | "verifying"
  | "paused" | "completed" | "failed" | "cancelled";

export interface Task {
  id: string;
  status: Status;
  statusLabel: string;
  active: boolean;
  title: string;
  source: string;
  host: string;
  platform: "x" | "threads" | "instagram" | "video" | "file";
  platformName: string;
  isMedia: boolean;
  author: string | null;
  quality: string | null;
  duration: string | null;
  thumbnail: string | null;
  progress: number | null;
  sizeText: string;
  receivedText: string;
  speed: number;
  speedText: string;
  etaText: string;
  connections: number;
  error: string | null;
  finalPath: string | null;
  fileExists: boolean;
  folder: string;
  fileName: string | null;
  resumable: boolean;
  sha256: string | null;
  segments: [number, number][];
  log: string[];
  createdMs: number;
}

export interface Settings {
  folder: string;
  folderPath: string;
  maxConcurrent: number;
  connections: number;
  speedLimit: number;
  speedLimitText: string;
  notify: boolean;
  verifySha256: boolean;
  quality: string;
  cookieBrowser: string;
  categoryShortcuts: boolean;
  freeText: string;
}

export type View = "all" | "active" | "done" | "failed";
export type Source = "any" | "x" | "threads" | "instagram" | "file";
export type DialogName = "add" | "settings" | null;

export const VIEWS: { id: View; label: string; glyph: string }[] = [
  { id: "all", label: "All Downloads", glyph: "grid" },
  { id: "active", label: "Downloading", glyph: "download" },
  { id: "done", label: "Completed", glyph: "check" },
  { id: "failed", label: "Failed", glyph: "alert" },
];

export const SOURCES: { id: Exclude<Source, "any">; label: string; mark: string }[] = [
  { id: "x", label: "X", mark: "𝕏" },
  { id: "threads", label: "Threads", mark: "@" },
  { id: "instagram", label: "Instagram", mark: "◎" },
  { id: "file", label: "Files", mark: "" },
];

// ---------------------------------------------------------------------------------------------
// State

const version = signal(0);
const tasks = signal<Task[]>([]);
const settingsSignal = signal<Settings | null>(null);
const selected = signal<string | null>(null);
const view = signal<View>("all");
const source = signal<Source>("any");
const query = signal("");
const dialog = signal<DialogName>(null);
const inspectorHidden = signal(false);
const toastSignal = signal<{ text: string; ok: boolean; key: number } | null>(null);
const addDraft = signal("");

const inView = (t: Task, v: View) =>
  v === "all" ||
  (v === "active" && (t.active || t.status === "queued" || t.status === "paused")) ||
  (v === "done" && t.status === "completed") ||
  (v === "failed" && (t.status === "failed" || t.status === "cancelled"));

const fromSource = (t: Task, s: Source) =>
  s === "any" || (s === "file" ? !t.isMedia : t.platform === s);

function filtered(): Task[] {
  const q = query().trim().toLowerCase();
  const v = view();
  const s = source();
  return tasks().filter(
    (t) => inView(t, v) && fromSource(t, s) && (!q || t.title.toLowerCase().includes(q) || t.source.toLowerCase().includes(q)),
  );
}

/** Frames that change because of the user's action glide (off with reduced motion). */
function glide() {
  if (!reduceMotion()) LayoutAnimation.easeInEaseOut();
}

// ---------------------------------------------------------------------------------------------
// Sync with the service

function sync() {
  const json = Engine.snapshot(version());
  if (json) {
    const snap = JSON.parse(json) as { version: number; tasks: Task[]; settings: Settings };
    batch(() => {
      version.set(snap.version);
      tasks.set(snap.tasks);
      settingsSignal.set(snap.settings);
      const ids = filtered().map((t) => t.id);
      const sel = selected();
      if (!sel || !ids.includes(sel)) selected.set(ids[0] ?? null);
    });
  }
  const events = Engine.takeEvents();
  if (events && events !== "[]") {
    for (const e of JSON.parse(events) as { kind: string; id: string; title: string; detail: string | null }[]) {
      const ok = e.kind === "finished";
      showToast(ok ? `Downloaded “${e.title}”` : `Couldn't download “${e.title}”`, ok);
      if (settingsSignal()?.notify) {
        Notifications.show({ id: e.id, title: ok ? "Download finished" : "Download failed", body: ok ? e.title : `${e.title}: ${e.detail ?? ""}` }).catch(() => {});
      }
    }
  }
}

/** Starts mirroring the service; called once by the root. */
export function start() {
  sync();
  setInterval(sync, 250);
  Notifications.requestPermission().catch(() => {});
}

// ---------------------------------------------------------------------------------------------
// Reads (tracked where they are read)

export const allTasks = () => tasks();
export const visibleTasks = filtered;
export const hasTasks = () => tasks().length > 0;
export const viewCount = (v: View) => tasks().filter((t) => inView(t, v)).length;
export const sourceCount = (s: Source) => tasks().filter((t) => fromSource(t, s)).length;
export const selectedId = () => selected();
export const selectedTask = () => tasks().find((t) => t.id === selected()) ?? null;
export const taskById = (id: string) => tasks().find((t) => t.id === id) ?? null;
export const currentView = () => view();
export const currentSource = () => source();
export const currentQuery = () => query();
export const openDialog = () => dialog();
export const inspectorOpen = () => !inspectorHidden() && selected() !== null;
export const toast = () => toastSignal();
export const draft = () => addDraft();
export const settings = () => settingsSignal();
export const totalSpeed = () => tasks().reduce((sum, t) => sum + (t.active ? t.speed : 0), 0);
export const runningCount = () => tasks().filter((t) => t.active).length;
export const queuedCount = () => tasks().filter((t) => t.status === "queued").length;
export const anyRunning = () => tasks().some((t) => t.active || t.status === "queued");
export const anyPaused = () => tasks().some((t) => t.status === "paused");
export const tools = () => JSON.parse(Engine.tools()) as { ytdlp: string | null; ffmpeg: string | null };
export const viewLabel = () => VIEWS.find((v) => v.id === view())!.label;

// ---------------------------------------------------------------------------------------------
// Actions

export function select(id: string) {
  if (selected() === id) return;
  selected.set(id);
}

export function stepSelection(delta: number) {
  const ids = filtered().map((t) => t.id);
  if (!ids.length) return;
  const at = Math.max(0, ids.indexOf(selected() ?? ""));
  selected.set(ids[Math.min(ids.length - 1, Math.max(0, at + delta))]);
}

export function setView(v: View) {
  glide();
  batch(() => {
    view.set(v);
    selected.set(filtered()[0]?.id ?? null);
  });
}

export function setSource(s: Source) {
  glide();
  batch(() => {
    source.set(source() === s ? "any" : s);
    const ids = filtered().map((t) => t.id);
    if (!ids.includes(selected() ?? "")) selected.set(ids[0] ?? null);
  });
}

export function setQuery(q: string) {
  batch(() => {
    query.set(q);
    const ids = filtered().map((t) => t.id);
    if (!ids.includes(selected() ?? "")) selected.set(ids[0] ?? null);
  });
}

export function toggleInspector() {
  glide();
  inspectorHidden.set(!inspectorHidden());
}

export function showDialog(d: DialogName) {
  dialog.set(d);
}

export function showToast(text: string, ok = true) {
  const key = Date.now();
  toastSignal.set({ text, ok, key });
  setTimeout(() => {
    if (toastSignal()?.key === key) toastSignal.set(null);
  }, 3600);
}

/** Adds every link in `text`; returns how many. */
export function addLinks(text: string, quality = ""): number {
  glide();
  const ids = Engine.addLinks(text, quality);
  sync();
  if (ids.length) {
    batch(() => {
      view.set("all");
      source.set("any");
      query.set("");
      selected.set(ids[0]);
    });
  }
  return ids.length;
}

export const linksIn = (text: string) => Engine.extractLinks(text);
export const linkKind = (url: string) => Engine.linkKind(url);

/** Links on the clipboard go straight to the queue. */
export async function pasteFromClipboard() {
  const text = await Clipboard.readText().catch(() => "");
  const n = linksIn(text).length ? addLinks(text) : 0;
  showToast(n === 0 ? "The clipboard has no links." : n === 1 ? "Added 1 link from the clipboard." : `Added ${n} links from the clipboard.`, n > 0);
}

/** Opens the Add dialog, prefilled with the clipboard's links. */
export async function openAdd() {
  const text = await Clipboard.readText().catch(() => "");
  addDraft.set(linksIn(text).join("\n"));
  dialog.set("add");
}

const act = (f: () => void) => {
  f();
  sync();
};
export const pause = (id: string) => act(() => Engine.pause(id));
export const resume = (id: string) => act(() => Engine.resume(id));
export const stop = (id: string) => act(() => Engine.stop(id));
export const redownload = (id: string) => act(() => Engine.redownload(id));
export const pauseAll = () => act(() => Engine.pauseAll());
export const resumeAll = () => act(() => Engine.resumeAll());
export const clearFinished = () => act(() => Engine.clearFinished());

/** The main action for a download's state: pause, resume, retry, open. */
export function primary(t: Task) {
  switch (t.status) {
    case "paused":
    case "failed":
    case "cancelled":
      return resume(t.id);
    case "completed":
      return t.fileExists && t.finalPath ? Engine.open(t.finalPath).catch(() => {}) : redownload(t.id);
    default:
      return pause(t.id);
  }
}

export function primaryLabel(t: Task): { glyph: string; label: string } {
  switch (t.status) {
    case "paused":
      return { glyph: "play", label: "Resume" };
    case "failed":
    case "cancelled":
      return { glyph: "retry", label: "Retry" };
    case "completed":
      return t.fileExists ? { glyph: "play", label: "Open" } : { glyph: "retry", label: "Download Again" };
    case "queued":
      return { glyph: "pause", label: "Hold" };
    default:
      return { glyph: "pause", label: "Pause" };
  }
}

export function reveal(t: Task) {
  const path = t.fileExists && t.finalPath ? t.finalPath : `${t.folder.replace(/^~/, "")}`;
  Engine.reveal(t.fileExists && t.finalPath ? path : settingsSignal()?.folderPath ?? path).catch((e: Error) =>
    showToast(`Couldn't show it in Finder: ${e.message}`, false),
  );
}

export function copyLink(t: Task) {
  Clipboard.writeText(t.source).then(() => showToast("Link copied."));
}

/** Asks how to remove a download: from the list, or its file too. */
export async function confirmRemove(t: Task) {
  const actions = [
    { id: "cancel", label: "Cancel", role: "cancel" as const },
    { id: "list", label: "Remove from List" },
    ...(t.fileExists ? [{ id: "trash", label: "Move File to Trash", role: "destructive" as const }] : []),
  ];
  const { action } = await AlertDialog.show({
    title: `Remove “${t.title.slice(0, 60)}”?`,
    description: t.fileExists ? "Remove it from the list, or also move the downloaded file to the Trash." : "It leaves the list; any partial file is deleted.",
    actions,
  });
  if (action === "list" || action === "trash") {
    glide();
    const path = Engine.remove(t.id, action === "trash");
    if (path) await Engine.trash(path).catch(() => {});
    sync();
  }
}

export function setSetting(key: string, value: string | number | boolean) {
  Engine.setSetting(key, String(value));
  sync();
}

export async function chooseFolder() {
  const path = await Engine.pickFolder().catch(() => null);
  if (path) setSetting("folder", path);
}

export const flush = () => Engine.flush();
