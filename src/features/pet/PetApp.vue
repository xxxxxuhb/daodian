<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { savePlan, saveTodo, setStageDone, setTodoDone } from "../../api/library";
import { useLibrary } from "../../data/useLibrary";
import { useSettings } from "../../data/useSettings";
import { useNow } from "../../domain/clock";
import { clockTime, relative, upcoming } from "../../domain/present";
import type { Settings } from "../../domain/settings";

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
  | "celebrate";

const COMMON_ACTIONS: PetAction[] = ["tilt", "yawn", "antenna", "tap"];
const RARE_ACTIONS: PetAction[] = ["stretch", "roll", "peek", "startled"];
const RARE_COOLDOWN = 3 * 60_000;

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
const lookX = ref(0);
const lookY = ref(-1);
const moving = ref(false);
const blinking = ref(false);
const action = ref<PetAction | "">("");
let unlistenData: UnlistenFn | null = null;
let unlistenReminder: UnlistenFn | null = null;
let unlistenSettings: UnlistenFn | null = null;
let clickTimer = 0;
let holdTimer = 0;
let down: { clientX: number; clientY: number; screenX: number; screenY: number } | null = null;
let frame = 0;
let pendingMove: { x: number; y: number } | null = null;
let moveChain: Promise<unknown> = Promise.resolve();
let randomTimer = 0;
let blinkTimer = 0;
let actionTimer = 0;
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
const sleeping = computed(() => {
  const hour = new Date(now.value).getHours();
  return hour >= 22 || hour < 8;
});
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
const state = computed(() =>
  sleeping.value ? "sleep" : isDue.value ? "due" : isSoon.value ? "soon" : "idle",
);
const bubble = computed(() => {
  if (!item.value) return "今天没有安排，歇一会儿";
  const when = relative(item.value.at, now.value).text;
  return `下一个 ${clockTime(item.value.at)} ${item.value.title} · ${when}`;
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
const petStyle = computed(() => ({
  "--look-x": `${lookX.value * 2.2}px`,
  "--look-y": `${lookY.value * 1.4}px`,
  "--antenna-angle": `${Math.atan2(lookY.value, lookX.value) * (180 / Math.PI) + 90}deg`,
  "--ring-offset": `${157 * (1 - ringProgress.value)}`,
}));

watch(mode, (value) => {
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
    !sleeping.value
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
  playAction(selected, selected === "peek" ? 2_000 : 1_200);
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

function pointerLook(event: MouseEvent) {
  const target = event.currentTarget as HTMLElement;
  const rect = target.getBoundingClientRect();
  const x = event.clientX - (rect.left + rect.width / 2);
  const y = event.clientY - (rect.top + rect.height / 2);
  const length = Math.max(1, Math.hypot(x, y));
  lookX.value = x / length;
  lookY.value = y / length;
}

function leavePet() {
  hovered.value = false;
  lookX.value = 0;
  lookY.value = -1;
}

function enterPet() {
  hovered.value = true;
  interruptAction();
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
  window.clearTimeout(clickTimer);
  clickTimer = window.setTimeout(() => {
    panel.value = panel.value === "card" ? "none" : "card";
  }, 220);
}

function onDoubleClick() {
  interruptAction();
  window.clearTimeout(clickTimer);
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
  pointerLook(event);
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
    await invoke("pet_snap").catch(() => {});
    window.setTimeout(() => (moving.value = false), 80);
  }
  if (pinched.value) window.setTimeout(() => (pinched.value = false), 180);
}

async function complete() {
  const target = item.value;
  if (!target) return;
  if (target.todoId) await setTodoDone(target.todoId, true);
  else if (target.planId && target.stageId) {
    await setStageDone(target.planId, target.stageId, true);
  }
  activeReminder.value = null;
  panel.value = "none";
  await reload();
}

async function snoozeTo(at: number) {
  const target = item.value;
  if (!target) return;
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
  const [snapshot] = await Promise.all([
    invoke<PetSnapshot>("pet_snapshot").catch(() => ({ pinches: 0, hiddenUntil: 0 })),
    reload(),
    loadSettings(),
  ]);
  pinches.value = snapshot.pinches;
  completionBaseline = completedCount.value;
  document.documentElement.dataset.theme = settings.value.theme;
  scheduleBlink();
  scheduleRandomAction();
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
});

onBeforeUnmount(() => {
  window.removeEventListener("blur", closePanelOnBlur);
  unlistenData?.();
  unlistenReminder?.();
  unlistenSettings?.();
  window.clearTimeout(clickTimer);
  window.clearTimeout(holdTimer);
  window.clearTimeout(randomTimer);
  window.clearTimeout(blinkTimer);
  window.clearTimeout(actionTimer);
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
      { 'is-pressed': pressed, 'is-pinched': pinched, 'is-blinking': blinking },
    ]"
    :style="petStyle"
    @mouseleave="leavePet"
  >
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
        <button type="button" class="pet-card__close" @click="panel = 'none'">×</button>
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

    <div v-if="mode === 'hover' && action !== 'peek'" class="pet-bubble">{{ bubble }}</div>
    <div
      class="pet-anchor"
      @mouseenter="enterPet"
      @mousemove="pointerLook"
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
      <span v-if="action === 'peek' && item" class="pet-peek">{{ clockTime(item.at) }} {{ item.title }}</span>
      <span v-if="wheelOffset" class="pet-wheel-tip">推后 {{ wheelOffset }} 分钟</span>
    </div>
  </main>
</template>
