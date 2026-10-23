<script setup lang="ts">
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { computed, inject, onMounted, onUnmounted, ref, watch } from "vue";
import { generatePlan } from "../../api/ai";
import {
  deletePlan,
  deleteTemplate,
  renameTemplate,
  savePlan,
  saveTemplate,
  setPlanDone,
  setStageDone,
  setTemplateQuick,
} from "../../api/library";
import { useLibrary } from "../../data/useLibrary";
import { useSettings } from "../../data/useSettings";
import { useTemplates } from "../../data/useTemplates";
import { useNow } from "../../domain/clock";
import { buildPlanCalendar, planCalendarFileName } from "../../domain/ics";
import { generatedPlanFields } from "../../domain/ai";
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
import type { Plan, PlanTemplate, Stage } from "../../domain/types";
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
  newStageKey,
  stagesFromTemplate,
  templatePreview,
  templateStagesFromPlan,
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
const { templates, templateError, reloadTemplates } = useTemplates();
const { settings } = useSettings();
const { saving, error, run } = useRun();
const face = computed(() => clockFace(now.value));
const view = ref<"cards" | "gantt">("cards");
const span = ref<"week" | "month">("week");
const important = ref(false);
const showAll = ref(false);
const name = ref("");
const templateId = ref("blank");
const undo = ref<{ plan: Plan; timer: number } | null>(null);
const composer = ref<HTMLInputElement | null>(null);
const draft = ref<PlanDraft | null>(null);
const templateSource = ref<Pick<Plan, "title" | "stages"> | PlanDraft | null>(null);
const templateName = ref("");
const managingTemplates = ref(false);
const aiText = ref("");
const aiGenerating = ref(false);
const aiError = ref("");
let aiRequest = 0;
const templateNames = ref<Record<string, string>>({});
const doneStamp = ref("");
const createTick = inject(createTickKey, null);
const tier = inject(tierKey, computed(() => "wide" as const));
const selectedTemplate = computed(
  () => templates.value.find((item) => item.id === templateId.value) ?? templates.value[0],
);
const customTemplates = computed(() => templates.value.filter((item) => !item.builtIn));
const quickTemplates = computed(() => templates.value.filter((item) => item.quick));

onMounted(reloadTemplates);
onUnmounted(() => {
  aiRequest += 1;
});

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
  const template = selectedTemplate.value;
  if (!template) {
    error.value = "模板还没有加载好";
    return;
  }
  const stages = stagesFromTemplate(template, now.value);
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
  templateId.value = "blank";
}

async function generateDraft() {
  const text = aiText.value.trim();
  if (!text || aiGenerating.value) return;
  const request = ++aiRequest;
  aiGenerating.value = true;
  aiError.value = "";
  try {
    const generated = await generatePlan(text);
    if (request !== aiRequest) return;
    const fields = generatedPlanFields(generated, newStageKey);
    draft.value = { ...emptyPlanDraft(), ...fields };
    doneStamp.value = "";
  } catch (cause) {
    if (request === aiRequest) {
      aiError.value = cause instanceof Error ? cause.message : String(cause);
    }
  } finally {
    if (request === aiRequest) aiGenerating.value = false;
  }
}

function beginSaveTemplate(source: Pick<Plan, "title" | "stages"> | PlanDraft) {
  templateSource.value = source;
  templateName.value = source.title.trim() || "新模板";
}

async function confirmSaveTemplate() {
  const source = templateSource.value;
  if (!source) return;
  const label = templateName.value.trim();
  if (!label) {
    error.value = "模板要有个名字";
    return;
  }
  const ok = await run(async () => {
    const saved = await saveTemplate({
      id: null,
      label,
      stages: templateStagesFromPlan(source, now.value),
    });
    await reloadTemplates();
    templateId.value = saved.id;
  });
  if (ok) templateSource.value = null;
}

function openTemplateManager() {
  templateNames.value = Object.fromEntries(customTemplates.value.map((item) => [item.id, item.label]));
  managingTemplates.value = true;
}

async function renameCustomTemplate(id: string) {
  const label = templateNames.value[id]?.trim();
  if (!label) {
    error.value = "模板要有个名字";
    return;
  }
  await run(async () => {
    await renameTemplate(id, label);
    await reloadTemplates();
  });
}

async function removeCustomTemplate(id: string) {
  await run(async () => {
    await deleteTemplate(id);
    await reloadTemplates();
    if (templateId.value === id) templateId.value = "blank";
  });
}

