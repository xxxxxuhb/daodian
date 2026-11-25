<script setup lang="ts">
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, useTemplateRef, watch } from "vue";
import { savePlan, saveTodo, setStageDone, setTodoDone } from "../../api/library";
import { useLibrary } from "../../data/useLibrary";
import { useSettings } from "../../data/useSettings";
import { useNow } from "../../domain/clock";
import { clockTime, relative, upcoming } from "../../domain/present";
import { REMINDER_SETTLED, reminderStillPending, type ReminderSettled } from "../../domain/reminderSync";
import type { Settings } from "../../domain/settings";
import { usePetShots } from "./usePetShots";
import PetLook from "./PetLook.vue";
import { petPalette } from "./look";

interface PetItem {
  key: string;
  at: number;
  title: string;
  detail: string;
  kind: "todo" | "stage";
  todoId?: string;
  planId?: string;
  stageId?: string;
}

interface ReminderEvent {
  key: string;
  title: string;
  detail: string;
  at: number;
  todoId?: string;
  planId?: string;
  stageId?: string;
}

interface PetSnapshot {
  pinches: number;
  hiddenUntil: number;
}

type PetAction =
  | "tilt"
  | "yawn"
  | "antenna"
  | "tap"
  | "stretch"
  | "roll"
  | "peek"
  | "startled"
  | "celebrate"
  | "stamp"
  | "shrink"
  | "nibble"
  | "flinch"
  | "fed"
  | "puff"
  | "burp"
  | "hop"
  | "look"
  | "nod"
  | "sway"
  | "shake"
  | "sneeze";

const COMMON_ACTIONS: PetAction[] = ["tilt", "yawn", "antenna", "tap", "look", "nod", "sway"];
const RARE_ACTIONS: PetAction[] = ["stretch", "roll", "peek", "startled", "hop", "shake", "sneeze"];
const RARE_COOLDOWN = 3 * 60_000;
// 动作时长要和 pet.css 里的 animation-duration 对上，否则类名会早摘或晚摘。
const ACTION_MS: Partial<Record<PetAction, number>> = {
  peek: 2_000,
  look: 1_700,
  sway: 2_400,
  hop: 1_100,
  shake: 700,
  sneeze: 900,
};

const { todos, plans, reload } = useLibrary();
const { settings, load: loadSettings } = useSettings();
const now = useNow();
const hovered = ref(false);
const panel = ref<"none" | "card" | "menu">("none");
const pressed = ref(false);
const pinched = ref(false);
const pinches = ref(0);
const activeReminder = ref<PetItem | null>(null);
const wheelBase = ref<number | null>(null);
const wheelOffset = ref(0);
const moving = ref(false);
const snapped = ref<"" | "left" | "right">("");
const hopping = ref(false);
const blinking = ref(false);
const action = ref<PetAction | "">("");
let unlistenData: UnlistenFn | null = null;
let unlistenReminder: UnlistenFn | null = null;
let unlistenSettled: UnlistenFn | null = null;
let unlistenSettings: UnlistenFn | null = null;
let unlistenLayout: UnlistenFn | null = null;
let holdTimer = 0;
let down: { clientX: number; clientY: number; screenX: number; screenY: number } | null = null;
let frame = 0;
let pendingMove: { x: number; y: number } | null = null;
let moveChain: Promise<unknown> = Promise.resolve();
let randomTimer = 0;
let blinkTimer = 0;
let actionTimer = 0;
let hopTimer = 0;
let snapTimer = 0;
let rareAvailableAt = 0;
let recentActions: PetAction[] = [];
let completionBaseline: number | null = null;

