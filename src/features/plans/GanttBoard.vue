<script setup lang="ts">
import { computed, ref } from "vue";
import { axisRange, axisRatio, dayHead } from "../../domain/present";
import { startOfDay } from "../../domain/time";
import type { Plan, Stage } from "../../domain/types";

const props = defineProps<{ plans: Plan[]; now: number; mode: "week" | "month" }>();
const emit = defineEmits<{ open: [string]; shift: [Plan, { id: string; at: number }[]] }>();
const dragging = ref("");

const preview = ref<Record<string, number>>({});
const range = computed(() => axisRange(props.now, props.mode));
const todayIndex = computed(() => range.value.days.findIndex((day) => day === startOfDay(props.now)));
const nowRatio = computed(() => axisRatio(props.now, range.value.start, range.value.end));
const monthLabel = computed(() => `${new Date(range.value.start).getMonth() + 1}月`);

function atOf(stage: Stage) {
  return preview.value[stage.id] ?? stage.at;
}

function segment(plan: Plan) {
  const times = plan.stages.map(atOf).filter((at): at is number => at != null);
  if (times.length < 2) return null;
  const left = Math.min(1, Math.max(0, axisRatio(Math.min(...times), range.value.start, range.value.end)));
  const right = Math.min(1, Math.max(0, axisRatio(Math.max(...times), range.value.start, range.value.end)));
  if (right - left < 0.004) return null;
  return { left: `${left * 100}%`, width: `${(right - left) * 100}%` };
}

function pins(plan: Plan) {
  const next = plan.stages.find((stage) => !stage.done && stage.at != null);
  return plan.stages.flatMap((stage) => {
    const at = atOf(stage);
    if (at == null) return [];
    const ratio = axisRatio(at, range.value.start, range.value.end);
    if (ratio < 0 || ratio > 1) return [];
    const kind = stage.done || plan.done ? "done" : next?.id === stage.id ? "next" : "";
    return [{ id: stage.id, left: `${ratio * 100}%`, title: stage.title, kind, stage }];
  });
}

function caption(plan: Plan) {
  const done = plan.stages.filter((stage) => stage.done).length;
  if (plan.stages.length === 0) return "还没分阶段";
  if (done === 0) return `${plan.stages.length} 个阶段`;
  return `${plan.stages.length} 个阶段 · ${done} 完成`;
}

function onPinDown(event: PointerEvent, plan: Plan, stage: Stage) {
  const pin = event.currentTarget as HTMLElement;
  const track = pin.closest(".gantt__track") as HTMLElement | null;
  const origin = stage.at;
  if (!track || origin == null) return;
  event.preventDefault();
  event.stopPropagation();
  const startX = event.clientX;
  let moved = false;
  dragging.value = stage.id;
  const index = plan.stages.findIndex((item) => item.id === stage.id);
  const followers = plan.stages.slice(index + 1).filter((item) => !item.done && item.at != null);
  const bounds = axisRange(props.now, props.mode);
  const move = (ev: PointerEvent) => {
    if (Math.abs(ev.clientX - startX) > 4) moved = true;
    const rect = track.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (ev.clientX - rect.left) / Math.max(rect.width, 1)));
    const raw = bounds.start + ratio * (bounds.end - bounds.start);
    const snapped = Math.round(raw / (15 * 60_000)) * 15 * 60_000;
    const delta = snapped - origin;
    const next: Record<string, number> = { [stage.id]: snapped };
    for (const item of followers) next[item.id] = (item.at ?? 0) + delta;
    preview.value = next;
  };
  const up = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    const changes = Object.entries(preview.value).map(([id, at]) => ({ id, at }));
    const next = preview.value[stage.id];
    preview.value = {};
    dragging.value = "";
    if (moved && next != null && Math.abs(next - origin) >= 60_000) emit("shift", plan, changes);
    else if (!moved) emit("open", plan.id);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}
</script>

<template>
  <div class="gantt-wrap">
  <div
    class="gantt"
    :class="{ 'is-month': mode === 'month', 'no-today': todayIndex < 0 }"
    :style="{ '--cols': range.days.length, '--today': Math.max(todayIndex, 0) }"
  >
    <div class="gantt__head">
      <div>计划 / {{ monthLabel }}</div>
      <div v-for="day in range.days" :key="day" :class="{ today: day === startOfDay(now) }">{{ dayHead(day) }}</div>
    </div>
    <div class="gantt__body" :style="{ '--now': nowRatio }">
      <span v-if="nowRatio >= 0 && nowRatio <= 1" class="gantt__now"></span>
      <div v-for="plan in plans" :key="plan.id" class="gantt__row">
        <button class="gantt__label" type="button" @click="emit('open', plan.id)">
          {{ plan.title }}
          <span>{{ caption(plan) }}</span>
        </button>
        <div class="gantt__track">
          <span
            v-if="segment(plan)"
            class="gantt__seg"
            :style="{ left: segment(plan)?.left, width: segment(plan)?.width }"
          ></span>
          <span
            v-for="pin in pins(plan)"
            :key="pin.id"
            class="pin"
            :class="[pin.kind, preview[pin.id] != null && pin.id !== dragging ? 'is-preview' : '']"
            :style="{ left: pin.left }"
            @pointerdown="onPinDown($event, plan, pin.stage)"
          >
            <span>{{ pin.title }}</span>
            <i></i>
          </span>
        </div>
      </div>
      <p v-if="plans.length === 0" class="empty">这条时间线上还没有计划</p>
    </div>
  </div>
  <div class="legend">
    <span><i style="background: var(--done)"></i>已完成</span>
    <span><i style="background: var(--accent)"></i>下一个到点</span>
    <span><i style="background: var(--fill-2)"></i>还没到</span>
    <span style="margin-left: auto; color: var(--ink-4)">拖动节点可以直接改时间</span>
  </div>
  <p class="foot-note">每个点都是一个阶段时间；细线是计划从第一个阶段到最后一个阶段的跨度。</p>
  </div>
</template>
