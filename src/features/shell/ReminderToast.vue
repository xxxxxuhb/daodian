<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { savePlan, saveTodo, setStageDone, setTodoDone } from "../../api/library";
import { useLibrary } from "../../data/useLibrary";
import { useNow } from "../../domain/clock";
import { clockTime, remindedHits, type RemindedHit } from "../../domain/present";
import { useRun } from "../../domain/useRun";
import Icon from "../../ui/Icon.vue";

const { todos, plans, loading, loadError, reload } = useLibrary();
const now = useNow();
const { saving, error, run } = useRun();
const queue = ref<RemindedHit[]>([]);
const current = computed(() => queue.value[0] ?? null);
const seen = new Set<string>();
let ready = false;
let unlisten: UnlistenFn | null = null;

interface ReminderEvent {
  key: string;
  title: string;
  detail: string;
  at: number;
  todoId?: string;
  planId?: string;
  stageId?: string;
}

function enqueue(hit: RemindedHit) {
  if (seen.has(hit.key)) return;
  seen.add(hit.key);
  queue.value = [...queue.value, hit];
}

onMounted(async () => {
  try {
    unlisten = await listen<ReminderEvent[]>("reminder-fired", ({ payload }) => {
      for (const item of payload) {
        enqueue({
          key: item.key,
          title: item.title,
          detail: item.detail,
          time: clockTime(item.at),
          todoId: item.todoId,
          planId: item.planId,
          stageId: item.stageId,
        });
      }
    });
  } catch {
    // 浏览器预览里没有 Tauri 事件。
  }
});

onBeforeUnmount(() => unlisten?.());

/* 珠子落盘：换一条提醒就重放一次 */
const landed = ref(false);
watch(
  () => current.value?.key,
  () => {
    landed.value = false;
    requestAnimationFrame(() => requestAnimationFrame(() => (landed.value = true)));
  },
  { immediate: true },
);

/* 稍后：把珠子拖到刻度上 */
const stops = [10, 30, 60];
const railEl = ref<HTMLElement | null>(null);
const knobX = ref(0);
const dragging = ref(false);
const nearest = ref(-1);

function label(minutes: number) {
  return minutes === 60 ? "1 小时" : `${minutes} 分钟`;
}

const hintText = computed(() => {
  const index = nearest.value;
  if (index < 0) return "松手不推后";
  const minutes = stops[index];
  return minutes == null ? "松手不推后" : `松手推后 ${label(minutes)}`;
});

function track(clientX: number) {
  const rail = railEl.value;
  if (!rail) return;
  const rect = rail.getBoundingClientRect();
  const x = Math.min(rect.width, Math.max(0, clientX - rect.left));
  knobX.value = x;
  const ratio = rect.width === 0 ? 0 : x / rect.width;
  nearest.value = ratio < 0.17 ? -1 : ratio < 0.5 ? 0 : ratio < 0.84 ? 1 : 2;
}

function onDown(event: PointerEvent) {
  dragging.value = true;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  track(event.clientX);
}

function onMove(event: PointerEvent) {
  if (!dragging.value) return;
  track(event.clientX);
}

function onUp() {
  if (!dragging.value) return;
  dragging.value = false;
  const index = nearest.value;
  nearest.value = -1;
  const minutes = index < 0 ? null : stops[index];
  if (minutes == null) {
    knobX.value = 0;
    return;
  }
  knobX.value = 0;
  snooze(minutes);
}

watch(
  () => [loading.value, loadError.value, todos.value, plans.value] as const,
  () => {
    if (loading.value || loadError.value) return;
    const hits = remindedHits(todos.value, plans.value, now.value);
    if (!ready) {
      for (const hit of hits) seen.add(hit.key);
      ready = true;
      return;
    }
    for (const hit of hits) {
      enqueue(hit);
    }
  },
  { deep: true, immediate: true },
);

function dismiss() {
  queue.value = queue.value.slice(1);
  error.value = "";
  knobX.value = 0;
}

async function complete() {
  const hit = current.value;
  if (!hit) return;
  const ok = await run(async () => {
    if (hit.todoId) await setTodoDone(hit.todoId, true);
    else if (hit.planId && hit.stageId) await setStageDone(hit.planId, hit.stageId, true);
    await reload();
  });
  if (ok) dismiss();
}

async function snooze(minutes: number) {
  const hit = current.value;
  if (!hit) return;
  const at = Date.now() + minutes * 60_000;
  const ok = await run(async () => {
    if (hit.todoId) {
      const todo = todos.value.find((item) => item.id === hit.todoId);
      if (!todo) return;
      await saveTodo({
        id: todo.id,
        title: todo.title,
        note: todo.note,
        dueAt: at,
        priority: todo.priority,
        repeat: todo.repeat,
        leadMs: todo.leadMs ?? 0,
      });
    } else if (hit.planId && hit.stageId) {
      const plan = plans.value.find((item) => item.id === hit.planId);
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
          at: stage.id === hit.stageId ? at : stage.at,
          done: stage.done,
        })),
      });
    }
    await reload();
  });
  if (ok) dismiss();
}
</script>

<template>
  <Teleport to="body">
    <div v-if="current" class="remind">
      <div class="remind__dial">
        <svg viewBox="0 0 64 64" fill="none" aria-hidden="true">
          <circle cx="32" cy="32" r="18.4" stroke="currentColor" stroke-width="4.8" />
          <path d="M32 32 40.5 23.5" stroke="currentColor" stroke-width="4.6" stroke-linecap="round" />
        </svg>
        <span class="remind__bead" :class="{ 'is-landed': landed }"></span>
        <span class="remind__ring"></span>
      </div>
      <div class="remind__main">
        <div class="remind__cap">
          <Icon name="bell" small />
          <span>到点</span>
          <time>{{ current.time }}</time>
          <button class="icon-btn" type="button" aria-label="关掉" @click="dismiss">
            <Icon name="x" small />
          </button>
        </div>
        <h3>{{ current.title }}</h3>
        <p class="remind__detail">{{ current.detail }}</p>
        <p v-if="error" class="banner">{{ error }}</p>
        <div class="remind__acts">
          <button class="btn btn--sm btn--primary" type="button" :disabled="saving" @click="complete">完成</button>
          <button class="btn btn--sm" type="button" :disabled="saving" @click="snooze(10)">稍后 10 分钟</button>
        </div>
        <div
          ref="railEl"
          class="remind__rail"
          @pointermove="onMove"
          @pointerup="onUp"
          @pointercancel="onUp"
        >
          <span class="remind__line"></span>
          <span
            v-for="(minutes, index) in stops"
            :key="minutes"
            class="remind__tick"
            :class="{ 'is-near': nearest === index }"
            :style="{ left: ((index + 1) / stops.length) * 100 + '%' }"
          >
            <b>{{ label(minutes) }}</b>
          </span>
          <button
            class="remind__knob"
            type="button"
            :style="{ left: knobX + 'px' }"
            :aria-label="'拖到刻度上推后，当前 ' + (nearest < 0 ? '不推后' : label(stops[nearest] ?? 10))"
            @pointerdown="onDown"
            @keydown.enter.prevent="snooze(10)"
          ></button>
        </div>
        <p class="remind__hint">{{ dragging ? hintText : "拖珠子到刻度上推后" }}</p>
      </div>
    </div>
  </Teleport>
</template>