const next = computed<PetItem | null>(() => {
  const item = upcoming(todos.value, plans.value, now.value);
  if (!item) return null;
  return {
    key: `${item.kind}:${item.todoId ?? item.planId}:${item.stageId ?? ""}:${item.at}`,
    at: item.at,
    title: item.title,
    detail:
      item.kind === "stage"
        ? `${item.context} · 阶段 ${item.index}/${item.total}`
        : "待办 · 只提醒这一次",
    kind: item.kind,
    todoId: item.todoId,
    planId: item.planId,
    stageId: item.stageId,
  };
});
const item = computed(() => activeReminder.value ?? next.value);
const followInput = computed(() => settings.value.pet.followInput !== false);
const stageRight = shallowRef(new URLSearchParams(location.search).get("stage") === "right");
const night = computed(() => {
  const hour = new Date(now.value).getHours();
  return hour >= 22 || hour < 8;
});
const shotGate = computed(() => ({
  hovered: hovered.value,
  blocked: panel.value !== "none",
  night: night.value,
}));
const interaction = computed(() => settings.value.pet.interaction || "normal");
const shots = usePetShots(followInput, stageRight, shotGate, interaction);
const sleeping = computed(() => night.value && !shots.engaged.value);
const rainEl = useTemplateRef<SVGSVGElement>("rain");
const anchorEl = useTemplateRef<HTMLElement>("anchor");
let drag: { id: number; x: number; y: number; moved: number } | null = null;
let spotTimer = 0;
let spotKey = "";
const isDue = computed(() => item.value != null && item.value.at <= now.value);
const isSoon = computed(() => {
  const delta = (item.value?.at ?? Infinity) - now.value;
  return delta > 0 && delta <= 60 * 60_000;
});
const mode = computed(() => {
  if (panel.value === "card") return "card";
  if (panel.value === "menu") return "menu";
  if (action.value === "peek") return "hover";
  return hovered.value ? "hover" : "compact";
});
const dragging = shallowRef(false);
const hitMode = computed(() => {
  if (panel.value === "card") return "card";
  if (panel.value === "menu") return "menu";
  if (dragging.value) return "drag";
  if (hovered.value || action.value === "peek") return "hover";
  return "compact";
});
const eyesOpen = computed(
  () => !sleeping.value && !pinched.value && !blinking.value && action.value !== "yawn",
);
const state = computed(() =>
  sleeping.value ? "sleep" : isDue.value ? "due" : isSoon.value ? "soon" : "idle",
);
const bubble = computed(() => {
  if (!next.value) return "今天没有安排，歇一会儿";
  const when = relative(next.value.at, now.value).text;
  return `下一个 ${clockTime(next.value.at)} ${next.value.title} · ${when}`;
});
const ringProgress = computed(() => {
  if (!item.value || !isSoon.value) return 0;
  return Math.max(0, Math.min(1, 1 - (item.value.at - now.value) / 3_600_000));
});
const completedCount = computed(
  () =>
    todos.value.filter((todo) => todo.done).length +
    plans.value.flatMap((plan) => plan.stages).filter((stage) => stage.done).length,
);
const palette = computed(() => petPalette(settings.value.pet.look?.palette));
const petStyle = computed(() => ({
  "--pet-body": palette.value.body,
  "--pet-ring": palette.value.ring,
  "--pet-halo": palette.value.halo,
  "--ring-offset": `${157 * (1 - ringProgress.value)}`,
  "--eye-x": eyesOpen.value ? `${shots.eye.value.x.toFixed(2)}px` : "0px",
  "--eye-y": eyesOpen.value ? `${shots.eye.value.y.toFixed(2)}px` : "0px",
  "--pulse-dur": `${shots.pulse.value.dur.toFixed(2)}s`,
  "--pulse-amp": shots.pulse.value.amp.toFixed(3),
  "--tip-glow": `${shots.pulse.value.glow.toFixed(1)}px`,
}));

