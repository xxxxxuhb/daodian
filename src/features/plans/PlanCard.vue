<script setup lang="ts">
import { computed } from "vue";
import { planPill, relative, stageWhen } from "../../domain/present";
import { isContainerShortcut } from "../../domain/shortcuts";
import type { Plan, Stage } from "../../domain/types";
import { stageProgress } from "./logic";
import Icon from "../../ui/Icon.vue";

const props = defineProps<{ plan: Plan; now: number }>();
const emit = defineEmits<{
  open: [];
  fill: [];
  toggle: [];
  toggleStage: [Stage];
  slip: [number];
  saveTemplate: [];
}>();

function slipPreview(days: number) {
  const open = props.plan.stages.filter((stage) => !stage.done && stage.at != null);
  if (!open.length) return "没有未完成的时间";
  const at = Math.min(...open.map((stage) => stage.at ?? 0)) + days * 86_400_000;
  const date = new Date(at);
  return `${open.length} 个阶段 → ${date.getMonth() + 1}月${date.getDate()}日`;
}

const progress = computed(() => stageProgress(props.plan));
const pill = computed(() => planPill(props.plan, props.now));
const nextId = computed(() => props.plan.stages.find((stage) => !stage.done)?.id ?? "");

function slipFromKey(event: KeyboardEvent) {
  if (!isContainerShortcut(event, "t", Boolean(document.querySelector('[role="dialog"]')))) return;
  event.preventDefault();
  emit("slip", 1);
}
</script>

<template>
  <article class="plan" tabindex="0" @click="emit('open')" @keydown.enter.prevent="emit('open')" @keydown="slipFromKey">
    <div class="plan__head">
      <button
        class="check"
        :class="{ 'is-done': plan.done }"
        type="button"
        :aria-label="plan.done ? '标为未完成' : '整份标为完成'"
        @click.stop="emit('toggle')"
      >
        <Icon v-if="plan.done" name="check" small />
      </button>
      <div class="plan__title">
        <h3>
          <i v-if="plan.priority === 'high'" class="dotpin"></i>
          <i v-else-if="plan.priority === 'low'" class="dotpin low"></i>
          {{ plan.title }}
        </h3>
        <p v-if="plan.note" class="plan__note--desktop">{{ plan.note }}</p>
      </div>
      <span class="pill" :class="`pill--${pill.tone}`">{{ pill.text }}</span>
      <div class="plan__more" @click.stop>
        <button type="button" class="icon-btn" aria-label="更多">⋯</button>
        <div class="plan__menu">
          <p>整体顺延</p>
          <button type="button" @click="emit('slip', 1)">1 天 <span>{{ slipPreview(1) }}</span></button>
          <button type="button" @click="emit('slip', 3)">3 天 <span>{{ slipPreview(3) }}</span></button>
          <button type="button" @click="emit('slip', 7)">1 周 <span>{{ slipPreview(7) }}</span></button>
          <p>模板</p>
          <button type="button" @click="emit('saveTemplate')">另存为模板 <span>{{ plan.stages.length }} 个阶段</span></button>
        </div>
      </div>
      <div v-if="progress.total" class="plan__prog">
        <span class="frac">{{ progress.done }}/{{ progress.total }}</span>
        <span class="segs"><i v-for="stage in plan.stages" :key="stage.id" :class="{ on: stage.done }"></i></span>
      </div>
    </div>
    <div class="plan__meta">
      <span class="plan__note">{{ plan.note || "还没写备注" }}</span>
      <span class="plan__fraction">{{ progress.done }}/{{ progress.total }} 阶段</span>
    </div>
    <div v-if="progress.total" class="plan__bar" aria-hidden="true">
      <i :style="{ width: `${Math.round((progress.done / progress.total) * 100)}%` }"></i>
    </div>
    <ol v-if="plan.stages.length" class="stepper">
      <li v-for="stage in plan.stages" :key="stage.id" :class="{ done: stage.done, next: stage.id === nextId }">
        <button
          class="dot"
          type="button"
          :aria-label="stage.done ? '标为未完成' : '标为完成'"
          @click.stop="emit('toggleStage', stage)"
        ></button>
        <span class="tm">{{ stageWhen(stage.at, now) }}</span>
        <span class="nm">{{ stage.title }}</span>
        <span class="side">
          <span v-if="!stage.done && stage.at != null" class="pill" :class="`pill--${relative(stage.at, now).tone}`">
            {{ relative(stage.at, now).text }}
          </span>
        </span>
      </li>
    </ol>
    <button v-else class="empty-stage" type="button" @click.stop="emit('fill')">
      <Icon name="plus" small />
      <span class="empty-stage__desktop">分阶段：议程 → 试跑 → 定稿</span>
      <span class="empty-stage__mini">还没分阶段 · 点开加「议程 → 试跑 → 定稿」</span>
    </button>
  </article>
</template>
