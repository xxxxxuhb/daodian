<script setup lang="ts">
import { computed, inject, ref, watch } from "vue";
import { deletePlan, savePlan, setPlanDone, setStageDone } from "../../api/library";
import { useLibrary } from "../../data/useLibrary";
import { useNow } from "../../domain/clock";
import {
  axisRange,
  clockFace,
  clockTime,
  countdown,
  dayLabel,
  relative,
  stageLong,
  upcoming,
  weekBars,
  weekStats,
} from "../../domain/present";
import { useRun } from "../../domain/useRun";
import type { Plan, Stage } from "../../domain/types";
import Icon from "../../ui/Icon.vue";
import { createTickKey } from "../shell/create";
import TodayStrip from "../shell/TodayStrip.vue";
import { tierKey } from "../shell/tier";
import GanttBoard from "./GanttBoard.vue";
import {
  adoptSavedStages,
  comparePlans,
  emptyPlanCopy,
  emptyPlanDraft,
  emptyStage,
  matchesPlan,
  PLAN_TEMPLATES,
  stagesFromTemplate,
  PLAN_FILTERS,
  planMatchesQuery,
  stagesForSave,
  toPlanDraft,
  type PlanDraft,
  type PlanFilter,
} from "./logic";
import PlanCard from "./PlanCard.vue";
import PlanEditor from "./PlanEditor.vue";

const props = defineProps<{ filter: PlanFilter; query: string; loadError: string }>();

const now = useNow();
const { todos, plans, loading, reload } = useLibrary();
const { saving, error, run } = useRun();
const face = computed(() => clockFace(now.value));
const view = ref<"cards" | "gantt">("cards");
const span = ref<"week" | "month">("week");
const important = ref(false);
const showAll = ref(false);
const name = ref("");
const templateId = ref("release");
const undo = ref<{ plan: Plan; timer: number } | null>(null);
const composer = ref<HTMLInputElement | null>(null);
const draft = ref<PlanDraft | null>(null);
const doneStamp = ref("");
const createTick = inject(createTickKey, null);
const tier = inject(tierKey, computed(() => "wide" as const));

const shown = computed(() =>
  plans.value
    .filter((plan) => showAll.value || matchesPlan(plan, props.filter))
    .filter((plan) => !important.value || plan.priority === "high")
    .filter((plan) => planMatchesQuery(plan, props.query))
    .sort(comparePlans),
);
const heading = computed(() => {
  if (view.value === "gantt") {
    return { label: "时间轴", hint: "七个计划排在同一条线上，谁先到点一眼看得出来" };
  }
  return PLAN_FILTERS.find((item) => item.id === props.filter) ?? PLAN_FILTERS[0]!;
});
const stageCount = computed(() => shown.value.reduce((sum, plan) => sum + plan.stages.length, 0));
const nextStageAt = computed(() => {
  let best: number | null = null;
  for (const plan of shown.value) {
    if (plan.done) continue;
    for (const stage of plan.stages) {
      if (stage.done || stage.at == null || stage.at < now.value) continue;
      if (best == null || stage.at < best) best = stage.at;
    }
  }
  return best;
});
const bars = computed(() => weekBars(plans.value, now.value));
const stats = computed(() => weekStats(plans.value, now.value));
const running = computed(() => plans.value.filter((plan) => !plan.done).length);
const next = computed(() => upcoming(todos.value, plans.value, now.value));
const axis = computed(() => axisRange(now.value, span.value));
const nodes = computed(() => {
  const items: { key: string; at: number; time: string; title: string; meta: string }[] = [];
  for (const plan of shown.value) {
    if (plan.done) continue;
    for (const stage of plan.stages) {
      if (stage.done || stage.at == null || stage.at < axis.value.start || stage.at >= axis.value.end) continue;
      items.push({
        key: stage.id,
        at: stage.at,
        time: clockTime(stage.at),
        title: stage.title,
        meta: `${plan.title} · ${dayLabel(stage.at, now.value)}`,
      });
    }
  }
  return items.sort((a, b) => a.at - b.at).slice(0, 8);
});
const rangeStats = computed(() => {
  let total = 0;
  let done = 0;
  for (const plan of shown.value) {
    for (const stage of plan.stages) {
      if (stage.at == null || stage.at < axis.value.start || stage.at >= axis.value.end) continue;
      total += 1;
      if (stage.done || plan.done) done += 1;
    }
  }
  return { total, done, ratio: total === 0 ? 0 : done / total };
});

function stamp(plan: Plan) {
  return `${plan.done}|${plan.stages.map((stage) => `${stage.id}:${stage.done}`).join()}`;
}