watch(hitMode, (value) => {
  void invoke("pet_set_mode", { mode: value }).catch(() => {});
});
watch(
  () => item.value?.key,
  () => {
    wheelBase.value = null;
    wheelOffset.value = 0;
  },
);
watch(isDue, (due) => {
  if (due) interruptAction();
});
watch(shots.shooting, (active) => {
  if (active) interruptAction();
});
watch([todos, plans], () => {
  const current = activeReminder.value;
  if (!current || reminderStillPending(current, todos.value, plans.value)) return;
  hideReminderCard();
});
watch(completedCount, (count) => {
  if (completionBaseline == null) {
    completionBaseline = count;
    return;
  }
  if (count > completionBaseline) playAction("celebrate", 1600);
  completionBaseline = count;
});

function canIdleAnimate() {
  return (
    !document.hidden &&
    !hovered.value &&
    panel.value === "none" &&
    !pressed.value &&
    !moving.value &&
    !isDue.value &&
    !sleeping.value &&
    !shots.shooting.value
  );
}

function randomBetween(min: number, max: number) {
  return min + Math.random() * (max - min);
}

function idleDelay() {
  const minSeconds = Math.max(5, Number(settings.value.pet.idleMinSeconds) || 12);
  const maxSeconds = Math.max(minSeconds, Number(settings.value.pet.idleMaxSeconds) || minSeconds);
  return randomBetween(minSeconds * 1000, maxSeconds * 1000);
}

function scheduleRandomAction(delay = idleDelay()) {
  window.clearTimeout(randomTimer);
  randomTimer = window.setTimeout(runRandomAction, delay);
}

function scheduleBlink() {
  window.clearTimeout(blinkTimer);
  blinkTimer = window.setTimeout(() => {
    if (canIdleAnimate() && !action.value) {
      blinking.value = true;
      window.setTimeout(() => (blinking.value = false), 170);
    }
    scheduleBlink();
  }, randomBetween(4_000, 9_000));
}

async function runRandomAction() {
  const systemAllows = await invoke<boolean>("pet_can_idle").catch(() => true);
  if (!systemAllows || !canIdleAnimate()) {
    scheduleRandomAction(idleDelay());
    return;
  }
  const useRare = Date.now() >= rareAvailableAt && Math.random() < 0.15;
  const source = useRare ? RARE_ACTIONS : COMMON_ACTIONS;
  const available = source.filter((name) => !recentActions.includes(name));
  const choices = available.length ? available : source;
  const selected = choices[Math.floor(Math.random() * choices.length)] ?? "tilt";
  if (useRare) rareAvailableAt = Date.now() + RARE_COOLDOWN;
  playAction(selected, ACTION_MS[selected] ?? 1_200);
}

function playAction(name: PetAction, duration = 1_200) {
  window.clearTimeout(actionTimer);
  action.value = name;
  blinking.value = false;
  recentActions = [name, ...recentActions.filter((item) => item !== name)].slice(0, 3);
  actionTimer = window.setTimeout(() => {
    action.value = "";
    scheduleRandomAction();
  }, duration);
}

function interruptAction() {
  window.clearTimeout(actionTimer);
  action.value = "";
  blinking.value = false;
  scheduleRandomAction();
}

watch(
  () => shots.cue.value,
  (next) => {
    if (!next) return;
    if (next.name === "hop") {
      interruptAction();
      startHop(next.ms);
      return;
    }
    playAction(next.name, next.ms);
  },
);

function rainPoint(event: PointerEvent) {
  const svg = rainEl.value;
  if (!svg) return null;
  const rect = svg.getBoundingClientRect();
  if (!rect.width || !rect.height) return null;
  return {
    x: ((event.clientX - rect.left) / rect.width) * 300,
    y: ((event.clientY - rect.top) / rect.height) * 300,
  };
}

function stopGlyphDrag() {
  window.removeEventListener("pointermove", onGlyphMove);
  window.removeEventListener("pointerup", onGlyphUp);
  window.removeEventListener("pointercancel", onGlyphUp);
  dragging.value = false;
}

