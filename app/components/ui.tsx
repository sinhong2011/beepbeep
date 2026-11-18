// SPDX-License-Identifier: AGPL-3.0-or-later
// The app's styled controls on `nsh/base` (NSH Coding Guides: the app's `ui` layer before pages).
// Colors come from theme tokens (theme.json); motion from the tokens below. Pages compose these.

import { type JSX, Image, type MotionSpec, Text, View, createMotion } from "nsh";
import { Button, Progress, Switch, Tab, Tabs } from "nsh/base";
import { Glyph } from "beepbeep/native";
import type { Task } from "../store";

// ---------------------------------------------------------------------------------------------
// Motion tokens (Design Guides: "Define motion tokens once")

export const motion = { instant: 120, fast: 180, base: 240, slow: 340 } as const;

/** Rises `dy` points while fading in: content arriving. */
export const riseIn = (dy = 8, duration: number = motion.base, delay = 0): MotionSpec => ({
  duration,
  delay,
  easing: "easeOut",
  keyframes: { opacity: [0, 1], translateY: [dy, 0] },
});

/** Dialogs: from 0.97 and 10 pt low. */
export const dialogIn: MotionSpec = {
  duration: motion.slow,
  easing: "easeOut",
  keyframes: { opacity: [0, 1], scale: [0.97, 1], translateY: [10, 0] },
};

/** From the right edge: the inspector. */
export const slideInRight: MotionSpec = {
  duration: motion.base,
  easing: "easeOut",
  keyframes: { opacity: [0, 1], translateX: [24, 0] },
};

/** Rows arriving together come in as a wave, the delay capped at the 10th. */
export const wave = (index: number) => riseIn(6, motion.base, Math.min(index, 10) * 30);

/** Press feedback on a soft settle (Design Guides: "Layered feedback on controls"). */
export const press = "transition duration-150 ease-[0.22,1,0.36,1] active:scale-[0.97] motion-reduce:active:scale-100";

export { Glyph };

// ---------------------------------------------------------------------------------------------
// Buttons

/** A 32 pt icon button in a toolbar. */
export function IconButton(props: { glyph: string; label: string; onPress: () => void; size?: number }) {
  const size = props.size ?? 32;
  return (
    <Button
      accessibilityLabel={props.label}
      onPress={props.onPress}
      class={`items-center justify-center rounded-[9px] border border-line bg-surface-raised hover:bg-hover ${press}`}
      style={{ width: size, height: size }}
    >
      <Glyph name={props.glyph} size={Math.round(size * 0.5)} iconClass="text-foreground" />
    </Button>
  );
}

/** A bordered button with an icon and a label. */
export function ToolButton(props: { glyph?: string; label: string; onPress: () => void }) {
  return (
    <Button
      accessibilityLabel={props.label}
      onPress={props.onPress}
      class={`h-8 flex-row items-center gap-1.5 rounded-[9px] border border-line bg-surface-raised px-3 hover:bg-hover ${press}`}
    >
      {props.glyph ? <Glyph name={props.glyph} size={14} iconClass="text-foreground" /> : null}
      <Text class="text-[12.5px] font-semibold text-foreground">{props.label}</Text>
    </Button>
  );
}

/** The main action: filled with the action color. */
export function PrimaryButton(props: { label: string; glyph?: string; onPress: () => void; disabled?: boolean; class?: string }) {
  return (
    <Button
      accessibilityLabel={props.label}
      onPress={props.onPress}
      disabled={props.disabled}
      class={`h-9 flex-row items-center justify-center gap-1.5 rounded-[10px] bg-primary px-4 hover:opacity-90 disabled:opacity-40 ${press} ${props.class ?? ""}`}
    >
      {props.glyph ? <Glyph name={props.glyph} size={15} iconClass="text-primary-foreground" /> : null}
      <Text class="text-[13.5px] font-semibold text-primary-foreground">{props.label}</Text>
    </Button>
  );
}

/** A quiet circular button for a row's quick action. */
export function RoundButton(props: { glyph: string; label: string; onPress: () => void }) {
  return (
    <Button
      accessibilityLabel={props.label}
      onPress={props.onPress}
      class={`size-[30px] items-center justify-center rounded-full hover:bg-track ${press}`}
    >
      <Glyph name={props.glyph} size={15} iconClass="text-muted-foreground" />
    </Button>
  );
}

// ---------------------------------------------------------------------------------------------
// nsh/base controls, styled

/** `Progress`: role, range and value for screen readers; the indicator eases between samples. */
export function ProgressBar(props: { value: number | null; tone?: "action" | "done" | "paused" | "failed"; height?: number }) {
  const indicator = () =>
    ({ action: "bg-primary", done: "bg-success", paused: "bg-ink-3", failed: "bg-destructive" })[props.tone ?? "action"];
  return (
    <Progress
      value={(props.value ?? 0) * 100}
      indeterminate={props.value === null}
      accessibilityLabel="Download progress"
      class="w-full"
      style={{ height: props.height ?? 5 }}
      trackClass="w-full overflow-hidden rounded-full bg-track"
      trackStyle={{ height: props.height ?? 5 }}
      indicatorClass={`h-full rounded-full ${indicator()} transition-all duration-300 ease-linear indeterminate:w-1/3 indeterminate:animate-pulse`}
    />
  );
}

