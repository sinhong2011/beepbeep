// SPDX-License-Identifier: AGPL-3.0-or-later
// BeepBeep: the entry (nsh.json). The screens are TSX; the download service is Rust (`beepbeep/native`).

import { Storage, View, keymap, mount, setColorScheme } from "nsh";
import { currentWindow } from "nsh/desktop";
import * as store from "./store";
import { Workspace } from "./pages/Workspace";
import { Dialogs } from "./pages/Dialogs";
import { ToastStack } from "./components/kit";

function App() {
  // Our own title bar: the sidebar runs under the traffic lights.
  currentWindow().set({ title: "BeepBeep", titleBar: { style: "hidden", height: 56 }, minSize: { width: 980, height: 600 } });
  store.start();
  Storage.getItem("appearance").then((a) => a && setColorScheme(a as "light" | "dark" | "system")).catch(() => {});
  shortcuts();
  return (
    <View class="flex-1 bg-background">
      <Workspace />
      <Dialogs />
      <ToastStack />
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