function onGlyphDown(event: PointerEvent, id: number) {
  if (event.button !== 0 || sleeping.value || panel.value !== "none" || drag) return;
  const point = rainPoint(event);
  if (!point) return;
  drag = { id, x: point.x, y: point.y, moved: 0 };
  dragging.value = true;
  window.addEventListener("pointermove", onGlyphMove);
  window.addEventListener("pointerup", onGlyphUp);
  window.addEventListener("pointercancel", onGlyphUp);
  shots.holdGlyph(id);
}

function onGlyphMove(event: PointerEvent) {
  if (!drag) return;
  const point = rainPoint(event);
  if (!point) return;
  drag.moved += Math.hypot(point.x - drag.x, point.y - drag.y);
  drag.x = point.x;
  drag.y = point.y;
  shots.moveGlyph(drag.id, point.x, point.y);
}

function onGlyphUp(event: PointerEvent) {
  if (!drag) return;
  const current = drag;
  drag = null;
  stopGlyphDrag();
  const box = anchorEl.value?.getBoundingClientRect();
  const over =
    !!box &&
    event.clientX >= box.left &&
    event.clientX <= box.right &&
    event.clientY >= box.top &&
    event.clientY <= box.bottom;
  if (over) shots.feedGlyph(current.id);
  else if (current.moved < 6) shots.popGlyph(current.id);
  else shots.dropGlyph(current.id);
}

function windowPoint(rect: DOMRect, x: number, y: number) {
  return {
    x: rect.left + (x / 300) * rect.width,
    y: rect.top + (y / 300) * rect.height,
  };
}

function windowLen(rect: DOMRect, n: number) {
  return (n / 300) * rect.width;
}

function reportSpots() {
  const svg = rainEl.value;
  if (!svg || sleeping.value || document.hidden) {
    if (spotKey) {
      spotKey = "";
      void invoke("pet_set_hotspots", { spots: [], patches: [] }).catch(() => {});
    }
    return;
  }
  const rect = svg.getBoundingClientRect();
  const glyphs = shots.glyphs.value.filter((glyph) => glyph.opacity > 0.15);
  const spots = glyphs.map((glyph) => windowPoint(rect, glyph.x, glyph.y));
  const patches: { kind: string; x: number; y: number; x2: number; y2: number }[] = [];
  const wave = shots.wave.value;
  if (wave && wave.opacity > 0.05 && wave.r > 1) {
    const tip = windowPoint(rect, shots.tip.value.x, shots.tip.value.y);
    patches.push({ kind: "disk", x: tip.x, y: tip.y, x2: windowLen(rect, wave.r + 8), y2: 0 });
  }
  for (const beam of shots.beams.value) {
    if (beam.opacity <= 0.05) continue;
    const from = windowPoint(rect, beam.fromX, beam.fromY);
    const to = windowPoint(rect, beam.x, beam.y);
    patches.push({ kind: "beam", x: from.x, y: from.y, x2: to.x, y2: to.y });
  }
  for (const glyph of glyphs) {
    const point = windowPoint(rect, glyph.x, glyph.y);
    const halfW = windowLen(rect, (glyph.wide ? 22 : 11) + 16);
    const halfH = windowLen(rect, 11 + 16);
    patches.push({
      kind: "rect",
      x: point.x - halfW,
      y: point.y - halfH,
      x2: point.x + halfW,
      y2: point.y + halfH,
    });
    if (glyph.job) {
      const tip = windowPoint(rect, shots.tip.value.x, shots.tip.value.y);
      patches.push({ kind: "beam", x: point.x, y: point.y, x2: tip.x, y2: tip.y });
    }
  }
  for (const burst of shots.bursts.value) {
    if (burst.opacity <= 0.05) continue;
    const point = windowPoint(rect, burst.x, burst.y);
    patches.push({ kind: "disk", x: point.x, y: point.y, x2: windowLen(rect, 28), y2: 0 });
  }
  const key = patches
    .map((patch) => `${patch.kind}:${patch.x.toFixed(0)},${patch.y.toFixed(0)},${patch.x2.toFixed(0)},${patch.y2.toFixed(0)}`)
    .join(";");
  if (key === spotKey) return;
  spotKey = key;
  void invoke("pet_set_hotspots", { spots, patches }).catch(() => {});
}

