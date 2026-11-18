// SPDX-License-Identifier: AGPL-3.0-or-later
// BeepBeep's expressive components: rolling numbers, an opening folder, an inline-confirm bin, a
// swinging bell icon and the toast stack. Original NSH implementations, written for this app; the
// interaction ideas (rolling digits, fan-out toasts, lid-lift delete) are common UI patterns.
// Everything uses theme tokens and the motion tokens in ui.tsx, and settles instantly with Reduce Motion.

import { type JSX, Text, View, createMotion, keymap, signal } from "nsh";
import { Button } from "nsh/base";
import { Glyph, motion, press } from "./ui";

// ---------------------------------------------------------------------------------------------
// RollingNumber: digits roll to their new value; places come and go with the number.

const DIGITS = "0123456789";
const FADE = "linear-gradient(to bottom, transparent 0%, black 22%, black 78%, transparent 100%)";

/**
 * Shows `text` (a number with its units, like "4.5 MB/s") with each digit in its own window that
 * rolls up or down to the new digit, fading at the window's edges instead of being cut off.
 * Non-digits (separators, units) sit between the windows. Width follows the text.
 */
export function RollingNumber(props: { text: string; size: number; class?: string; weight?: "regular" | "semibold" | "bold" }) {
  const line = () => Math.round(props.size * 1.2);
  const digitWidth = () => props.size * 0.62;
  const weight = () => (props.weight === "bold" ? "font-bold" : props.weight === "semibold" ? "font-semibold" : "");
  // Keyed from the right, so the ones place stays the ones place as the number grows.
  const chars = () => [...props.text].map((c, i, all) => ({ c, key: all.length - i }));
  return (
    <View class="flex-row items-end" accessibilityLabel={props.text}>
      <Each of={chars()} key={(c) => c.key}>
        {(ch) => (
          <If
            when={ch().c !== " "}
            else={<View style={{ width: props.size * 0.3 }} />}
          >
          <If
            when={DIGITS.includes(ch().c)}
            else={<Text class={`tabular-nums ${weight()} ${props.class ?? ""}`} style={{ fontSize: props.size, lineHeight: line() }}>{ch().c}</Text>}
          >
            <Digit value={Number(ch().c)} size={props.size} line={line()} width={digitWidth()} cls={`${weight()} ${props.class ?? ""}`} />
          </If>
          </If>
        )}
      </Each>
    </View>
  );
}

function Digit(props: { value: number; size: number; line: number; width: number; cls: string }) {
  const enter = createMotion({ enter: { duration: motion.base, easing: "easeOut", keyframes: { opacity: [0, 1], translateY: [props.line * 0.4, 0] } } });
  return (
    <View class="overflow-hidden" style={[{ height: props.line, width: props.width, maskImage: FADE }, enter.style]}>
      <View
        class="transition-transform duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] motion-reduce:duration-0"
        style={{ transform: [{ translateY: -props.value * props.line }] }}
      >
        {[...DIGITS].map((d) => (
          <Text class={`text-center tabular-nums ${props.cls}`} style={{ fontSize: props.size, lineHeight: props.line, height: props.line, width: props.width }}>
            {d}
          </Text>
        ))}
      </View>
    </View>
  );
}

// ---------------------------------------------------------------------------------------------
// FolderButton: "Show in Finder" — the front of the folder tips open and a page peeks out on hover.

export function FolderButton(props: { label: string; onPress: () => void; size?: number }) {
  const s = props.size ?? 36;
  return (
    <Button
      accessibilityLabel={props.label}
      tooltip={props.label}
      onPress={props.onPress}
      class={`group items-center justify-center rounded-[9px] border border-line bg-surface-raised hover:bg-hover ${press}`}
      style={{ width: s, height: s }}
    >
      <View style={{ width: 18, height: 15 }}>
        {/* Back of the folder, with its tab. */}
        <View class="absolute top-0 left-0 h-1 w-2 rounded-t-[2px] bg-warning/80" />
        <View class="absolute top-[2px] left-0 h-[13px] w-[18px] rounded-[2.5px] bg-warning/80" />
        {/* A page: rises out of the folder on hover. */}
        <View class="absolute top-[4px] left-[3px] h-[9px] w-3 rounded-[1.5px] bg-white shadow-sm transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] group-hover:-translate-y-[4px]" />
        {/* Front: tips forward on hover. */}
        <View
          class="absolute bottom-0 left-0 h-[10px] w-[18px] origin-bottom rounded-[2.5px] bg-warning transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] group-hover:scale-y-[0.82] group-hover:skew-x-[-8deg]"
        />
      </View>
    </Button>
  );
}

// ---------------------------------------------------------------------------------------------
// ConfirmBin: a bin whose lid lifts open; a panel slides out with ✓ (remove) and ✕ (keep).

