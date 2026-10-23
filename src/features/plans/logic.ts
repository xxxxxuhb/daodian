import { priorityRank } from "../../domain/priority";
import { atClock } from "../../domain/present";
import { timeMeta } from "../../domain/time";
import type { Plan, Priority, Stage, StageInput } from "../../domain/types";

export const PLAN_TEMPLATES: {
  id: string;
  label: string;
  stages: { title: string; day: number; hour: number; minute: number }[];
}[] = [
  {
    id: "release",
    label: "提测 → 发布 → 观察",
    stages: [
      { title: "提测", day: 0, hour: 15, minute: 0 },
      { title: "发布", day: 1, hour: 10, minute: 0 },
      { title: "观察", day: 3, hour: 10, minute: 0 },
    ],
  },
  {
    id: "meeting",
    label: "周会改版",
    stages: [
      { title: "议程", day: 0, hour: 9, minute: 0 },
      { title: "改版", day: 2, hour: 14, minute: 0 },
      { title: "定稿", day: 4, hour: 16, minute: 0 },
    ],
  },
  { id: "blank", label: "空计划", stages: [] },
];

export function stagesFromTemplate(id: string, now: number): StageInput[] {
  const template = PLAN_TEMPLATES.find((item) => item.id === id) ?? PLAN_TEMPLATES[0]!;
  return template.stages.map((stage) => ({
    id: null,
    title: stage.title,
    at: atClock(now, stage.day, stage.hour, stage.minute),
    done: false,
  }));
}

export type PlanFilter = "active" | "done";

export const PLAN_FILTERS: { id: PlanFilter; label: string; short: string; hint: string }[] = [
  { id: "active", label: "进行中", short: "进", hint: "一件事分成多个阶段，比如提测和发布，到了分别提醒" },
  { id: "done", label: "已完成", short: "完", hint: "整份计划已经做完" },
];

export const MAX_STAGES = 20;

export function isPlanFilter(value: string): value is PlanFilter {
  return PLAN_FILTERS.some((item) => item.id === value);
}

export interface StageDraft {
  key: string;
  id: string | null;
  title: string;
  at: number | null;
  done: boolean;
}

export interface PlanDraft {
  id: string | null;
  title: string;
  note: string;
  priority: Priority;
  done: boolean;
  templateId: string | null;
  stages: StageDraft[];
}

export function newStageKey() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function emptyStage(): StageDraft {
  return { key: newStageKey(), id: null, title: "", at: null, done: false };
}

export function emptyPlanDraft(): PlanDraft {
  return {
    id: null,
    title: "",
    note: "",
    priority: "normal",
    done: false,
    templateId: null,
    stages: [emptyStage(), emptyStage()],
  };
}

export function toPlanDraft(plan: Plan): PlanDraft {
  return {
    id: plan.id,
    title: plan.title,
    note: plan.note,
    priority: plan.priority,
    done: plan.done,
    templateId: plan.templateId,
    stages: plan.stages.map((stage) => ({
      key: stage.id,
      id: stage.id,
      title: stage.title,
      at: stage.at,
      done: stage.done,
    })),
  };
}

export function adoptSavedStages(draft: PlanDraft, saved: Plan): PlanDraft {
  let index = 0;
  return {
    ...draft,
    id: saved.id,
    done: saved.done,
    stages: draft.stages.map((stage) => {
      const meaningful = Boolean(stage.title.trim() || stage.at != null || stage.done);
      if (!meaningful) return stage;
      const server = saved.stages[index];
      index += 1;
      if (!server) return stage;
      return { ...stage, id: server.id, done: server.done };
    }),
  };
}

export function matchesPlan(plan: Plan, filter: PlanFilter) {
  return filter === "done" ? plan.done : !plan.done;
}

export function planCounts(plans: Plan[]) {
  return {
    active: plans.filter((plan) => matchesPlan(plan, "active")).length,
    done: plans.filter((plan) => matchesPlan(plan, "done")).length,
  };
}

export function stageProgress(plan: Plan) {
  return {
    done: plan.stages.filter((stage) => stage.done).length,
    total: plan.stages.length,
  };
}

export function planMeta(plan: Plan, now: number) {
  const { done, total } = stageProgress(plan);
  if (total === 0) return "还没分阶段";
  const head = `${done}/${total} 个阶段`;
  const next = plan.stages.find((stage) => !stage.done);
  if (next == null) return `${head} · 阶段都做完了`;
  if (next.at == null) return `${head} · 下一阶段 ${next.title}`;
  return `${head} · ${next.title} ${timeMeta(next.at, now, next.reminded, false)}`;
}

export function planTone(plan: Plan, now: number) {
  if (plan.done) return "idle";
  const next = plan.stages.find((stage) => !stage.done && stage.at != null);
  if (next?.at == null) return "idle";
  if (next.at < now) return "overdue";
  if (next.at - now <= 60 * 60 * 1000) return "soon";
  return "idle";
}

export function comparePlans(a: Plan, b: Plan) {
  const timeOf = (plan: Plan) => plan.stages.find((stage) => !stage.done && stage.at != null)?.at ?? Number.POSITIVE_INFINITY;
  const delta = timeOf(a) - timeOf(b);
  if (delta !== 0) return delta;
  const priority = priorityRank(a.priority) - priorityRank(b.priority);
  if (priority !== 0) return priority;
  return b.updatedAt - a.updatedAt;
}

export function stageMeta(stage: Stage, now: number) {
  return timeMeta(stage.at, now, stage.reminded, stage.done);
}

export function stagePlaceholder(index: number) {
  if (index === 0) return "例如提测";
  if (index === 1) return "例如发布";
  return "阶段名称";
}

export function stagesForSave(stages: StageDraft[]): StageInput[] | string {
  const meaningful = stages.filter((stage) => stage.title.trim() || stage.at != null || stage.done);
  if (meaningful.some((stage) => !stage.title.trim())) return "阶段要有个名字";
  if (meaningful.length > MAX_STAGES) return "最多 20 个阶段";
  return meaningful.map((stage) => ({
    id: stage.id,
    title: stage.title.trim(),
    at: stage.at,
    done: stage.done,
  }));
}

export function moveStage(stages: StageDraft[], index: number, direction: -1 | 1) {
  const target = index + direction;
  if (target < 0 || target >= stages.length) return stages;
  const next = stages.slice();
  const [item] = next.splice(index, 1);
  if (!item) return stages;
  next.splice(target, 0, item);
  return next;
}

export function emptyPlanCopy(filter: PlanFilter, hasQuery: boolean) {
  if (hasQuery) return "没有匹配的计划";
  if (filter === "done") return "还没有完成的计划";
  return "记一个计划，再分成提测、发布这样的阶段";
}

export function planMatchesQuery(plan: Plan, keyword: string) {
  if (!keyword) return true;
  const haystack = [plan.title, plan.note, ...plan.stages.map((stage) => stage.title)].join("\n").toLowerCase();
  return haystack.includes(keyword);
}
