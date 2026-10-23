import { startOfDay } from "./time.ts";
import type { Plan, Todo } from "./types.ts";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 86_400_000;

function pad(value: number) {
  return String(value).padStart(2, "0");
}

export function clockFace(now: number) {
  const date = new Date(now);
  const week = "日一二三四五六"[date.getDay()];
  return {
    date: `${date.getMonth() + 1}月${date.getDate()}日`,
    rest: `周${week} · ${pad(date.getHours())}:${pad(date.getMinutes())}`,
  };
}

export function clockTime(at: number) {
  const date = new Date(at);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function dayLabel(at: number, now: number) {
  const diff = Math.round((startOfDay(at) - startOfDay(now)) / DAY);
  if (diff === 0) return "今天";
  if (diff === 1) return "明天";
  if (diff === -1) return "昨天";
  const date = new Date(at);
  return `${date.getMonth() + 1}/${date.getDate()}`;
}

export function whenParts(at: number | null, now: number) {
  if (at == null) return { time: "—", day: "未定" };
  return { time: clockTime(at), day: dayLabel(at, now) };
}

export function stageWhen(at: number | null, now: number) {
  if (at == null) return "未定";
  return `${dayLabel(at, now)} ${clockTime(at)}`;
}

export function stageLong(at: number | null, now: number) {
  if (at == null) return "选一个时间";
  const diff = Math.round((startOfDay(at) - startOfDay(now)) / DAY);
  const time = clockTime(at);
  if (diff === 0) return `今天 ${time}`;
  if (diff === 1) return `明天 ${time}`;
  if (diff === -1) return `昨天 ${time}`;
  const date = new Date(at);
  const week = "日一二三四五六"[date.getDay()];
  return `${date.getMonth() + 1}月${date.getDate()}日 周${week} ${time}`;
}

export type PillTone = "red" | "amber" | "plain" | "line" | "moss";

export function relative(at: number, now: number): { text: string; tone: PillTone } {
  const delta = at - now;
  if (delta < 0) {
    const late = -delta;
    if (late < HOUR) return { text: `逾期 ${Math.max(1, Math.round(late / MIN))} 分钟`, tone: "red" };
    if (late < DAY) return { text: `逾期 ${Math.max(1, Math.round(late / HOUR))} 小时`, tone: "red" };
    return { text: `逾期 ${Math.max(1, Math.round(late / DAY))} 天`, tone: "red" };
  }
  if (delta < 45 * MIN) return { text: `${Math.max(1, Math.round(delta / MIN))} 分钟后`, tone: "amber" };
  if (delta < 2 * HOUR) return { text: `${Math.max(1, Math.round(delta / HOUR))} 小时后`, tone: "amber" };
  const day = Math.round((startOfDay(at) - startOfDay(now)) / DAY);
  if (day === 0 && new Date(at).getHours() >= 18) return { text: "今晚", tone: "line" };
  if (day === 0) return { text: `${Math.round(delta / HOUR)} 小时后`, tone: "plain" };
  if (day === 1) return { text: "明天", tone: "line" };
  return { text: dayLabel(at, now), tone: "plain" };
}

export function countdown(at: number, now: number) {
  const total = Math.max(0, Math.round((at - now) / MIN));
  const hours = Math.floor(total / 60);
  const minutes = total % 60;
  return `${hours}:${pad(minutes)}`;
}

export function rowState(todo: Todo, now: number) {
  if (todo.done) return "is-done";
  if (todo.dueAt == null) return "";
  if (todo.dueAt < now) return "is-overdue";
  if (todo.dueAt - now <= 2 * HOUR) return "is-soon";
  return "";
}

export function splitToday(todos: Todo[], now: number) {
  const past: Todo[] = [];
  const later: Todo[] = [];
  const unset: Todo[] = [];
  for (const todo of todos) {
    if (todo.dueAt == null) unset.push(todo);
    else if (todo.dueAt < now) past.push(todo);
    else later.push(todo);
  }
  return { past, later, unset };
}

export function atClock(now: number, dayOffset: number, hour: number, minute: number) {
  return startOfDay(now) + dayOffset * DAY + hour * HOUR + minute * MIN;
}

export function toLocalInput(at: number | null) {
  if (at == null) return "";
  const date = new Date(at);
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function fromLocalInput(value: string) {
  if (!value) return null;
  const time = new Date(value).getTime();
  return Number.isNaN(time) ? null : time;
}

export interface Upcoming {
  at: number;
  title: string;
  context: string;
  kind: "todo" | "stage";
  todoId?: string;
  planId?: string;
  stageId?: string;
  index: number;
  total: number;
}

export function upcoming(todos: Todo[], plans: Plan[], now: number): Upcoming | null {
  let best: Upcoming | null = null;
  const consider = (item: Upcoming) => {
    if (item.at < now) return;
    if (best == null || item.at < best.at) best = item;
  };
  for (const todo of todos) {
    if (todo.done || todo.dueAt == null) continue;
    consider({
      at: todo.dueAt,
      title: todo.title,
      context: "待办",
      kind: "todo",
      todoId: todo.id,
      index: 1,
      total: 1,
    });
  }
  for (const plan of plans) {
    if (plan.done) continue;
    plan.stages.forEach((stage, index) => {
      if (stage.done || stage.at == null) return;
      consider({
        at: stage.at,
        title: stage.title,
        context: plan.title,
        kind: "stage",
        planId: plan.id,
        stageId: stage.id,
        index: index + 1,
        total: plan.stages.length,
      });
    });
  }
  return best;
}

export interface TimelineEntry {
  key: string;
  at: number;
  time: string;
  title: string;
  meta: string;
  done: boolean;
  readonly?: boolean;
}

export function todayTimeline(todos: Todo[], plans: Plan[], now: number): TimelineEntry[] {
  const start = startOfDay(now);
  const end = start + DAY;
  const items: TimelineEntry[] = [];
  for (const todo of todos) {
    if (todo.dueAt == null || todo.dueAt < start || todo.dueAt >= end) continue;
    items.push({
      key: `todo:${todo.id}`,
      at: todo.dueAt,
      time: clockTime(todo.dueAt),
      title: todo.title,
      meta: "待办",
      done: todo.done,
    });
  }
  for (const plan of plans) {
    for (const stage of plan.stages) {
      if (stage.at == null || stage.at < start || stage.at >= end) continue;
      items.push({
        key: `stage:${stage.id}`,
        at: stage.at,
        time: clockTime(stage.at),
        title: stage.title,
        meta: `计划 · ${plan.title}`,
        done: stage.done || plan.done,
      });
    }
  }
  return items.sort((a, b) => a.at - b.at);
}

export function todayProgress(todos: Todo[], now: number) {
  const start = startOfDay(now);
  const end = start + DAY;
  const dated = todos.filter((todo) => todo.dueAt != null && todo.dueAt >= start && todo.dueAt < end);
  const undated = todos.filter((todo) => !todo.done && todo.dueAt == null);
  const total = dated.length + undated.length;
  const done = dated.filter((todo) => todo.done).length;
  return { done, total, ratio: total === 0 ? 0 : done / total };
}

export const DEFAULT_WEEK_REPORT_TEMPLATE = `# 本周周报

## 概览
{{overview}}

## 待办
{{todos}}

## 计划推进
{{plans}}`;

export interface WeekSummary {
  done: number;
  overdue: number;
  advanced: number;
  bounds: { start: number; end: number };
  rangeLabel: string;
  overview: string;
  todoText: string;
  planText: string;
  planMarkdown: string;
  text: string;
}

export function renderWeekReport(
  template: string,
  values: Pick<WeekSummary, "overview" | "todoText" | "planText">,
): string {
  return template
    .split("{{overview}}").join(values.overview)
    .split("{{todos}}").join(values.todoText)
    .split("{{plans}}").join(values.planText)
    .trim();
}

export interface WeekSummaryOptions {
  bounds?: { start: number; end: number };
  weekOffset?: number;
}

export function weekSummary(
  todos: Todo[],
  plans: Plan[],
  now: number,
  template = DEFAULT_WEEK_REPORT_TEMPLATE,
  options: WeekSummaryOptions = {},
): WeekSummary {
  const selected = options.bounds
    ? { monday: options.bounds.start, next: options.bounds.end }
    : weekBounds(now, options.weekOffset ?? 0);
  const { monday, next } = selected;
  const completedTodos = todos.filter(
    (todo) => todo.done && todo.updatedAt >= monday && todo.updatedAt < next,
  );
  const openTodos = todos.filter(
    (todo) => !todo.done && todo.dueAt != null && todo.dueAt >= monday && todo.dueAt < next,
  );
  const done = completedTodos.length;
  const overdue = todos.filter(
    (todo) => !todo.done
      && todo.dueAt != null
      && todo.dueAt < now
      && todo.dueAt >= monday
      && todo.dueAt < next,
  ).length;
  const planRows = plans.flatMap((plan) => {
    const completed = plan.stages.filter((stage) => {
      if (!stage.done) return false;
      if (stage.completedAt != null) return stage.completedAt >= monday && stage.completedAt < next;
      return stage.at != null && stage.at >= monday && stage.at < next;
    });
    const current = plan.stages.find((stage) => !stage.done);
    if (completed.length === 0 && !current) return [];
    const details: string[] = [];
    if (completed.length) details.push(`本周完成：${completed.map((stage) => stage.title).join("、")}`);
    if (current) details.push(`当前：${current.title}`);
    else if (plan.done || plan.stages.length > 0) details.push("当前：全部阶段已完成");
    return [{ text: `- ${plan.title} — ${details.join("；")}`, advanced: completed.length > 0 }];
  });
  const advanced = planRows.filter((row) => row.advanced).length;
  const overview = `- 完成待办 ${done} 件\n- 逾期待办 ${overdue} 件\n- ${advanced} 个计划本周有推进`;
  const completedText = completedTodos.length
    ? completedTodos.map((todo) => `- [x] ${todo.title}`).join("\n")
    : "- [x] 本周暂无已完成待办";
  const openText = openTodos.length
    ? openTodos.map((todo) => `- [ ] ${todo.title}`).join("\n")
    : "- [ ] 本周暂无未完成待办";
  const todoText = `### 已完成\n${completedText}\n\n### 未完成\n${openText}`;
  const planText = planRows.length ? planRows.map((row) => row.text).join("\n") : "- 本周暂无计划推进";
  const planMarkdown = `## 计划推进\n${planText}`;
  const values = { overview, todoText, planText };
  return {
    done,
    overdue,
    advanced,
    bounds: { start: monday, end: next },
    rangeLabel: weekRangeLabel(monday, next),
    overview,
    todoText,
    planText,
    planMarkdown,
    text: renderWeekReport(template, values),
  };
}

export function weekBounds(now: number, weekOffset = 0) {
  const monday = startOfDay(now)
    - ((new Date(now).getDay() + 6) % 7) * DAY
    + Math.trunc(weekOffset) * 7 * DAY;
  return { monday, next: monday + 7 * DAY, today: (new Date(now).getDay() + 6) % 7 };
}

function weekRangeLabel(start: number, end: number) {
  const first = new Date(start);
  const last = new Date(end - 1);
  const date = (value: Date) => `${value.getFullYear()}.${pad(value.getMonth() + 1)}.${pad(value.getDate())}`;
  return `${date(first)} – ${date(last)}`;
}

export function weekBars(plans: Plan[], now: number) {
  const { monday, today } = weekBounds(now);
  const counts = [0, 0, 0, 0, 0, 0, 0];
  for (const plan of plans) {
    if (plan.done) continue;
    for (const stage of plan.stages) {
      if (stage.at == null) continue;
      const day = Math.round((startOfDay(stage.at) - monday) / DAY);
      if (day >= 0 && day < 7) counts[day] += 1;
    }
  }
  const max = Math.max(1, ...counts);
  return counts.map((count, index) => ({
    height: count === 0 ? 8 : Math.max(18, Math.round((count / max) * 100)),
    on: index === today,
    moss: index < today && count > 0,
    label: "一二三四五六日"[index] ?? "",
  }));
}

export function weekStats(plans: Plan[], now: number) {
  const { monday, next } = weekBounds(now);
  let total = 0;
  let done = 0;
  for (const plan of plans) {
    for (const stage of plan.stages) {
      if (stage.at == null || stage.at < monday || stage.at >= next) continue;
      total += 1;
      if (stage.done || plan.done) done += 1;
    }
  }
  return { total, done };
}

export function axisRange(now: number, mode: "week" | "month") {
  if (mode === "week") {
    const start = startOfDay(now);
    const days = Array.from({ length: 7 }, (_, index) => start + index * DAY);
    return { start, end: start + 7 * DAY, days };
  }
  const date = new Date(now);
  const start = new Date(date.getFullYear(), date.getMonth(), 1).getTime();
  const end = new Date(date.getFullYear(), date.getMonth() + 1, 1).getTime();
  const days: number[] = [];
  for (let cursor = start; cursor < end; cursor += DAY) days.push(cursor);
  return { start, end, days };
}

export function axisRatio(at: number, start: number, end: number) {
  if (end <= start) return 0;
  return (at - start) / (end - start);
}

export function dayHead(at: number) {
  const date = new Date(at);
  const week = "日一二三四五六"[date.getDay()];
  return `${date.getMonth() + 1}/${date.getDate()} ${week}`;
}

export function planPill(plan: Plan, now: number): { text: string; tone: PillTone } {
  if (plan.stages.length === 0) return { text: "还没分阶段", tone: "plain" };
  const next = plan.stages.find((stage) => !stage.done);
  if (next == null) return { text: "阶段都做完了", tone: "moss" };
  if (next.at == null) return { text: "还没定时间", tone: "plain" };
  const label = relative(next.at, now);
  if (label.tone === "red") return label;
  if (label.text === "明天") return { text: "明天到点", tone: "plain" };
  if (label.text === "今晚") return { text: "今晚到点", tone: "plain" };
  if (label.tone === "amber") return { text: `${label.text}到点`, tone: "amber" };
  return { text: `${label.text}到点`, tone: "plain" };
}

export interface RemindedHit {
  key: string;
  title: string;
  detail: string;
  time: string;
  todoId?: string;
  planId?: string;
  stageId?: string;
}

export function remindedHits(todos: Todo[], plans: Plan[], now: number): RemindedHit[] {
  const hits: RemindedHit[] = [];
  for (const todo of todos) {
    if (!todo.reminded || todo.done || todo.dueAt == null) continue;
    hits.push({
      key: `todo:${todo.id}:${todo.dueAt}`,
      title: todo.title,
      detail: `待办 · ${stageLong(todo.dueAt, now)} · 只提醒这一次`,
      time: clockTime(todo.dueAt),
      todoId: todo.id,
    });
  }
  for (const plan of plans) {
    if (plan.done) continue;
    for (const stage of plan.stages) {
      if (!stage.reminded || stage.done || stage.at == null) continue;
      hits.push({
        key: `stage:${plan.id}:${stage.id}:${stage.at}`,
        title: stage.title,
        detail: `${plan.title} · ${stageLong(stage.at, now)}`,
        time: clockTime(stage.at),
        planId: plan.id,
        stageId: stage.id,
      });
    }
  }
  return hits;
}
