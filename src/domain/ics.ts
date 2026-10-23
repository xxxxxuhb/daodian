import type { Plan } from "./types.ts";

function pad(value: number) {
  return String(value).padStart(2, "0");
}

function utc(at: number) {
  const date = new Date(at);
  return `${date.getUTCFullYear()}${pad(date.getUTCMonth() + 1)}${pad(date.getUTCDate())}T${pad(date.getUTCHours())}${pad(date.getUTCMinutes())}${pad(date.getUTCSeconds())}Z`;
}

function text(value: string) {
  return value
    .replace(/\\/g, "\\\\")
    .replace(/\n/g, "\\n")
    .replace(/,/g, "\\,")
    .replace(/;/g, "\\;");
}

function fold(line: string) {
  const chunks: string[] = [];
  let rest = line;
  while (new TextEncoder().encode(rest).length > 73) {
    let index = Math.min(73, rest.length);
    while (index > 1 && new TextEncoder().encode(rest.slice(0, index)).length > 73) index -= 1;
    chunks.push(rest.slice(0, index));
    rest = ` ${rest.slice(index)}`;
  }
  chunks.push(rest);
  return chunks.join("\r\n");
}

export function buildPlanCalendar(plan: Pick<Plan, "id" | "title" | "note" | "stages" | "updatedAt">) {
  const timed = plan.stages.filter(
    (stage): stage is typeof stage & { at: number } => stage.at != null,
  );
  const lines = [
    "BEGIN:VCALENDAR",
    "VERSION:2.0",
    "PRODID:-//Daodian//Plan Export//ZH-CN",
    "CALSCALE:GREGORIAN",
    `X-WR-CALNAME:${text(plan.title)}`,
  ];
  for (const stage of timed) {
    lines.push(
      "BEGIN:VEVENT",
      `UID:${text(`${plan.id}.${stage.id}@daodian`)}`,
      `DTSTAMP:${utc(plan.updatedAt)}`,
      `DTSTART:${utc(stage.at)}`,
      `DTEND:${utc(stage.at + 3_600_000)}`,
      `SUMMARY:${text(`${plan.title} · ${stage.title}`)}`,
      `DESCRIPTION:${text(plan.note || `计划阶段：${stage.title}`)}`,
      "END:VEVENT",
    );
  }
  lines.push("END:VCALENDAR");
  return `${lines.map(fold).join("\r\n")}\r\n`;
}

export function planCalendarFileName(title: string) {
  const safe = title.trim().replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").replace(/[. ]+$/g, "");
  return `${safe || "计划"}.ics`;
}
