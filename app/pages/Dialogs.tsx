// SPDX-License-Identifier: AGPL-3.0-or-later
// The modal sheets, each an `nsh/base` Dialog: a layer over the window that hears Escape and backdrop
// presses natively. The card rises in from 0.97 while the backdrop fades.

import { type JSX, Scroll, Storage, Text, View, createMotion, setColorScheme, signal } from "nsh";
import { Dialog, Input } from "nsh/base";
import * as store from "../store";
import { Enter, Glyph, IconButton, PrimaryButton, Segmented, Toggle, ToolButton, dialogIn, motion, riseIn } from "../components/ui";

function Modal(props: { open: boolean; label: string; width: number; children: JSX.Element }) {
  return (
    <Dialog
      open={props.open}
      animation="fade"
      accessibilityLabel={props.label}
      onOpenChange={({ open }) => !open && store.showDialog(null)}
      class="flex-1 items-center justify-center"
      backdropClass="bg-scrim"
    >
      <Card width={props.width}>{props.children}</Card>
    </Dialog>
  );
}

function Card(props: { width: number; children: JSX.Element }) {
  const m = createMotion({ enter: dialogIn });
  return (
    <View class="max-h-[86%] overflow-hidden rounded-[18px] border border-line bg-surface shadow-2xl" style={[{ width: props.width }, m.style]}>
      {props.children}
    </View>
  );
}

function Header(props: { title: string }) {
  return (
    <View class="flex-row items-center px-5 pt-[18px] pb-3.5">
      <Text class="flex-1 text-xl font-bold text-foreground">{props.title}</Text>
      <IconButton glyph="close" label="Close" size={30} onPress={() => store.showDialog(null)} />
    </View>
  );
}

export function Dialogs() {
  return (
    <>
      <If when={store.openDialog() === "add"}>
        <Modal open label="Add downloads" width={600}>
          <AddSheet />
        </Modal>
      </If>
      <If when={store.openDialog() === "settings"}>
        <Modal open label="Settings" width={680}>
          <SettingsSheet />
        </Modal>
      </If>
    </>
  );
}

// ---------------------------------------------------------------------------------------------
// Add (Link Collector, plan §12–13)

const QUALITIES = [
  { value: "", label: "Default" },
  { value: "best", label: "Best" },
  { value: "1080", label: "1080p" },
  { value: "720", label: "720p" },
  { value: "480", label: "480p" },
  { value: "audio", label: "Audio" },
];

function AddSheet() {
  const draft = signal(store.draft());
  const quality = signal("");
  const links = () => store.linksIn(draft());
  const submit = () => {
    const n = store.addLinks(draft(), quality());
    if (n > 0) store.showDialog(null); // each download shows its own live toast
  };
  return (
    <View>
      <Header title="Add Downloads" />
      <View class="gap-3 px-5">
        <Text class="text-[12.5px] text-muted-foreground">
          Paste links to files or to posts on X, Threads and Instagram: one per line, or mixed with any text.
        </Text>
        <Input
          multiline
          autoFocus
          value={draft()}
          onChange={({ value }) => draft.set(value)}
          placeholder={"https://x.com/…/status/…\nhttps://www.threads.com/@…/post/…\nhttps://www.instagram.com/reel/…"}
          class="h-[130px] rounded-xl border border-line bg-surface-raised p-3 font-mono text-[12.5px] text-foreground"
        />
        <Scroll class="max-h-[170px]">
          <View class="gap-0.5">
            <Each of={links()}>
              {(url, i) => (
                <Enter spec={riseIn(4, motion.fast, Math.min(i(), 8) * 25)} class="flex-row items-center gap-2 py-1">
                  <View class="rounded-full bg-track px-2 py-0.5">
                    <Text class="text-[11px] font-semibold text-muted-foreground">{kindLabel(url())}</Text>
                  </View>
                  <Text class="line-clamp-1 flex-1 font-mono text-[12px] text-foreground">{url()}</Text>
                </Enter>
              )}
            </Each>
          </View>
        </Scroll>
        <View class="flex-row items-center gap-3">
          <Text class="text-[12.5px] font-semibold text-foreground">Video quality</Text>
          <Segmented label="Video quality" options={QUALITIES} value={quality()} onChange={(q) => quality.set(q)} />
        </View>
        <Text class="text-[12px] text-ink-3">Saves to {store.settings()?.folder ?? ""}</Text>
      </View>
      <View class="flex-row justify-end gap-2.5 p-5">
        <ToolButton label="Cancel" onPress={() => store.showDialog(null)} />
        <PrimaryButton
          label={links().length === 0 ? "Download" : links().length === 1 ? "Download 1 Link" : `Download ${links().length} Links`}
          disabled={links().length === 0}
          onPress={submit}
        />
      </View>
    </View>
  );
}

