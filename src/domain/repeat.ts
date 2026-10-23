export type RepeatRule = "daily" | "weekly" | "weekdays" | "monthEnd" | "custom";

export interface Repeat {
  rule: RepeatRule;
  weekday: number | null;
  intervalDays: number | null;
  until: number | null;
  skipWeekends: boolean;
}

export const LEADS: { ms: number; label: string }[] = [
  { ms: 0, label: "到点" },
  { ms: 10 * 60_000, label: "提前 10 分钟" },
  { ms: 60 * 60_000, label: "提前 1 小时" },
  { ms: 24 * 60 * 60_000, label: "提前 1 天" },
];

const WEEKDAY = ["日", "一", "二", "三", "四", "五", "六"];

export interface RepeatCalendar {
  holidays: string[];
  workdays: string[];
}

export function repeatLabel(repeat: Repeat | null, dueAt: number | null): string {
  if (!repeat) return "不重复";
  if (repeat.rule === "daily") return repeat.skipWeekends ? "每天（跳过周末）" : "每天";
  if (repeat.rule === "weekdays") return "每个工作日";
  if (repeat.rule === "monthEnd") return "每月最后一天";
  if (repeat.rule === "custom") return `每 ${repeat.intervalDays ?? 1} 天`;
  const day = repeat.weekday ?? (dueAt == null ? 5 : new Date(dueAt).getDay());
  return `每周${WEEKDAY[day] ?? "五"}`;
}

export function nextOccurrence(
  repeat: Repeat,
  dueAt: number,
  calendar: RepeatCalendar = { holidays: [], workdays: [] },
): number {
  const date = new Date(dueAt);
  if (repeat.rule === "daily" || repeat.rule === "custom") {
    date.setDate(date.getDate() + (repeat.rule === "custom" ? Math.max(1, repeat.intervalDays ?? 1) : 1));
    moveToAvailable(date, 1, repeat.skipWeekends, calendar);
    return date.getTime();
  }
  if (repeat.rule === "weekly") {
    const wanted = repeat.weekday ?? date.getDay();
    const delta = (wanted - date.getDay() + 7) % 7 || 7;
    date.setDate(date.getDate() + delta);
    moveToAvailable(date, 1, repeat.skipWeekends, calendar);
    return date.getTime();
  }
  if (repeat.rule === "monthEnd") {
    const next = new Date(date.getFullYear(), date.getMonth() + 2, 0, date.getHours(), date.getMinutes(), 0, 0);
    moveToAvailable(next, -1, repeat.skipWeekends, calendar);
    return next.getTime();
  }
  date.setDate(date.getDate() + 1);
  moveToAvailable(date, 1, true, calendar);
  return date.getTime();
}

function moveToAvailable(
  date: Date,
  direction: 1 | -1,
  skipWeekends: boolean,
  calendar: RepeatCalendar,
) {
  while (isUnavailable(date, skipWeekends, calendar)) {
    date.setDate(date.getDate() + direction);
  }
}

function isUnavailable(date: Date, skipWeekends: boolean, calendar: RepeatCalendar) {
  const key = localDateText(date);
  if (calendar.workdays.includes(key)) return false;
  return calendar.holidays.includes(key)
    || (skipWeekends && (date.getDay() === 0 || date.getDay() === 6));
}

function localDateText(date: Date) {
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

export function nextLabel(
  repeat: Repeat | null,
  dueAt: number | null,
  calendar?: RepeatCalendar,
): string {
  if (!repeat || dueAt == null) return "";
  const next = new Date(nextOccurrence(repeat, dueAt, calendar));
  const month = next.getMonth() + 1;
  const day = next.getDate();
  const hour = `${next.getHours()}`.padStart(2, "0");
  const minute = `${next.getMinutes()}`.padStart(2, "0");
  return `下一次 ${month}月${day}日 ${hour}:${minute}`;
}