async function toggleTemplateQuick(item: PlanTemplate, event: Event) {
  const quick = (event.target as HTMLInputElement).checked;
  await run(async () => {
    await setTemplateQuick(item.id, quick);
    await reloadTemplates();
  });
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
    dependsOn: stage.dependsOn,
    completedAt: stage.completedAt,
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

async function exportCalendar(plan: Plan) {
  if (!plan.stages.some((stage) => stage.at != null)) {
    error.value = "这个计划还没有带时间的阶段";
    return;
  }
  await run(async () => {
    const path = await save({
      defaultPath: planCalendarFileName(plan.title),
      filters: [{ name: "iCalendar", extensions: ["ics"] }],
    });
    if (!path) return;
    await writeTextFile(path, buildPlanCalendar(plan));
  });
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
        dependsOn: stage.dependsOn,
        completedAt: stage.completedAt,
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
        dependsOn: stage.dependsOn,
        completedAt: stage.completedAt,
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
          v-for="item in quickTemplates"
          :key="item.id"
          type="button"
          class="chip"
          :class="{ on: templateId === item.id }"
          @click="templateId = item.id"
        >
          {{ item.label }}
        </button>
        <button type="button" class="chip template-manage-trigger" @click="openTemplateManager">管理</button>
      </div>
      <p v-if="selectedTemplate" class="template-preview">{{ templatePreview(selectedTemplate) }}</p>
    </div>

    <div v-if="tier === 'mini'" class="toolbar mini-toolbar">
      <button type="button" class="chip on" @click="span = 'week'">本周⌄</button>
      <span class="grow"></span>
      <button type="button" class="chip" :class="{ on: important }" @click="important = !important">只看重要</button>
    </div>

    <div :class="tier === 'mini' ? 'mini-scroll' : 'main__scroll'">
      <p v-if="error || loadError || templateError" class="banner">{{ error || loadError || templateError }}</p>

      <div
        v-if="view === 'cards' && tier !== 'mini' && settings.deepSeek.enabled && settings.deepSeek.hasApiKey"
        class="ai-plan"
      >
        <textarea
          v-model="aiText"
          maxlength="2000"
          rows="3"
          placeholder="描述目标、截止时间和关键步骤，AI 会生成一份可编辑草稿"
        ></textarea>
        <div>
          <span>{{ aiError || "生成结果会先进入计划编辑器，不会直接保存" }}</span>
          <button class="btn btn--sm" type="button" :disabled="!aiText.trim() || aiGenerating" @click="generateDraft">
            {{ aiGenerating ? "生成中…" : "AI 生成计划" }}
          </button>
        </div>
      </div>

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
            v-for="item in quickTemplates"
            :key="item.id"
            type="button"
            class="chip"
            :class="{ on: templateId === item.id }"
            @click="templateId = item.id"
          >
            {{ item.label }}
          </button>
          <button type="button" class="chip" @click="openTemplateManager">管理模板</button>
          <span class="grow"></span>
          <button class="btn btn--primary btn--sm" type="button" :disabled="saving" @click="commit">记下</button>
        </div>
        <p v-if="selectedTemplate" class="template-preview">
          <span>阶段预览</span>{{ templatePreview(selectedTemplate) }}
        </p>
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
          @save-template="beginSaveTemplate(plan)"
          @export-calendar="exportCalendar(plan)"
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
        <span><span class="kbd">T</span>顺延 1 天</span>
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
      @save-template="beginSaveTemplate(draft)"
    />

    <Teleport v-if="templateSource" to="body">
      <div class="scrim" @click="templateSource = null"></div>
      <div class="drawer drawer--sm template-drawer" role="dialog" aria-modal="true">
        <div class="drawer__head">
          <div>
            <p class="eyebrow">模板</p>
            <div class="drawer__title">另存为模板</div>
          </div>
          <button class="icon-btn" type="button" aria-label="关闭" @click="templateSource = null"><Icon name="x" small /></button>
        </div>
        <div class="drawer__body">
          <div class="field">
            <div class="field__label"><span>模板名称</span></div>
            <div class="input input--big">
              <input v-model="templateName" maxlength="40" autofocus @keydown.enter.prevent="confirmSaveTemplate" />
            </div>
          </div>
          <div class="template-stage-list">
            <span v-for="(stage, index) in templateSource.stages" :key="index">{{ stage.title || "未命名阶段" }}</span>
            <p v-if="templateSource.stages.length === 0" class="field__hint">这个模板不预设阶段。</p>
          </div>
        </div>
        <div class="drawer__foot">
          <button class="btn btn--primary" type="button" :disabled="saving" @click="confirmSaveTemplate">保存模板</button>
          <button class="btn" type="button" @click="templateSource = null">取消</button>
        </div>
      </div>
    </Teleport>

    <Teleport v-if="managingTemplates" to="body">
      <div class="scrim" @click="managingTemplates = false"></div>
      <div class="drawer drawer--sm template-drawer" role="dialog" aria-modal="true">
        <div class="drawer__head">
          <div>
            <p class="eyebrow">模板</p>
            <div class="drawer__title">管理模板</div>
          </div>
          <button class="icon-btn" type="button" aria-label="关闭" @click="managingTemplates = false"><Icon name="x" small /></button>
        </div>
        <div class="drawer__body">
          <div v-for="item in templates" :key="item.id" class="template-manage-row">
            <strong v-if="item.builtIn">{{ item.label }}</strong>
            <div v-else class="input"><input v-model="templateNames[item.id]" maxlength="40" /></div>
            <button v-if="!item.builtIn" class="btn btn--sm" type="button" :disabled="saving" @click="renameCustomTemplate(item.id)">重命名</button>
            <button
              v-if="!item.builtIn"
              class="icon-btn template-delete"
              type="button"
              aria-label="删除模板"
              :disabled="saving"
              @click="removeCustomTemplate(item.id)"
            >
              <Icon name="trash" small />
            </button>
            <label class="settings-check template-quick">
              <input
                type="checkbox"
                :checked="item.quick"
                :disabled="saving"
                @change="toggleTemplateQuick(item, $event)"
              />
              <span>快捷显示</span>
            </label>
            <p>{{ templatePreview(item) }}</p>
          </div>
          <p class="field__hint">勾选“快捷显示”的模板才会出现在新建计划下方；内置模板不能重命名或删除。</p>
        </div>
      </div>
    </Teleport>
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
