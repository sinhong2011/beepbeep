// SPDX-License-Identifier: AGPL-3.0-or-later
// The downloads workspace (plan §8–11): a sidebar of views and sources, a paste-and-go link bar, a
// dense table of downloads, an inspector for the selection and a status bar with live throughput.

import { Image, List, Scroll, Text, View, createMotion, signal } from "nsh";
import appIcon from "../assets/icon/icon.png";
import { Button, Input, Tab, Tabs } from "nsh/base";
import { titleBar } from "nsh/desktop";
import * as store from "../store";
import type { Task } from "../store";
import {
  Enter, Glyph, IconButton, PrimaryButton, ProgressBar, RoundButton, SectionLabel, StatusIcon, Thumb, ToolButton,
  motion, press, progressTone, riseIn, slideInRight, wave,
} from "../components/ui";

export function Workspace() {
  return (
    <View class="flex-1 flex-row bg-background">
      <Sidebar />
      <View class="flex-1">
        <Toolbar />
        <LinkBar />
        <View class="flex-1 flex-row">
          <Table />
          <If when={store.inspectorOpen()}>
            <Inspector />
          </If>
        </View>
        <StatusBar />
      </View>
    </View>
  );
}

// ---------------------------------------------------------------------------------------------
// Sidebar

function Sidebar() {
  return (
    <View
      class="w-[232px] gap-[18px] border-r border-line bg-sidebar px-3 pb-3.5"
      style={{ paddingTop: Math.max(titleBar().height, 12) + 6 }}
      desktop={{ windowDrag: true }}
    >
      <View class="flex-row items-center gap-2.5 px-1.5">
        <Image src={appIcon} class="size-[34px]" style={{ objectFit: "contain" }} />
        <View>
          <Text class="text-base font-bold text-foreground">BeepBeep</Text>
          <Text class="text-[11.5px] text-ink-3">Download manager</Text>
        </View>
      </View>

      <View desktop={{ windowDrag: false }}>
        <PrimaryButton label="New Download" glyph="plus" onPress={store.openAdd} />
      </View>

      <View class="gap-2" desktop={{ windowDrag: false }}>
        <SectionLabel>Library</SectionLabel>
        {/* One selection springs between the views (Tabs' indicator). */}
        <Tabs accessibilityLabel="Views" class="gap-0.5" indicatorClass="inset-x-0 rounded-[9px] bg-selection">
          {store.VIEWS.map((v) => (
            <Tab
              selected={store.currentView() === v.id}
              onPress={() => store.setView(v.id)}
              accessibilityLabel={v.label}
              class={`h-8 flex-row items-center gap-2.5 rounded-[9px] px-2.5 hover:bg-hover ${press}`}
            >
              <Glyph name={v.glyph} size={16} iconClass={store.currentView() === v.id ? "text-primary" : "text-muted-foreground"} />
              <Text class={`flex-1 text-sm font-medium ${store.currentView() === v.id ? "text-foreground" : "text-muted-foreground"}`}>{v.label}</Text>
              <Text class="text-[11.5px] font-semibold tabular-nums text-ink-3">{store.viewCount(v.id) || ""}</Text>
            </Tab>
          ))}
        </Tabs>
      </View>

      <View class="gap-2" desktop={{ windowDrag: false }}>
        <SectionLabel>Sources</SectionLabel>
        <View class="gap-0.5">
          {store.SOURCES.map((s) => (
            <Button
              accessibilityLabel={`Only ${s.label}`}
              selected={store.currentSource() === s.id}
              onPress={() => store.setSource(s.id)}
              class={`h-[30px] flex-row items-center gap-2.5 rounded-[9px] px-2.5 transition-colors duration-200 hover:bg-hover selected:bg-selection ${press}`}
            >
              <View class="size-[18px] items-center justify-center rounded-[5px] bg-track">
                <If when={s.mark} else={<Glyph name="file" size={11} iconClass="text-file-foreground" />}>
                  <Text class="text-[11px] font-bold text-foreground">{s.mark}</Text>
                </If>
              </View>
              <Text class="flex-1 text-sm font-medium text-muted-foreground">{s.label}</Text>
              <Text class="text-[11.5px] font-semibold tabular-nums text-ink-3">{store.sourceCount(s.id) || ""}</Text>
            </Button>
          ))}
        </View>
      </View>

      <View class="flex-1" />
      <Throughput />
    </View>
  );
}