watch(plans, () => {
  const current = draft.value;
  if (!current?.id) return;
  const live = plans.value.find((plan) => plan.id === current.id);
  if (!live) {
    draft.value = null;
    return;
  }
  const nextStamp = stamp(live);
  if (nextStamp === doneStamp.value) return;
  doneStamp.value = nextStamp;
  draft.value = adoptSavedStages(current, live);
});

watch(
  () => createTick?.value,
  () => {
    if (draft.value) return;
    if (view.value === "gantt") view.value = "cards";
    composer.value?.focus();
  },
);

function openPlan(id: string, preset?: string[]) {
  const plan = plans.value.find((item) => item.id === id);
  if (!plan) return;
  const nextDraft = toPlanDraft(plan);
  if (preset && nextDraft.stages.length === 0) {
    nextDraft.stages = preset.map((title) => ({ ...emptyStage(), title }));
  }
  draft.value = nextDraft;
  doneStamp.value = stamp(plan);
}

async function commit() {
  const title = name.value.trim();
  if (!title) {
    error.value = "先写个名字";
    composer.value?.focus();
    return;
  }
  const stages = stagesFromTemplate(templateId.value, now.value);
  const ok = await run(async () => {
    const saved = await savePlan({
      id: null,
      title,
      note: "",
      priority: "normal",
      templateId: templateId.value === "blank" ? null : templateId.value,
      stages,
    });
    await reload();
    draft.value = toPlanDraft(saved);
    doneStamp.value = stamp(saved);
  });
  if (!ok) return;
  name.value = "";
  templateId.value = "release";
}

function openStages(plan: Plan) {
  return plan.stages.filter((stage) => !stage.done && stage.at != null);
}

async function slip(plan: Plan, days: number) {
  const stages = plan.stages.map((stage) => ({
    id: stage.id,
    title: stage.title,
    done: stage.done,
    at: stage.done || stage.at == null ? stage.at : stage.at + days * 86_400_000,
  }));
  const ok = await run(async () => {
    await savePlan({
      id: plan.id,
      title: plan.title,
      note: plan.note,
      priority: plan.priority,
      templateId: plan.templateId,
      stages,
    });
    await reload();
  });
  if (!ok) return;
  if (undo.value) window.clearTimeout(undo.value.timer);
  undo.value = {
    plan,
    timer: window.setTimeout(() => {
      undo.value = null;
    }, 5000),
  };
}

async function undoSlip() {
  const item = undo.value;
  if (!item) return;
  window.clearTimeout(item.timer);
  undo.value = null;
  await run(async () => {
    await savePlan({
      id: item.plan.id,
      title: item.plan.title,
      note: item.plan.note,
      priority: item.plan.priority,
      templateId: item.plan.templateId,
      stages: item.plan.stages.map((stage) => ({
        id: stage.id,
        title: stage.title,
        at: stage.at,
        done: stage.done,
      })),
    });
    await reload();
  });
}

async function togglePlan(plan: Plan) {
  await run(async () => {
    await setPlanDone(plan.id, !plan.done);
    await reload();
  });
}

async function toggleStage(plan: Plan, stage: Stage) {
  await run(async () => {
    await setStageDone(plan.id, stage.id, !stage.done);
    await reload();
  });
}

async function shift(plan: Plan, changes: { id: string; at: number }[]) {
  const next = new Map(changes.map((item) => [item.id, item.at]));
  await run(async () => {
    await savePlan({
      id: plan.id,
      title: plan.title,
      note: plan.note,
      priority: plan.priority,
      templateId: plan.templateId,
      stages: plan.stages.map((stage) => ({
        id: stage.id,
        title: stage.title,
        at: next.get(stage.id) ?? stage.at,
        done: stage.done,
      })),
    });
    await reload();
  });
}

async function saveDraft() {
  const current = draft.value;
  if (!current) return;
  const stages = stagesForSave(current.stages);
  if (typeof stages === "string") {
    error.value = stages;
    return;
  }
  const ok = await run(async () => {
    await savePlan({
      id: current.id,
      title: current.title,
      note: current.note,
      priority: current.priority,
      templateId: current.templateId,
      stages,
    });
    await reload();
  });
  if (ok) draft.value = null;
}

async function finishDraft() {
  const current = draft.value;
  if (!current?.id) return;
  const ok = await run(async () => {
    await setPlanDone(current.id!, !current.done);
    await reload();
  });
  if (ok) draft.value = null;
}