function kindLabel(url: string): string {
  const k = store.linkKind(url);
  return k === "file" ? "File" : `${k === "x" ? "X" : k[0].toUpperCase() + k.slice(1)} video`;
}

// ---------------------------------------------------------------------------------------------
// Settings

type Pane = "general" | "network" | "media" | "about";

function SettingsSheet() {
  const pane = signal<Pane>("general");
  return (
    <View>
      <Header title="Settings" />
      <View class="items-center pb-3.5">
        <Segmented
          label="Settings sections"
          options={[
            { value: "general", label: "General" },
            { value: "network", label: "Network" },
            { value: "media", label: "Media" },
            { value: "about", label: "About" },
          ]}
          value={pane()}
          onChange={(p) => pane.set(p)}
        />
      </View>
      <View class="mx-5 overflow-hidden rounded-[14px] border border-line bg-surface-raised">
        <If when={pane() === "general"}><Enter spec={riseIn(4, motion.fast)}><General /></Enter></If>
        <If when={pane() === "network"}><Enter spec={riseIn(4, motion.fast)}><Network /></Enter></If>
        <If when={pane() === "media"}><Enter spec={riseIn(4, motion.fast)}><Media /></Enter></If>
        <If when={pane() === "about"}><Enter spec={riseIn(4, motion.fast)}><About /></Enter></If>
      </View>
      <View class="flex-row justify-end p-5">
        <PrimaryButton label="Done" onPress={() => store.showDialog(null)} />
      </View>
    </View>
  );
}

function Row(props: { title: string; detail: string; children?: JSX.Element }) {
  return (
    <View class="flex-row items-center gap-4 border-b border-line px-[18px] py-3">
      <View class="flex-1 gap-0.5">
        <Text class="text-sm font-semibold text-foreground">{props.title}</Text>
        <Text class="text-[12.5px] text-muted-foreground">{props.detail}</Text>
      </View>
      {props.children}
    </View>
  );
}

function Stepper(props: { label: string; value: string; onLess: () => void; onMore: () => void }) {
  return (
    <View class="flex-row items-center gap-2">
      <IconButton glyph="close" label={`Less ${props.label}`} size={28} onPress={props.onLess} />
      <Text class="min-w-16 text-center text-sm font-semibold tabular-nums text-foreground">{props.value}</Text>
      <IconButton glyph="plus" label={`More ${props.label}`} size={28} onPress={props.onMore} />
    </View>
  );
}

const APPEARANCES = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];
const appearance = signal("system");
Storage.getItem("appearance").then((a) => a && appearance.set(a)).catch(() => {});

function General() {
  const s = () => store.settings();
  return (
    <View>
      <Row title="Appearance" detail="Follow the system, or always light or dark.">
        <Segmented
          label="Appearance"
          options={APPEARANCES}
          value={appearance()}
          onChange={(a) => {
            appearance.set(a);
            setColorScheme(a as "system" | "light" | "dark");
            Storage.setItem("appearance", a).catch(() => {});
          }}
        />
      </Row>
      <Row title="Download Folder" detail={s()?.folder ?? ""}>
        <ToolButton label="Change…" onPress={store.chooseFolder} />
      </Row>
      <Row title="Notify When a Download Finishes" detail="A system notification when a download finishes or fails.">
        <Toggle label="Notify when a download finishes" checked={!!s()?.notify} onChange={(v) => store.setSetting("notify", v)} />
      </Row>
      <Row title="Verify With SHA-256" detail="Computes a checksum after each download and shows it in Details.">
        <Toggle label="Verify with SHA-256" checked={!!s()?.verifySha256} onChange={(v) => store.setSetting("verifySha256", v)} />
      </Row>
      <Row title="Switch Views With Shortcuts" detail="⌘1–⌘4 switch to All / Downloading / Completed / Failed.">
        <Toggle label="Switch views with shortcuts" checked={!!s()?.categoryShortcuts} onChange={(v) => store.setSetting("categoryShortcuts", v)} />
      </Row>
    </View>
  );
}

