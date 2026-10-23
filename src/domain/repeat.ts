export type RepeatRule = "weekly" | "weekdays" | "monthEnd";

export interface Repeat {
  rule: RepeatRule;
  weekday: number | null;
}

export const LEADS: { ms: number; label: string }[] = [
  { ms: 0, label: "到点" },
  { ms: 10 * 60_000, label: "提前 10 分钟" },
  { ms: 60 * 60_000, label: "提前 1 小时" },
  { ms: 24 * 60 * 60_000, label: "提前 1 天" },
];

const WEEKDAY = ["日", "一", "二", "三", "四", "五", "六"];

export function repeatLabel(repeat: Repeat | null, dueAt: number | null): string {
  if (!repeat) return "不重复";
  if (repeat.rule === "weekdays") return "每个工作日";
  if (repeat.rule === "monthEnd") return "每月最后一天";
  const day = repeat.weekday ?? (dueAt == null ? 5 : new Date(dueAt).getDay());
  return `每周${WEEKDAY[day] ?? "五"}`;
}

export function nextOccurrence(repeat: Repeat, dueAt: number): number {
  const date = new Date(dueAt);
  if (repeat.rule === "weekly") {
    date.setDate(date.getDate() + 7);
    return date.getTime();
  }
  if (repeat.rule === "monthEnd") {
    const next = new Date(date.getFullYear(), date.getMonth() + 2, 0, date.getHours(), date.getMinutes(), 0, 0);
    return next.getTime();
  }
  date.setDate(date.getDate() + 1);
  while (date.getDay() === 0 || date.getDay() === 6) date.setDate(date.getDate() + 1);
  return date.getTime();
}

export function nextLabel(repeat: Repeat | null, dueAt: number | null): string {
  if (!repeat || dueAt == null) return "";
  const next = new Date(nextOccurrence(repeat, dueAt));
  const month = next.getMonth() + 1;
  const day = next.getDate();
  const hour = `${next.getHours()}`.padStart(2, "0");
  const minute = `${next.getMinutes()}`.padStart(2, "0");
  return `下一次 ${month}月${day}日 ${hour}:${minute}`;
}