async function removeDraft() {
  const id = draft.value?.id;
  if (!id) return;
  const ok = await run(async () => {
    await deletePlan(id);
    await reload();
  });
  if (ok) draft.value = null;
}
</script>

<template>
  <main :class="tier === 'mini' ? 'mini-body' : tier === 'standard' ? 'std-main' : 'main'">
    <header v-if="tier !== 'mini'" :class="tier === 'standard' ? 'std-head' : 'topbar'">
      <div>
        <p class="eyebrow">计划</p>
        <h1>{{ heading.label }}</h1>
        <p v-if="tier === 'wide'" class="sub">{{ heading.hint }}</p>
      </div>
      <div :class="tier === 'standard' ? 'right' : 'topbar__right'">
        <div class="date"><b>{{ face.date }}</b>{{ face.rest }}</div>
        <div class="seg">
          <button type="button" :class="{ on: view === 'cards' }" @click="view = 'cards'">卡片</button>
          <button type="button" :class="{ on: view === 'gantt' }" @click="view = 'gantt'">
            <Icon name="timeline" small />时间轴
          </button>
        </div>
        <button class="btn btn--primary" :class="{ 'btn--sm': tier === 'standard' }" type="button" @click="draft = emptyPlanDraft()">
          <Icon name="plus" small />新建
        </button>
      </div>
    </header>
    <div v-else class="mini-title">
      <h1>{{ heading.label }}</h1>
      <span class="today-fig">{{ running }}</span>
      <span class="grow"></span>
      <span>{{ clockTime(now) }}</span>
    </div>
    <TodayStrip v-if="tier === 'standard'" />

    <div v-if="tier === 'mini' && view === 'cards'" class="mini-add">
      <div class="mini-add__line">
        <span class="mini-add__dot"></span>
        <input ref="composer" v-model="name" maxlength="80" placeholder="先写个名字…" @keydown.enter.prevent="commit" />
        <button class="mini-add__go" type="button" :disabled="saving" @click="commit"><Icon name="plus" /></button>
      </div>
      <div class="mini-add__row">
        <button
          v-for="item in PLAN_TEMPLATES"
          :key="item.id"
          type="button"
          class="chip"
          :class="{ on: templateId === item.id }"
          @click="templateId = item.id"
        >
          {{ item.label }}
        </button>
      </div>
    </div>

    <div v-if="tier === 'mini'" class="toolbar mini-toolbar">
      <button type="button" class="chip" :class="{ on: span === 'week' }" @click="span = 'week'">本周</button>
      <button type="button" class="chip" :class="{ on: span === 'month' }" @click="span = 'month'">本月</button>
      <span class="grow"></span>
      <button type="button" class="chip" :class="{ on: showAll }" @click="showAll = !showAll">全部计划</button>
      <button type="button" class="chip" :class="{ on: important }" @click="important = !important">只看重要</button>
    </div>

    <div :class="tier === 'mini' ? 'mini-scroll' : 'main__scroll'">
      <p v-if="error || loadError" class="banner">{{ error || loadError }}</p>

      <div v-if="view === 'cards' && tier !== 'mini'" class="composer">
        <div class="composer__line">
          <span class="composer__dot"></span>
          <input
            ref="composer"
            v-model="name"
            maxlength="80"
            placeholder="先写个名字，再拆阶段，例如「1.2 版本」"
            @keydown.enter.prevent="commit"
          />
          <span class="kbd">Ctrl N</span>
        </div>
        <div class="composer__row">
          <button
            v-for="item in PLAN_TEMPLATES"
            :key="item.id"
            type="button"
            class="chip"
            :class="{ on: templateId === item.id }"
            @click="templateId = item.id"
          >
            {{ item.label }}
          </button>
          <span class="grow"></span>
          <button class="btn btn--primary btn--sm" type="button" :disabled="saving" @click="commit">记下</button>
        </div>
      </div>

      <div v-else-if="tier !== 'mini'" class="toolbar">
        <button type="button" class="chip" :class="{ on: span === 'week' }" @click="span = 'week'">本周</button>
        <button type="button" class="chip" :class="{ on: span === 'month' }" @click="span = 'month'">本月</button>
        <span class="grow"></span>
        <button type="button" class="chip" :class="{ on: showAll }" @click="showAll = !showAll">全部计划</button>
        <button type="button" class="chip" :class="{ on: important }" @click="important = !important">只看重要</button>
      </div>

      <p v-if="loading && shown.length === 0" class="empty">正在读取…</p>
      <p v-else-if="view === 'cards' && shown.length === 0" class="empty">{{ emptyPlanCopy(filter, Boolean(query)) }}</p>
      <template v-else-if="view === 'cards'">
        <PlanCard
          v-for="plan in shown"
          :key="plan.id"
          :plan="plan"
          :now="now"
          @open="openPlan(plan.id)"
          @fill="openPlan(plan.id, ['议程', '试跑', '定稿'])"
          @toggle="togglePlan(plan)"
          @toggle-stage="toggleStage(plan, $event)"
          @slip="slip(plan, $event)"
        />
      </template>
      <GanttBoard v-else :plans="shown" :now="now" :mode="span" @open="openPlan" @shift="shift" />
    </div>

    <button v-if="undo" type="button" class="undo" @click="undoSlip">
      已顺延 {{ openStages(undo.plan).length }} 个阶段
      <b>撤销</b>
    </button>

    <div v-if="tier !== 'mini'" class="main-foot">
      <span>
        <b>{{ shown.length }}</b> 份计划 · {{ stageCount }} 个阶段
        <template v-if="nextStageAt">，下一个 {{ relative(nextStageAt, now).text }}</template>
      </span>
      <span class="k">
        <span><span class="kbd">Enter</span>打开</span>
        <span><span class="kbd">Ctrl N</span>新建</span>
      </span>
    </div>
    <PlanEditor
      v-if="draft"
      :draft="draft"
      :now="now"
      :saving="saving"
      :error="error"
      @close="draft = null"
      @save="saveDraft"
      @remove="removeDraft"
      @toggle="finishDraft"
    />
  </main>

  <aside v-if="tier === 'wide'" class="aside">
    <template v-if="view === 'cards'">
      <div class="aside__head">
        <p class="eyebrow">本周</p>
        <div class="big"><b>{{ running }}</b><span>份计划在跑</span></div>
        <div class="bars">
          <i v-for="(bar, index) in bars" :key="index" :class="{ on: bar.on, moss: bar.moss }" :style="{ height: `${bar.height}%` }"></i>
        </div>
        <div class="bar-labels">
          <span v-for="(bar, index) in bars" :key="index" :class="{ on: bar.on }">{{ bar.label }}</span>
        </div>
        <div class="stat-grid">
          <div class="stat"><b>{{ stats.total }}</b><span>本周到点</span></div>
          <div class="stat"><b>{{ stats.done }}</b><span>已完成</span></div>
        </div>
      </div>
      <div class="next-card" style="margin-top: auto">
        <template v-if="next">
          <p class="eyebrow">下一阶段</p>
          <h4>{{ next.title }}</h4>
          <p v-if="next.kind === 'stage'">{{ next.context }} · 阶段 {{ next.index }} / {{ next.total }}</p>
          <p v-else>待办 · {{ stageLong(next.at, now) }}</p>
          <div class="cd"><b>{{ countdown(next.at, now) }}</b><span>后到点</span></div>
        </template>
        <template v-else>
          <p class="eyebrow">下一阶段</p>
          <h4>还没有下一个阶段</h4>
          <p>给计划补上时间点之后会出现在这里</p>
        </template>
      </div>
    </template>
    <template v-else>
      <div class="aside__head">
        <p class="eyebrow">{{ span === "week" ? "本周节点" : "本月节点" }}</p>
        <div class="big"><b>{{ rangeStats.total }}</b><span>个到点</span></div>
        <div class="bar"><i :style="{ width: `${Math.round(rangeStats.ratio * 100)}%`, background: 'var(--accent)' }"></i></div>
      </div>
      <div class="tl">
        <div v-for="(item, index) in nodes" :key="item.key" class="tl__item" :class="{ 'is-key': index === 0 }">
          <i class="tl__t">{{ item.time }}</i>
          <span class="tl__dot"></span>
          <span><span class="nm">{{ item.title }}</span><span class="meta">{{ item.meta }}</span></span>
        </div>
        <p v-if="nodes.length === 0" class="empty">这段时间还没有阶段</p>
      </div>
      <div class="next-card" style="margin-top: auto">
        <template v-if="next">
          <p class="eyebrow">最近一个</p>
          <h4>{{ next.kind === "stage" ? `${next.title} · ${next.context}` : next.title }}</h4>
          <p>{{ stageLong(next.at, now) }}</p>
          <div class="cd"><b>{{ countdown(next.at, now) }}</b><span>后到点</span></div>
        </template>
        <template v-else>
          <p class="eyebrow">最近一个</p>
          <h4>没有即将到点的事</h4>
          <p>阶段排上时间之后会显示倒计时</p>
        </template>
      </div>
    </template>
  </aside>
</template>