export function ConfirmBin(props: {
  label: string;
  onConfirm: () => void;
  /** A second, destructive choice shown in the panel (e.g. also move the file to the Trash). */
  extra?: { label: string; onPress: () => void } | null;
  size?: number;
}) {
  const s = props.size ?? 36;
  const open = signal(false);
  let unbind: (() => void) | null = null;
  const close = () => {
    open.set(false);
    unbind?.();
    unbind = null;
  };
  const toggle = () => {
    if (open()) return close();
    open.set(true);
    unbind = keymap.bind("escape", close);
  };
  return (
    <View class="flex-row items-center gap-1">
      <Button
        accessibilityLabel={props.label}
        tooltip={open() ? "Keep it" : props.label}
        onPress={toggle}
        class={`items-center justify-center rounded-[9px] border transition-colors duration-200 ${open() ? "border-destructive/40 bg-destructive/10" : "border-line bg-surface-raised hover:bg-hover"} ${press}`}
        style={{ width: s, height: s }}
      >
        <View class="items-center" style={{ width: 16, height: 17 }}>
          {/* Lid with its handle: lifts and tilts while the panel is open. */}
          <View
            class={`items-center transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] origin-left ${open() ? "-translate-y-[3px] -rotate-[18deg]" : ""}`}
          >
            <View class={`h-[2px] w-[6px] rounded-t-[1px] ${open() ? "bg-destructive" : "bg-foreground"}`} />
            <View class={`h-[2px] w-[16px] rounded-full ${open() ? "bg-destructive" : "bg-foreground"}`} />
          </View>
          {/* Body. */}
          <View class={`mt-[1.5px] h-[12px] w-[12px] rounded-b-[3px] border-[1.8px] border-t-0 ${open() ? "border-destructive" : "border-foreground"}`} />
        </View>
      </Button>
      <If when={open()}>
        <ConfirmPanel onConfirm={() => { close(); props.onConfirm(); }} onCancel={close} extra={props.extra ?? null} />
      </If>
    </View>
  );
}

function ConfirmPanel(props: { onConfirm: () => void; onCancel: () => void; extra: { label: string; onPress: () => void } | null }) {
  const m = createMotion({ enter: { duration: motion.base, easing: "easeOut", keyframes: { opacity: [0, 1], translateX: [-8, 0], scale: [0.94, 1] } } });
  return (
    <View class="flex-row items-center gap-1 rounded-[10px] border border-line bg-surface-raised p-[3px] shadow-sm" style={m.style}>
      <Button accessibilityLabel="Remove from list" tooltip="Remove from list" onPress={props.onConfirm} class={`size-7 items-center justify-center rounded-[7px] bg-destructive ${press}`}>
        <Glyph name="check" size={15} iconClass="text-white" />
      </Button>
      <If when={props.extra}>
        {(x) => (
          <Button accessibilityLabel={x().label} tooltip={x().label} onPress={x().onPress} class={`h-7 items-center justify-center rounded-[7px] px-2 hover:bg-destructive/15 ${press}`}>
            <Text class="text-[11.5px] font-semibold text-destructive">{x().label}</Text>
          </Button>
        )}
      </If>
      <Button accessibilityLabel="Keep it" tooltip="Keep it" onPress={props.onCancel} class={`size-7 items-center justify-center rounded-[7px] hover:bg-hover ${press}`}>
        <Glyph name="close" size={14} iconClass="text-muted-foreground" />
      </Button>
    </View>
  );
}

// ---------------------------------------------------------------------------------------------
// SwingGlyph: an icon that swings like a bell when `count` grows (harder when it grows fast).

export function SwingGlyph(props: { name: string; size: number; iconClass: string; count: number }) {
  const m = createMotion();
  let last = props.count;
  let lastAt = 0;
  // Read the count in a binding so a change runs this; play outside the render.
  const watch = () => {
    const n = props.count;
    if (n > last) {
      const now = Date.now();
      const fast = now - lastAt < 1500;
      lastAt = now;
      const deg = fast ? 26 : 16;
      m.play({ duration: fast ? 700 : 560, easing: "easeOut", keyframes: { rotate: [0, deg, -deg * 0.7, deg * 0.4, -deg * 0.2, 0] } });
    }
    last = n;
    return null;
  };
  return (
    <View class="origin-top" style={m.style}>
      {watch()}
      <Glyph name={props.name} size={props.size} iconClass={props.iconClass} />
    </View>
  );
}

// ---------------------------------------------------------------------------------------------
// Toasts: a stack in the bottom-right corner. Each status icon pops in; a "loading" toast morphs into
// success or error in place; hovering the stack fans it out; a toast can carry one action pill.

export type ToastKind = "loading" | "success" | "error" | "info";
export interface ToastData {
  id: string;
  kind: ToastKind;
  title: string;
  description?: string | null;
  action?: { label: string; onPress: () => void } | null;
}

