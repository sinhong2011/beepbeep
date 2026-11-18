// SPDX-License-Identifier: AGPL-3.0-or-later
// BeepBeep: the entry (nsh.json). The screens are TSX; the download service is Rust (`beepbeep/native`).

import { Text, View, createMotion, keymap, mount } from "nsh";
import { currentWindow } from "nsh/desktop";
import * as store from "./store";
import { Workspace } from "./pages/Workspace";
import { Dialogs } from "./pages/Dialogs";
import { Glyph, motion } from "./components/ui";

function App() {
  // Our own title bar: the sidebar runs under the traffic lights.
  currentWindow().set({ title: "BeepBeep", titleBar: { style: "hidden", height: 56 }, minSize: { width: 980, height: 600 } });
  store.start();
  shortcuts();
  return (
    <View class="flex-1 bg-background">
      <Workspace />
      <Dialogs />
      <If when={store.toast()}>{(t) => <Toast text={t().text} ok={t().ok} />}</If>
    </View>
  );
}

/** Toasts slide in from the bottom edge they live on. */
function Toast(props: { text: string; ok: boolean }) {
  const m = createMotion({ enter: { duration: motion.slow, easing: "easeOut", keyframes: { opacity: [0, 1], translateY: [24, 0] } } });
  return (
    <View class="absolute inset-x-0 bottom-11 items-center" style={{ pointerEvents: "box-none" }}>
      <View class="max-w-[640px] flex-row items-center gap-2 rounded-full bg-pill px-4 py-2.5 shadow-lg" style={m.style}>
        <Glyph name={props.ok ? "check" : "alert"} size={15} iconClass={props.ok ? "text-success" : "text-warning"} />
        <Text class="line-clamp-2 text-sm text-pill-foreground">{props.text}</Text>
      </View>
    </View>
  );
}

/** ⌘N add · ⌘V paste links · ⌘, settings · ⌘I details · ⌘1–4 views · ↑/↓ select · space pause/resume · ⌘⌫ remove. */
function shortcuts() {
  const notEditing = { when: "notEditing" } as const;
  keymap.bind("mod+n", () => void store.openAdd());
  keymap.bind("mod+v", () => void store.pasteFromClipboard(), notEditing);
  keymap.bind("mod+,", () => store.showDialog("settings"));
  keymap.bind("mod+i", store.toggleInspector);
  store.VIEWS.forEach((v, i) =>
    keymap.bind(`mod+${i + 1}`, () => {
      if (store.settings()?.categoryShortcuts) store.setView(v.id);
    }),
  );
  keymap.bind("up", () => store.stepSelection(-1), notEditing);
  keymap.bind("down", () => store.stepSelection(1), notEditing);
  keymap.bind("space", () => {
    const t = store.selectedTask();
    if (t) store.primary(t);
  }, notEditing);
  keymap.bind("mod+backspace", () => {
    const t = store.selectedTask();
    if (t) void store.confirmRemove(t);
  }, notEditing);
}

mount(App);