function leavePet() {
  hovered.value = false;
}

function startHop(ms = 720) {
  window.clearTimeout(hopTimer);
  hopping.value = false;
  requestAnimationFrame(() => {
    hopping.value = true;
    hopTimer = window.setTimeout(() => {
      hopping.value = false;
    }, ms);
  });
}

function enterPet() {
  hovered.value = true;
  shots.showRecap();
  interruptAction();
  startHop();
}

function openTarget() {
  const target = item.value;
  void invoke("pet_open_main", {
    kind: target?.kind ?? "todo",
    title: target?.title ?? "",
    todoId: target?.todoId,
    planId: target?.planId,
    stageId: target?.stageId,
  }).catch(() => {});
}

function onClick() {
  if (moving.value || pinched.value) return;
  interruptAction();
}

function onDoubleClick() {
  interruptAction();
  panel.value = "none";
  openTarget();
}

function onPointerDown(event: PointerEvent) {
  if (panel.value !== "none" || event.button !== 0) return;
  interruptAction();
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  hovered.value = false;
  void invoke("pet_set_mode", { mode: "compact" }).catch(() => {});
  pressed.value = true;
  moving.value = false;
  pinched.value = false;
  down = {
    clientX: event.clientX - rect.left,
    clientY: event.clientY - rect.top,
    screenX: event.screenX,
    screenY: event.screenY,
  };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  holdTimer = window.setTimeout(async () => {
    if (moving.value || !pressed.value) return;
    pinched.value = true;
    pinches.value = await invoke<number>("pet_record_pinch").catch(() => pinches.value + 1);
  }, 520);
}

function flushMove() {
  frame = 0;
  const move = pendingMove;
  pendingMove = null;
  if (move) {
    moveChain = moveChain
      .catch(() => undefined)
      .then(() => invoke("pet_move_to", move));
  }
}

function onPointerMove(event: PointerEvent) {
  if (!down || !pressed.value) return;
  if (Math.hypot(event.screenX - down.screenX, event.screenY - down.screenY) < 4) return;
  moving.value = true;
  window.clearTimeout(holdTimer);
  const scale = window.devicePixelRatio || 1;
  pendingMove = {
    x: Math.round((event.screenX - down.clientX) * scale),
    y: Math.round((event.screenY - down.clientY) * scale),
  };
  if (!frame) frame = requestAnimationFrame(flushMove);
}

async function onPointerUp() {
  window.clearTimeout(holdTimer);
  pressed.value = false;
  down = null;
  if (moving.value) {
    if (frame) cancelAnimationFrame(frame);
    flushMove();
    await moveChain.catch(() => undefined);
    const layout = await invoke<{ opensRight: boolean }>("pet_snap").catch(() => null);
    if (layout) {
      stageRight.value = layout.opensRight;
      // 吸附到位：天线朝空的一侧探一下，身体压一下再回弹。
      snapped.value = layout.opensRight ? "right" : "left";
      window.clearTimeout(snapTimer);
      snapTimer = window.setTimeout(() => (snapped.value = ""), 420);
    }
    window.setTimeout(() => (moving.value = false), 80);
  }
  if (pinched.value) window.setTimeout(() => (pinched.value = false), 180);
}

function hideReminderCard() {
  activeReminder.value = null;
  if (panel.value === "card") panel.value = "none";
}

function publishSettled(key: string) {
  void emit(REMINDER_SETTLED, { key } satisfies ReminderSettled).catch(() => undefined);
}

function dismissCard() {
  const key = activeReminder.value?.key;
  hideReminderCard();
  panel.value = "none";
  if (key) publishSettled(key);
}

async function complete() {
  const target = item.value;
  if (!target) return;
  const key = activeReminder.value?.key;
  if (target.todoId) await setTodoDone(target.todoId, true);
  else if (target.planId && target.stageId) {
    await setStageDone(target.planId, target.stageId, true);
  }
  hideReminderCard();
  panel.value = "none";
  if (key) publishSettled(key);
  await reload();
}

