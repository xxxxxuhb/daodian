const DAY_MS = 86_400_000;

export function startOfDay(timestamp: number) {
  const date = new Date(timestamp);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

function pad(value: number) {
  return String(value).padStart(2, "0");
}

export function formatClock(timestamp: number) {
  const date = new Date(timestamp);
  const week = "日一二三四五六"[date.getDay()];
  return `${date.getMonth() + 1}月${date.getDate()}日 周${week} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function formatWhen(timestamp: number, now: number) {
  const diffDays = Math.round((startOfDay(timestamp) - startOfDay(now)) / DAY_MS);
  const date = new Date(timestamp);
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  const day =
    diffDays === 0
      ? "今天"
      : diffDays === 1
        ? "明天"
        : diffDays === -1
          ? "昨天"
          : `${date.getMonth() + 1}月${date.getDate()}日`;
  return `${day} ${time}`;
}

export function timeMeta(at: number | null, now: number, reminded: boolean, done: boolean) {
  if (at == null) return "未设时间";
  const when = formatWhen(at, now);
  if (done) return when;
  if (reminded) return `${when} · 已提醒`;
  if (at < now) return `${when} · 已过`;
  return when;
}