const LIMITS = [0, 512_000, 1_000_000, 5_000_000, 10_000_000, 25_000_000, 50_000_000];

function Network() {
  const s = () => store.settings();
  const stepLimit = (dir: number) => {
    const cur = s()?.speedLimit ?? 0;
    const i = Math.max(0, LIMITS.findIndex((l) => l >= cur));
    store.setSetting("speedLimit", LIMITS[Math.min(LIMITS.length - 1, Math.max(0, i + dir))]);
  };
  return (
    <View>
      <Row title="Downloads at Once" detail="The rest wait in the queue.">
        <Stepper
          label="downloads at once"
          value={String(s()?.maxConcurrent ?? 3)}
          onLess={() => store.setSetting("maxConcurrent", (s()?.maxConcurrent ?? 3) - 1)}
          onMore={() => store.setSetting("maxConcurrent", (s()?.maxConcurrent ?? 3) + 1)}
        />
      </Row>
      <Row title="Connections per Download" detail="Large files on servers that allow it download in this many parts at once.">
        <Stepper
          label="connections"
          value={String(s()?.connections ?? 8)}
          onLess={() => store.setSetting("connections", (s()?.connections ?? 8) - 1)}
          onMore={() => store.setSetting("connections", (s()?.connections ?? 8) + 1)}
        />
      </Row>
      <Row title="Speed Limit" detail="Shared by all downloads.">
        <Stepper label="speed limit" value={s()?.speedLimitText ?? "Unlimited"} onLess={() => stepLimit(-1)} onMore={() => stepLimit(1)} />
      </Row>
    </View>
  );
}

function Media() {
  const s = () => store.settings();
  const tools = signal(store.tools());
  return (
    <View>
      <Row title="Video Quality" detail="For posts, unless you pick another when adding.">
        <Segmented
          label="Video quality"
          options={QUALITIES.slice(1)}
          value={s()?.quality ?? "best"}
          onChange={(q) => store.setSetting("quality", q)}
        />
      </Row>
      <Row
        title="yt-dlp"
        detail={tools().ytdlp ? `Found at ${tools().ytdlp}. Used when a post needs a login or a merge.` : "Not installed. Posts that need a login use it: brew install yt-dlp"}
      >
        <ToolButton label="Check Again" onPress={() => tools.set(store.tools())} />
      </Row>
      <Row title="Browser Cookies" detail="Lets yt-dlp use your browser's sign-in for posts you can see there. Off unless you choose one.">
        <Segmented
          label="Browser cookies"
          options={[
            { value: "none", label: "Off" },
            { value: "safari", label: "Safari" },
            { value: "chrome", label: "Chrome" },
            { value: "firefox", label: "Firefox" },
          ]}
          value={s()?.cookieBrowser ?? "none"}
          onChange={(b) => store.setSetting("cookieBrowser", b)}
        />
      </Row>
      <Row
        title="FFmpeg"
        detail={tools().ffmpeg ? `Found at ${tools().ffmpeg}. Merges separate video and audio.` : "Not installed. Needed only to merge separate video and audio: brew install ffmpeg"}
      />
    </View>
  );
}

function About() {
  return (
    <View>
      <Row title="BeepBeep 1.0" detail="Built on NSH · AGPL-3.0-or-later" />
      <Row title="Supported Links" detail="Direct HTTP(S) files, and video posts on X, Threads and Instagram. Public posts work without an account." />
      <Row title="AI Tools" detail="`beep mcp` serves the queue to AI assistants over MCP; `beep … --json` answers scripts and agents." />
      <Row title="Your Responsibility" detail="Download only what you have the right to. BeepBeep does not bypass DRM, paywalls or private accounts." />
    </View>
  );
}

export const _glyph = Glyph;