async function snoozeTo(at: number) {
  const target = item.value;
  if (!target) return;
  const key = activeReminder.value?.key;
  if (target.todoId) {
    const todo = todos.value.find((entry) => entry.id === target.todoId);
    if (!todo) return;
    await saveTodo({
      id: todo.id,
      title: todo.title,
      note: todo.note,
      dueAt: at,
      priority: todo.priority,
      repeat: todo.repeat,
      leadMs: todo.leadMs,
    });
  } else if (target.planId && target.stageId) {
    const plan = plans.value.find((entry) => entry.id === target.planId);
    if (!plan) return;
    await savePlan({
      id: plan.id,
      title: plan.title,
      note: plan.note,
      priority: plan.priority,
      templateId: plan.templateId,
      stages: plan.stages.map((stage) => ({
        id: stage.id,
        title: stage.title,
        at: stage.id === target.stageId ? at : stage.at,
        done: stage.done,
        dependsOn: stage.dependsOn,
        completedAt: stage.completedAt,
      })),
    });
  }
  activeReminder.value = null;
  if (key) publishSettled(key);
  await reload();
}

async function snooze(minutes: number) {
  await snoozeTo(Date.now() + minutes * 60_000);
  panel.value = "none";
}

function onWheel(event: WheelEvent) {
  const target = item.value;
  if (!target || sleeping.value) return;
  event.preventDefault();
  wheelBase.value ??= target.at;
  wheelOffset.value = Math.max(0, wheelOffset.value + (event.deltaY >= 0 ? 5 : -5));
  void snoozeTo(wheelBase.value + wheelOffset.value * 60_000);
}

function contextMenu() {
  interruptAction();
  panel.value = "menu";
}

function closePanelOnBlur() {
  panel.value = "none";
  hovered.value = false;
}

function hideFor(ms: number) {
  panel.value = "none";
  void invoke("pet_hide_until", { until: Date.now() + ms }).catch(() => {});
}

function hideToday() {
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(8, 0, 0, 0);
  hideFor(tomorrow.getTime() - Date.now());
}

onMounted(async () => {
  window.addEventListener("blur", closePanelOnBlur);
  const [snapshot, layout] = await Promise.all([
    invoke<PetSnapshot>("pet_snapshot").catch(() => ({ pinches: 0, hiddenUntil: 0 })),
    invoke<{ opensRight: boolean }>("pet_layout").catch(() => null),
    reload(),
    loadSettings(),
  ]);
  if (layout) stageRight.value = layout.opensRight;
  pinches.value = snapshot.pinches;
  completionBaseline = completedCount.value;
  document.documentElement.dataset.theme = settings.value.theme;
  scheduleBlink();
  scheduleRandomAction();
  spotTimer = window.setInterval(reportSpots, 100);
  unlistenData = await listen("data-changed", () => reload()).catch(() => null);
  unlistenSettings = await listen<Settings>("settings-changed", ({ payload }) => {
    settings.value = payload;
    document.documentElement.dataset.theme = payload.theme;
    if (!action.value) scheduleRandomAction();
  }).catch(() => null);
  unlistenReminder = await listen<ReminderEvent[]>("reminder-fired", ({ payload }) => {
    const first = payload[0];
    if (!first) return;
    interruptAction();
    activeReminder.value = {
      key: first.key,
      at: first.at,
      title: first.title,
      detail: first.detail,
      kind: first.todoId ? "todo" : "stage",
      todoId: first.todoId,
      planId: first.planId,
      stageId: first.stageId,
    };
    panel.value = "card";
  }).catch(() => null);
  unlistenLayout = await listen<{ opensRight: boolean }>("pet-layout", ({ payload }) => {
    stageRight.value = !!payload?.opensRight;
  }).catch(() => null);
  unlistenSettled = await listen<ReminderSettled>(REMINDER_SETTLED, ({ payload }) => {
    if (payload?.key && activeReminder.value?.key === payload.key) hideReminderCard();
  }).catch(() => null);
});