const toasts = signal<ToastData[]>([]);
const timers = new Map<string, number>();

/** Shows a toast, or updates the one with the same id in place. Loading toasts stay until updated. */
export function toast(t: ToastData) {
  const list = toasts().filter((x) => x.id !== t.id);
  const existing = toasts().find((x) => x.id === t.id);
  toasts.set(existing ? toasts().map((x) => (x.id === t.id ? t : x)) : [t, ...list].slice(0, 4));
  const old = timers.get(t.id);
  if (old) clearTimeout(old);
  if (t.kind !== "loading") timers.set(t.id, setTimeout(() => dismiss(t.id), t.action ? 6000 : 3800));
}

export function dismiss(id: string) {
  timers.delete(id);
  toasts.set(toasts().filter((x) => x.id !== id));
}

const ROW = 58;

export function ToastStack() {
  const expanded = signal(false);
  return (
    <View class="absolute right-5 bottom-11 w-[360px]" style={{ pointerEvents: "box-none", height: 4 * (ROW + 8) }}>
      <View
        class="absolute inset-x-0 bottom-0"
        style={{ height: toasts().length ? (expanded() ? toasts().length * (ROW + 8) : ROW + 16) : 0 }}
        onMouseEnter={() => expanded.set(true)}
        onMouseLeave={() => expanded.set(false)}
      >
        <Each of={toasts()} key={(t) => t.id}>
          {(t, i) => <ToastCard t={t} index={i} expanded={expanded()} />}
        </Each>
      </View>
    </View>
  );
}

function ToastCard(props: { t: () => ToastData; index: () => number; expanded: boolean }) {
  const enter = createMotion({ enter: { duration: motion.slow, easing: "easeOut", keyframes: { opacity: [0, 1], translateY: [28, 0], scale: [0.94, 1] } } });
  // Collapsed: cards behind the newest peek out above it, smaller and fainter. Expanded: a list.
  const offset = () => (props.expanded ? -props.index() * (ROW + 8) : -props.index() * 8);
  const scale = () => (props.expanded ? 1 : 1 - props.index() * 0.05);
  const opacity = () => (props.index() > 2 ? 0 : props.expanded ? 1 : 1 - props.index() * 0.25);
  return (
    <View
      class="group absolute inset-x-0 bottom-0 transition-all duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]"
      style={[{ height: ROW, zIndex: 10 - props.index(), opacity: opacity(), transform: [{ translateY: offset() }, { scale: scale() }] }]}
    >
      <View class="flex-1 flex-row items-center gap-3 rounded-2xl border border-line bg-surface px-3.5 shadow-lg" style={enter.style}>
        <StatusMark kind={props.t().kind} />
        <View class="flex-1 gap-0.5">
          <Text class="line-clamp-1 text-[13px] font-semibold text-foreground">{props.t().title}</Text>
          <If when={props.t().description}>
            {(d) => <Text class="line-clamp-1 text-[11.5px] text-muted-foreground">{d()}</Text>}
          </If>
        </View>
        <If when={props.t().action}>
          {(a) => (
            <Button
              accessibilityLabel={a().label}
              onPress={() => {
                a().onPress();
                dismiss(props.t().id);
              }}
              class={`h-7 items-center justify-center rounded-full bg-pill px-3 ${press}`}
            >
              <Text class="text-[11.5px] font-semibold text-pill-foreground">{a().label}</Text>
            </Button>
          )}
        </If>
        <Button
          accessibilityLabel="Dismiss"
          onPress={() => dismiss(props.t().id)}
          class={`size-6 items-center justify-center rounded-full opacity-0 transition-opacity duration-150 group-hover:opacity-100 hover:bg-hover ${press}`}
        >
          <Glyph name="close" size={12} iconClass="text-muted-foreground" />
        </Button>
      </View>
    </View>
  );
}

/** The status icon; a change of kind pops the new mark in where the old one was. */
function StatusMark(props: { kind: ToastKind }) {
  return (
    <View class="size-6 items-center justify-center">
      <If when={props.kind === "loading"}>
        <View class="animate-spin motion-reduce:animate-none">
          <Glyph name="retry" size={16} iconClass="text-info" />
        </View>
      </If>
      <If when={props.kind === "success"}><Pop><Glyph name="check" size={18} iconClass="text-success" /></Pop></If>
      <If when={props.kind === "error"}><Pop><Glyph name="alert" size={17} iconClass="text-destructive" /></Pop></If>
      <If when={props.kind === "info"}><Pop><Glyph name="download" size={17} iconClass="text-info" /></Pop></If>
    </View>
  );
}

function Pop(props: { children: JSX.Element }) {
  const m = createMotion({ enter: "popIn", duration: 380 });
  return <View style={m.style}>{props.children}</View>;
}