/** Live throughput: what every download adds up to. */
function Throughput() {
  return (
    <View class="gap-1.5 rounded-xl border border-line bg-surface-raised p-3">
      <View class="flex-row items-center gap-1.5">
        <View class={`size-[7px] rounded-full transition-colors duration-300 ${store.runningCount() ? "bg-success" : "bg-ink-3"}`} />
        <Text class="text-[11.5px] text-muted-foreground">
          {store.runningCount() === 0 ? "Idle" : store.runningCount() === 1 ? "1 download running" : `${store.runningCount()} downloads running`}
        </Text>
      </View>
      <Text class="text-[22px] font-bold tabular-nums text-foreground">{speed(store.totalSpeed())}</Text>
      <Text class="text-[11.5px] text-ink-3">
        {store.settings()?.speedLimit ? `Limited to ${store.settings()?.speedLimitText}` : "No speed limit"}
      </Text>
    </View>
  );
}

function speed(bps: number): string {
  if (bps < 1) return "0 B/s";
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let v = bps;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

// ---------------------------------------------------------------------------------------------
// Toolbar and link bar

function Toolbar() {
  return (
    <View class="h-14 flex-row items-center gap-2.5 pl-5" style={{ paddingRight: titleBar().right + 20 }} desktop={{ windowDrag: true }}>
      <View class="flex-1 flex-row items-baseline gap-2.5">
        <Text class="text-xl font-bold text-foreground">{store.viewLabel()}</Text>
        <Text class="text-sm text-ink-3">
          {store.currentSource() === "any" ? "" : `from ${store.SOURCES.find((s) => s.id === store.currentSource())?.label}`}
        </Text>
      </View>
      <View class="h-8 w-60 flex-row items-center gap-1.5 rounded-[9px] border border-line bg-surface-raised px-2.5" desktop={{ windowDrag: false }}>
        <Glyph name="search" size={14} iconClass="text-ink-3" />
        <Input
          value={store.currentQuery()}
          onChange={({ value }) => store.setQuery(value)}
          placeholder="Search downloads"
          class="flex-1 text-[12.5px] text-foreground"
        />
      </View>
      <View class="flex-row gap-2" desktop={{ windowDrag: false }}>
        <If when={store.anyRunning()}>
          <Enter spec={riseIn(0, motion.fast)}>
            <ToolButton glyph="pause" label="Pause All" onPress={store.pauseAll} />
          </Enter>
        </If>
        <If when={!store.anyRunning() && store.anyPaused()}>
          <Enter spec={riseIn(0, motion.fast)}>
            <ToolButton glyph="play" label="Resume All" onPress={store.resumeAll} />
          </Enter>
        </If>
        <IconButton glyph="details" label="Show or hide details" onPress={store.toggleInspector} />
        <IconButton glyph="settings" label="Settings" onPress={() => store.showDialog("settings")} />
      </View>
    </View>
  );
}

/** Paste a link, press Return: the fastest way in. */
function LinkBar() {
  const draft = signal("");
  const links = () => store.linksIn(draft());
  const kind = () => {
    const l = links();
    if (l.length === 0) return "";
    if (l.length > 1) return `${l.length} links`;
    const k = store.linkKind(l[0]);
    return k === "file" ? "File" : `${k === "x" ? "X" : k[0].toUpperCase() + k.slice(1)} video`;
  };
  const submit = () => {
    const n = store.addLinks(draft());
    if (n > 0) {
      draft.set("");
      store.showToast(n === 1 ? "Download added." : `${n} downloads added.`);
    }
  };
  return (
    <View
      class={`mx-5 mb-3 h-12 flex-row items-center gap-2.5 rounded-xl border bg-surface pr-1.5 pl-3.5 shadow-sm transition-colors duration-200 ${links().length ? "border-primary" : "border-line"}`}
    >
      <Glyph name="link" size={17} iconClass="text-ink-3" />
      <Input
        value={draft()}
        onChange={({ value }) => draft.set(value)}
        onSubmit={submit}
        placeholder="Paste a link to a file, or to a post on X, Threads or Instagram"
        class="flex-1 text-sm text-foreground"
      />
      <Text class={`text-[11.5px] font-semibold text-muted-foreground transition-opacity duration-150 ${links().length ? "opacity-100" : "opacity-0"}`}>
        {kind()}
      </Text>
      <IconButton glyph="paste" label="Paste from the clipboard" onPress={store.pasteFromClipboard} />
      <PrimaryButton label="Download" onPress={submit} disabled={links().length === 0} />
    </View>
  );
}

// ---------------------------------------------------------------------------------------------
// Table

const COLS = { progress: 140, size: 92, speed: 80, eta: 56 };

function Table() {
  return (
    <View class={`mb-3 ml-5 flex-1 overflow-hidden rounded-[14px] border border-line bg-surface ${store.inspectorOpen() ? "mr-3" : "mr-5"}`}>
      <View class="h-[34px] flex-row items-center gap-3.5 border-b border-line pr-14 pl-[62px]">
        <Header label="Name" />
        <Header label="Progress" width={COLS.progress} />
        <Header label="Size" width={COLS.size} />
        {/* Speed and time left move to the inspector while it is open, so names keep their room. */}
        <If when={!store.inspectorOpen()}>
          <Header label="Speed" width={COLS.speed} />
          <Header label="Left" width={COLS.eta} />
        </If>
      </View>
      <If when={store.hasTasks()} else={<EmptyState />}>
        <List
          each={store.visibleTasks()}
          keyExtractor={(t) => t.id}
          class="flex-1"
          empty={<NoMatches />}
        >
          {(task, index) => <Row task={task} index={index()} />}
        </List>
      </If>
    </View>
  );
}

function Header(props: { label: string; width?: number }) {
  return (
    <View class={props.width ? "" : "min-w-[180px] flex-1"} style={props.width ? { width: props.width } : undefined}>
      <Text class="text-[10.5px] font-semibold tracking-[0.5px] text-ink-3 uppercase">{props.label}</Text>
    </View>
  );
}

/** A row: bindings read the task the cell shows now; the cell rises in once, as part of a wave. */
function Row(props: { task: () => Task; index: number }) {
  const t = props.task;
  const enter = createMotion({ enter: wave(props.index) });
  const selected = () => store.selectedId() === t().id;
  return (
    <Button
      accessibilityLabel={t().title}
      selected={selected()}
      onPress={() => store.select(t().id)}
      class="h-[58px] flex-row items-center gap-3.5 border-b border-line px-3 transition-colors duration-200 hover:bg-hover selected:bg-selection"
      style={enter.style}
    >
      <Thumb task={t} size={38} radius={9} />
      <View class="min-w-[180px] flex-1 gap-[3px]">
        <Text class="line-clamp-1 text-sm font-semibold text-foreground">{t().title}</Text>
        <View class="flex-row items-center gap-1.5">
          <StatusIcon status={t().status} size={12} />
          <Text class={`text-[11.5px] ${t().status === "failed" ? "text-destructive" : "text-muted-foreground"}`}>
            {rowDetail(t())}
          </Text>
        </View>
      </View>
      <View class="gap-1" style={{ width: COLS.progress }}>
        <If when={t().progress !== null || t().active}>
          <ProgressBar value={t().progress} tone={progressTone(t())} />
        </If>
        <Text class="text-[11.5px] tabular-nums text-ink-3">{t().progress === null ? "" : `${(t().progress! * 100).toFixed(t().status === "completed" ? 0 : 1)}%`}</Text>
      </View>
      <Cell width={COLS.size} text={sizeCell(t())} />
      <If when={!store.inspectorOpen()}>
        <Cell width={COLS.speed} text={t().speedText} />
        <Cell width={COLS.eta} text={t().etaText} />
      </If>
      <RoundButton glyph={rowGlyph(t())} label={rowLabel(t())} onPress={() => rowAction(t())} />
    </Button>
  );
}

function Cell(props: { width: number; text: string }) {
  return (
    <View style={{ width: props.width }}>
      <Text class="line-clamp-1 text-[12.5px] tabular-nums text-muted-foreground">{props.text}</Text>
    </View>
  );
}

function rowDetail(t: Task): string {
  const source = t.isMedia ? [t.platformName, t.author, t.quality].filter(Boolean).join(" · ") : t.host;
  if (t.status === "failed") return t.error ?? "Failed";
  if (t.status === "downloading" || t.status === "completed") return source;
  return `${t.statusLabel} · ${source}`;
}

function sizeCell(t: Task): string {
  if (t.status === "completed" || t.progress === null) return t.sizeText;
  return `${t.receivedText.split(" ")[0]} / ${t.sizeText}`;
}

const rowGlyph = (t: Task) =>
  t.status === "completed" ? "folder" : t.status === "paused" ? "play" : t.status === "failed" || t.status === "cancelled" ? "retry" : "pause";
const rowLabel = (t: Task) =>
  t.status === "completed" ? "Show in Finder" : t.status === "paused" ? "Resume" : t.status === "failed" || t.status === "cancelled" ? "Retry" : "Pause";
function rowAction(t: Task) {
  if (t.status === "completed") store.reveal(t);
  else if (t.status === "paused" || t.status === "failed" || t.status === "cancelled") store.resume(t.id);
  else store.pause(t.id);
}

// ---------------------------------------------------------------------------------------------
// Inspector

function Inspector() {
  const slide = createMotion({ enter: slideInRight });
  return (
    <View class="mr-5 mb-3 w-[340px] overflow-hidden rounded-[14px] border border-line bg-surface" style={slide.style}>
      {/* `If` keyed by id: the body is rebuilt, and rises in, only when the selection changes. */}
      <If when={store.selectedId()}>{(id) => <InspectorBody id={id()} />}</If>
    </View>
  );
}

function InspectorBody(props: { id: string }) {
  const t = () => store.taskById(props.id);
  const enter = createMotion({ enter: riseIn(6) });
  return (
    <Scroll class="flex-1">
      <If when={t()}>
        {(task) => (
          <View class="gap-3.5 p-4" style={enter.style}>
            <View class="w-full overflow-hidden rounded-xl" style={{ aspectRatio: 16 / 10 }}>
              <InspectorPreview task={task} />
            </View>
            <View class="gap-1.5">
              <Text class="line-clamp-3 select-text text-[17px] font-bold text-foreground">{task().title}</Text>
              <View class="flex-row items-center gap-1.5">
                <StatusIcon status={task().status} size={14} />
                <Text class="text-[12.5px] tabular-nums text-muted-foreground">{facts(task())}</Text>
              </View>
              <If when={task().active && task().progress !== null}>
                <View class="pt-1">
                  <ProgressBar value={task().progress} tone={progressTone(task())} height={6} />
                </View>
              </If>
              <Text class={`text-[12.5px] ${task().status === "failed" ? "text-destructive" : "text-ink-3"}`}>{note(task())}</Text>
            </View>
            <View class="flex-row gap-2">
              <Button
                accessibilityLabel={store.primaryLabel(task()).label}
                onPress={() => store.primary(task())}
                class={`h-9 flex-1 flex-row items-center justify-center gap-1.5 rounded-[10px] bg-pill ${press}`}
              >
                <Glyph name={store.primaryLabel(task()).glyph} size={14} iconClass="text-pill-foreground" />
                <Text class="text-[12.5px] font-semibold text-pill-foreground">{store.primaryLabel(task()).label}</Text>
              </Button>
              <IconButton glyph="folder" label="Show in Finder" size={36} onPress={() => store.reveal(task())} />
              <IconButton glyph="link" label="Copy link" size={36} onPress={() => store.copyLink(task())} />
              <IconButton glyph="stop" label="Stop" size={36} onPress={() => store.stop(task().id)} />
              <IconButton glyph="trash" label="Remove" size={36} onPress={() => store.confirmRemove(task())} />
            </View>
            <Divider />
            <Facts task={task} />
            <Divider />
            <SectionLabel class="px-0">Connections</SectionLabel>
            <Segments task={task} />
            <Divider />
            <SectionLabel class="px-0">Activity</SectionLabel>
            <View class="gap-1.5">
              <Each of={task().log}>{(line) => <Text class="select-text font-mono text-[11px] text-muted-foreground">{line()}</Text>}</Each>
            </View>
          </View>
        )}
      </If>
    </Scroll>
  );
}

function InspectorPreview(props: { task: () => Task }) {
  return (
    <View class="size-full">
      <Thumb task={props.task} size={308} radius={12} glyph={56} />
    </View>
  );
}

const Divider = () => <View class="h-px bg-line" />;

function Facts(props: { task: () => Task }) {
  const rows = () => {
    const t = props.task();
    const r: [string, string][] = [["Source", t.source]];
    if (t.isMedia) {
      r.push(["Platform", t.platformName]);
      if (t.author) r.push(["Author", t.author]);
      if (t.quality) r.push(["Quality", t.quality]);
      if (t.duration) r.push(["Duration", t.duration]);
    }
    r.push(["Size", t.sizeText], ["Resumable", t.resumable ? "Yes, by range" : "No"], ["Folder", t.folder]);
    if (t.fileName) r.push(["File", t.fileName]);
    if (t.sha256) r.push(["SHA-256", t.sha256]);
    return r;
  };
  return (
    <View class="gap-2">
      <Each of={rows()}>
        {(row) => (
          <View class="flex-row gap-2.5">
            <Text class="w-[78px] text-[11.5px] text-ink-3">{row()[0]}</Text>
            <Text class="line-clamp-3 select-text flex-1 text-[11.5px] text-foreground">{row()[1]}</Text>
          </View>
        )}
      </Each>
    </View>
  );
}

/** One bar per segment, filled by what it has received: the engine's connections at work. */
function Segments(props: { task: () => Task }) {
  return (
    <If when={props.task().segments.length > 0} else={<Text class="text-[11.5px] text-ink-3">Shown while a file downloads in parts.</Text>}>
      <View class="gap-1.5">
        <View class="h-3 flex-row gap-0.5">
          <Each of={props.task().segments.map((_, i) => i)}>
            {(i) => (
              <View class="h-full overflow-hidden rounded-[3px] bg-track" style={{ flexGrow: Math.max(props.task().segments[i()]?.[0] ?? 0, 0.01) }}>
                <View
                  class={`h-full w-full origin-left transition-transform duration-300 ease-linear ${(props.task().segments[i()]?.[1] ?? 0) >= 1 ? "bg-success" : "bg-primary"}`}
                  style={{ transform: [{ scaleX: props.task().segments[i()]?.[1] ?? 0 }] }}
                />
              </View>
            )}
          </Each>
        </View>
        <Text class="text-[11.5px] tabular-nums text-ink-3">
          {`${props.task().segments.length} ${props.task().segments.length === 1 ? "segment" : "segments"} · ${props.task().connections} ${props.task().connections === 1 ? "connection" : "connections"} open`}
        </Text>
      </View>
    </If>
  );
}

function facts(t: Task): string {
  const parts = [t.statusLabel];
  if (t.status === "downloading" || t.status === "retrying") {
    parts.push(`${t.receivedText} of ${t.sizeText}`, t.speedText);
    if (t.etaText && t.etaText !== "—") parts.push(`${t.etaText} left`);
  } else if (t.sizeText !== "—") {
    parts.push(t.sizeText);
  }
  return parts.join(" · ");
}

function note(t: Task): string {
  switch (t.status) {
    case "failed":
      return t.error ?? "Something went wrong.";
    case "completed":
      return t.fileExists ? `Saved to ${t.folder}/${t.fileName ?? ""}` : "The file was moved or deleted. Download it again to get a new copy.";
    case "paused":
      return "Paused. Resume continues where it stopped.";
    case "resolving":
      return "Finding the video in the post…";
    case "queued":
      return "Waiting for a free slot.";
    case "merging":
      return "Merging video and audio…";
    case "retrying":
      return t.log[0] ?? "";
    default:
      return "";
  }
}

// ---------------------------------------------------------------------------------------------
// Status bar and empty states

function StatusBar() {
  return (
    <View class="h-[30px] flex-row items-center gap-[18px] border-t border-line px-5">
      <Stat glyph="download" text={`${speed(store.totalSpeed())} down`} />
      <Stat glyph="clock" text={`${store.runningCount()} running · ${store.queuedCount()} waiting`} />
      <View class="flex-1" />
      <Stat glyph="folder" text={`${store.settings()?.folder ?? ""} · ${store.settings()?.freeText ?? "—"} free`} />
    </View>
  );
}

function Stat(props: { glyph: string; text: string }) {
  return (
    <View class="flex-row items-center gap-1.5">
      <Glyph name={props.glyph} size={13} iconClass="text-ink-3" />
      <Text class="line-clamp-1 text-[11.5px] tabular-nums text-muted-foreground">{props.text}</Text>
    </View>
  );
}

function EmptyState() {
  return (
    <Enter spec={riseIn(10, motion.slow)} class="flex-1 items-center justify-center gap-2.5 p-10">
      <View class="size-14 items-center justify-center rounded-2xl bg-track">
        <Glyph name="download" size={26} iconClass="text-muted-foreground" />
      </View>
      <Text class="text-xl font-bold text-foreground">No downloads yet</Text>
      <Text class="max-w-[440px] text-center text-sm text-muted-foreground">
        Paste a link above. Files download over several connections and resume after interruptions; posts on X, Threads and Instagram save their video.
      </Text>
      <View class="flex-row gap-2 pt-2">
        <ToolButton glyph="paste" label="Paste Link" onPress={store.pasteFromClipboard} />
        <ToolButton glyph="plus" label="Add Several…" onPress={store.openAdd} />
      </View>
    </Enter>
  );
}

function NoMatches() {
  return (
    <Enter class="flex-1 items-center justify-center gap-1.5 p-10">
      <Text class="text-[17px] font-bold text-foreground">Nothing here</Text>
      <Text class="text-sm text-muted-foreground">
        {store.currentQuery() ? `Nothing matches “${store.currentQuery()}”.` : `No downloads in ${store.viewLabel()}.`}
      </Text>
    </Enter>
  );
}