onBeforeUnmount(() => {
  window.removeEventListener("blur", closePanelOnBlur);
  unlistenData?.();
  unlistenReminder?.();
  unlistenSettled?.();
  unlistenSettings?.();
  unlistenLayout?.();
  window.clearTimeout(holdTimer);
  window.clearTimeout(randomTimer);
  window.clearTimeout(blinkTimer);
  window.clearTimeout(actionTimer);
  window.clearTimeout(hopTimer);
  window.clearTimeout(snapTimer);
  stopGlyphDrag();
  window.clearInterval(spotTimer);
  spotKey = "";
  void invoke("pet_set_hotspots", { spots: [], patches: [] }).catch(() => {});
  if (frame) cancelAnimationFrame(frame);
});
</script>

<template>
  <main
    class="pet-window"
    :class="[
      `is-${mode}`,
      `is-${state}`,
      action && `is-action-${action}`,
      { 'is-stage-right': stageRight },
      { 'is-firing': shots.beams.value.length > 0 },
      { 'is-boom': shots.mood.value === 'boom', 'is-still': shots.mood.value === 'still' },
      { 'is-pressed': pressed, 'is-pinched': pinched, 'is-blinking': blinking, 'is-hop': hopping },
      { 'is-snapped': snapped, 'is-snapped-left': snapped === 'left', 'is-snapped-right': snapped === 'right' },
    ]"
    :style="petStyle"
    @mouseleave="leavePet"
  >
    <div class="pet-stage">
    <section v-if="panel === 'card'" class="pet-card">
      <div class="pet-card__cap">
        <span>到点</span>
        <time>{{ item ? clockTime(item.at) : "—" }}</time>
      </div>
      <h3>{{ item?.title || "暂时没有安排" }}</h3>
      <p>{{ item?.detail || "享受一小段空闲时间" }}</p>
      <div class="pet-card__actions">
        <button type="button" class="is-primary" :disabled="!item" @click="complete">完成</button>
        <button type="button" @click="openTarget">打开</button>
        <button type="button" class="pet-card__close" @click="dismissCard">×</button>
      </div>
      <div class="pet-card__zones">
        <button v-for="minutes in [10, 30, 60]" :key="minutes" type="button" :disabled="!item" @click="snooze(minutes)">
          {{ minutes === 60 ? "1 小时" : `${minutes} 分` }}
        </button>
      </div>
    </section>

    <section v-else-if="panel === 'menu'" class="pet-menu">
      <b>桌面珠子</b>
      <button type="button" @click="hideFor(60 * 60_000)">暂停 1 小时</button>
      <button type="button" @click="openTarget">打开主窗口</button>
      <button type="button" @click="hideToday">今天不再出现</button>
      <small>已经捏过 {{ pinches }} 次</small>
      <button type="button" class="pet-menu__close" @click="panel = 'none'">返回</button>
    </section>

    <svg ref="rain" class="pet-rain" viewBox="0 0 300 300" aria-hidden="true">
      <g
        v-for="glyph in shots.glyphs.value"
        :key="glyph.id"
        class="pet-glyph"
        :class="{ 'is-wide': glyph.wide, 'is-target': glyph.targeted, 'is-loose': glyph.loose, 'is-frozen': glyph.frozen }"
        :opacity="glyph.opacity"
        :style="{ '--ink': glyph.ink.ink, '--paper': glyph.ink.paper, '--glow': glyph.ink.glow }"
        :transform="`translate(${glyph.x + glyph.jolt} ${glyph.y}) rotate(${glyph.rot}) scale(${glyph.scale})`"
        @pointerdown.stop="onGlyphDown($event, glyph.id)"
      >
        <rect class="pet-glyph-hit" x="-20" y="-20" width="40" height="40" />
        <template v-if="glyph.label">
          <rect :x="glyph.wide ? -22 : -11" y="-11" :width="glyph.wide ? 44 : 22" height="22" rx="6" />
          <text y="4">{{ glyph.label }}</text>
        </template>
        <circle v-else class="pet-ink" r="3.2" />
      </g>
      <g
        v-for="beam in shots.beams.value"
        :key="beam.id"
        :opacity="beam.opacity"
        :style="{ '--glow': beam.ink.glow, '--core': beam.ink.core, '--head': beam.ink.head }"
      >
        <line class="pet-beam-glow" :x1="beam.fromX" :y1="beam.fromY" :x2="beam.headX" :y2="beam.headY" />
        <line class="pet-beam-core" :x1="beam.fromX" :y1="beam.fromY" :x2="beam.headX" :y2="beam.headY" />
        <circle class="pet-beam-head" :cx="beam.headX" :cy="beam.headY" r="1.7" />
      </g>
      <g v-if="shots.wave.value" class="pet-wave" :class="{ 'is-still': shots.mood.value === 'still' }" :opacity="shots.wave.value.opacity">
        <circle class="pet-wave-fill" :cx="shots.tip.value.x" :cy="shots.tip.value.y" :r="Math.max(0, shots.wave.value.r - 16)" />
        <circle class="pet-wave-ring" :cx="shots.tip.value.x" :cy="shots.tip.value.y" :r="shots.wave.value.r" />
      </g>
      <g v-for="burst in shots.bursts.value" :key="`burst-${burst.id}`" :opacity="burst.opacity" :style="{ '--ink': burst.ink }">
        <circle
          v-for="(dot, index) in burst.dots"
          :key="index"
          class="pet-burst-dot"
          :cx="dot.x"
          :cy="dot.y"
          :r="dot.r"
        />
      </g>
    </svg>
    <div v-if="mode === 'hover' && action !== 'peek'" class="pet-bubble">{{ shots.recap.value || bubble }}</div>
    <div
      ref="anchor"
      class="pet-anchor"
      @mouseenter="enterPet"
      @click="onClick"
      @dblclick="onDoubleClick"
      @contextmenu.prevent="contextMenu"
      @wheel="onWheel"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
    >
      <svg class="pet-art" viewBox="0 0 96 96" aria-label="桌面珠子">
        <circle class="pet-progress-bg" cx="48" cy="52" r="25" />
        <circle class="pet-progress" cx="48" cy="52" r="25" />
        <g class="pet-creature">
          <path class="pet-antenna" d="M48 39.5V26.5" />
          <circle class="pet-tip" cx="48" cy="24" r="3.5" />
          <circle class="pet-body" cx="48" cy="52" r="16" />
          <PetLook :look="settings.pet.look" />
          <g class="pet-eyes">
            <circle v-if="!sleeping && !pinched && !blinking && action !== 'yawn'" cx="42.5" cy="50" r="2.25" />
            <circle v-if="!sleeping && !pinched && !blinking && action !== 'yawn'" cx="53.5" cy="50" r="2.25" />
            <path v-else d="M39 51q3-3 6 0M51 51q3-3 6 0" />
          </g>
          <text v-if="isSoon || action === 'startled'" class="pet-mark" x="57" y="28">!</text>
          <text v-if="sleeping" class="pet-mark" x="63" y="31">z</text>
          <path v-if="action === 'celebrate'" class="pet-spark" d="M67 31l1.5 3.2 3.2 1.5-3.2 1.5-1.5 3.2-1.5-3.2-3.2-1.5 3.2-1.5z" />
        </g>
      </svg>
      <span class="pet-shadow"></span>
      <span v-if="action === 'peek' && next" class="pet-peek">{{ clockTime(next.at) }} {{ next.title }}</span>
      <span v-if="wheelOffset" class="pet-wheel-tip">推后 {{ wheelOffset }} 分钟</span>
    </div>
    </div>
  </main>
</template>