/** `Switch`: the thumb glides across, the track changes color. */
export function Toggle(props: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <Switch
      checked={props.checked}
      accessibilityLabel={props.label}
      onChange={({ checked }) => props.onChange(checked)}
      trackClass="h-[26px] w-11 rounded-full bg-track p-[3px] transition-colors duration-200 checked:bg-primary"
      thumbClass="size-5 rounded-full bg-white shadow-sm transition-transform duration-250 ease-[0.22,1,0.36,1] checked:translate-x-[18px]"
    />
  );
}

/** `Tabs` as a segmented control: one pill springs to the chosen option. */
export function Segmented<T extends string>(props: { label: string; options: { value: T; label: string }[]; value: T; onChange: (v: T) => void }) {
  return (
    <Tabs
      accessibilityLabel={props.label}
      class="flex-row self-start gap-0.5 rounded-[10px] bg-track p-[3px]"
      indicatorClass="inset-y-[3px] rounded-[8px] bg-surface shadow-sm"
    >
      {props.options.map((o) => (
        <Tab
          selected={props.value === o.value}
          onPress={() => props.onChange(o.value)}
          class={`h-7 items-center justify-center px-3 ${press}`}
        >
          <Text class={`text-[12.5px] font-semibold transition-colors ${props.value === o.value ? "text-foreground" : "text-muted-foreground"}`}>
            {o.label}
          </Text>
        </Tab>
      ))}
    </Tabs>
  );
}

// ---------------------------------------------------------------------------------------------
// Task pieces

const statusGlyph: Record<string, string> = {
  completed: "check",
  failed: "alert",
  cancelled: "alert",
  paused: "pause",
  queued: "clock",
};
const statusTone: Record<string, string> = {
  completed: "text-success",
  failed: "text-destructive",
  cancelled: "text-ink-3",
  paused: "text-ink-3",
  queued: "text-ink-3",
  retrying: "text-warning",
};

export function StatusIcon(props: { status: string; size?: number }) {
  return <Glyph name={statusGlyph[props.status] ?? "download"} size={props.size ?? 13} iconClass={statusTone[props.status] ?? "text-info"} />;
}

export const progressTone = (t: Task) =>
  t.status === "completed" ? "done" : t.status === "failed" ? "failed" : t.status === "paused" || t.status === "cancelled" ? "paused" : "action";

/** The thumbnail, or a tile in the platform's colors. `If` keeps the image while the task updates. */
export function Thumb(props: { task: () => Task | null; size: number; radius?: number; glyph?: number }) {
  const r = props.radius ?? Math.round(props.size / 4);
  const glyph = props.glyph ?? Math.round(props.size * 0.42);
  const platform = () => props.task()?.platform ?? "file";
  const tile = (
    <View class="size-full">
      <If when={platform() === "x" || platform() === "threads"}>
        <View class={`size-full items-center justify-center ${platform() === "x" ? "bg-x" : "bg-threads"}`}>
          <Text class="font-bold text-background" style={{ fontSize: glyph }}>{platform() === "x" ? "𝕏" : "@"}</Text>
        </View>
      </If>
      <If when={platform() === "instagram"}>
        <View class="size-full items-center justify-center bg-linear-to-br from-instagram to-instagram-2">
          <Text class="font-bold text-white" style={{ fontSize: glyph }}>◎</Text>
        </View>
      </If>
      <If when={platform() === "file" || platform() === "video"}>
        <View class="size-full items-center justify-center bg-linear-to-br from-file to-file-2">
          <Glyph name="file" size={glyph} iconClass="text-file-foreground" />
        </View>
      </If>
    </View>
  );
  return (
    <View class="overflow-hidden" style={{ width: props.size, height: props.size, borderRadius: r }}>
      <If when={props.task()?.thumbnail} else={tile}>
        {(src) => <Image src={src()} class="size-full" style={{ objectFit: "cover" }} />}
      </If>
    </View>
  );
}

export function SectionLabel(props: { children: string; class?: string }) {
  return <Text class={`px-2 text-[10.5px] font-semibold tracking-[0.6px] text-ink-3 uppercase ${props.class ?? ""}`}>{props.children}</Text>;
}

/** A view that rises in when it mounts. */
export function Enter(props: { spec?: MotionSpec; class?: string; children: JSX.Element }) {
  const m = createMotion({ enter: props.spec ?? riseIn() });
  return (
    <View class={props.class} style={m.style}>
      {props.children}
    </View>
  );
}
