import { priorityRank } from "../../domain/priority";
import type { Repeat } from "../../domain/repeat";
import { startOfDay, timeMeta } from "../../domain/time";
import type { Priority, Todo } from "../../domain/types";

const DAY_MS = 86_400_000;

export type TodoFilter = "today" | "later" | "overdue" | "done";

export const TODO_FILTERS: { id: TodoFilter; label: string; short: string; hint: string }[] = [
  { id: "today", label: "今天", short: "今", hint: "临时接到、今天要办的，还有还没定时间的" },
  { id: "later", label: "以后", short: "后", hint: "排在明天和更晚的" },
  { id: "overdue", label: "逾期", short: "逾", hint: "已经过了时间点，还没做完" },
  { id: "done", label: "已完成", short: "完", hint: "做完的会留在这里" },
];

export function isTodoFilter(value: string): value is TodoFilter {
  return TODO_FILTERS.some((item) => item.id === value);
}

export interface TodoDraft {
  id: string | null;
  title: string;
  note: string;
  dueAt: number | null;
  priority: Priority;
  done: boolean;
  repeat: Repeat | null;
  leadMs: number;
}

export function emptyTodoDraft(): TodoDraft {
  return { id: null, title: "", note: "", dueAt: null, priority: "normal", done: false, repeat: null, leadMs: 0 };
}

export function toTodoDraft(todo: Todo): TodoDraft {
  return {
    id: todo.id,
    title: todo.title,
    note: todo.note,
    dueAt: todo.dueAt,
    priority: todo.priority,
    done: todo.done,
    repeat: todo.repeat ? { ...todo.repeat } : null,
    leadMs: todo.leadMs ?? 0,
  };
}

export function matchesTodo(todo: Todo, filter: TodoFilter, now: number) {
  if (filter === "done") return todo.done;
  if (todo.done) return false;
  const start = startOfDay(now);
  const end = start + DAY_MS;
  if (filter === "overdue") return todo.dueAt != null && todo.dueAt < start;
  if (filter === "later") return todo.dueAt != null && todo.dueAt >= end;
  return todo.dueAt == null || (todo.dueAt >= start && todo.dueAt < end);
}

export function todoBucket(todo: Todo, now: number): TodoFilter {
  if (todo.done) return "done";
  if (matchesTodo(todo, "overdue", now)) return "overdue";
  if (matchesTodo(todo, "later", now)) return "later";
  return "today";
}

export function todoCounts(todos: Todo[], now: number) {
  const counts: Record<TodoFilter, number> = { today: 0, later: 0, overdue: 0, done: 0 };
  for (const todo of todos) {
    for (const filter of TODO_FILTERS) {
      if (matchesTodo(todo, filter.id, now)) counts[filter.id] += 1;
    }
  }
  return counts;
}

export function compareTodos(a: Todo, b: Todo) {
  const aDue = a.dueAt ?? Number.POSITIVE_INFINITY;
  const bDue = b.dueAt ?? Number.POSITIVE_INFINITY;
  if (aDue !== bDue) return aDue - bDue;
  const delta = priorityRank(a.priority) - priorityRank(b.priority);
  if (delta !== 0) return delta;
  return b.createdAt - a.createdAt;
}

export function todoMeta(todo: Todo, now: number) {
  return timeMeta(todo.dueAt, now, todo.reminded, todo.done);
}

export function todoTone(todo: Todo, now: number) {
  if (todo.done || todo.dueAt == null) return "idle";
  if (todo.dueAt < now) return "overdue";
  if (todo.dueAt - now <= 60 * 60 * 1000) return "soon";
  return "idle";
}

export function emptyTodoCopy(filter: TodoFilter, hasQuery: boolean) {
  if (hasQuery) return "没有匹配的待办";
  if (filter === "today") return "今天还没有临时事项";
  if (filter === "later") return "还没有排到以后的待办";
  if (filter === "overdue") return "没有逾期的待办";
  if (filter === "done") return "还没有完成的待办";
  return "记一件临时的事，到点提醒一次";
}

export function todoMatchesQuery(todo: Todo, keyword: string) {
  const needle = keyword.trim().toLowerCase();
  if (!needle) return true;
  return `${todo.title}\n${todo.note}`.toLowerCase().includes(needle);
}
