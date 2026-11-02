import type { NoteIndex, NoteNode } from "../../domain/notes.ts";
import { clockTime, stageLong, type RemindedHit } from "../../domain/present.ts";
import { startOfDay } from "../../domain/time.ts";

export type NoteFilter = "all" | "recent" | "due" | "archived";

export const NOTE_FILTERS: { id: NoteFilter; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "recent", label: "最近" },
  { id: "due", label: "有到点" },
  { id: "archived", label: "归档" },
];

const WEEK = 7 * 86_400_000;
const DAY = 86_400_000;
export const MAX_DOC_BYTES = 2 * 1024 * 1024;
export const MAX_IMAGES = 200;
export const VIRTUAL_AFTER = 200;
export const PAGE_AFTER = 5000;
export const PAGE_SIZE = 50;

export function noteVisible(note: NoteIndex, filter: NoteFilter, now: number) {
  if (filter === "archived") return note.archived;
  if (note.archived) return false;
  if (filter === "due") return note.dueAt != null;
  if (filter === "recent") return now - note.updatedAt <= WEEK;
  return true;
}

export function matchScore(note: NoteIndex, query: string) {
  const keyword = query.trim().toLowerCase();
  if (!keyword) return 1;
  if (note.title.toLowerCase().includes(keyword)) return 3;
  if (note.tags.some((tag) => tag.toLowerCase().includes(keyword))) return 2;
  if (`${note.summary}\n${note.text}`.toLowerCase().includes(keyword)) return 1;
  return 0;
}

export function sortNotes(notes: NoteIndex[], filter: NoteFilter, query: string) {
  return [...notes].sort((left, right) => {
    if (query.trim()) {
      const score = matchScore(right, query) - matchScore(left, query);
      if (score !== 0) return score;
    }
    if (left.pinned !== right.pinned) return left.pinned ? -1 : 1;
    if (filter === "due") {
      return (left.dueAt ?? Number.MAX_SAFE_INTEGER) - (right.dueAt ?? Number.MAX_SAFE_INTEGER);
    }
    return right.updatedAt - left.updatedAt;
  });
}

export function filterNotes(notes: NoteIndex[], filter: NoteFilter, query: string, now: number, tag = "") {
  return sortNotes(
    notes.filter((note) => noteVisible(note, filter, now) && matchScore(note, query) > 0 && (!tag || note.tags.includes(tag))),
    filter,
    query,
  );
}

export function noteTagEntries(previous: string[], notes: NoteIndex[]) {
  const counts = new Map<string, number>();
  const fresh: string[] = [];
  const ordered = notes
    .filter((note) => !note.archived)
    .slice()
    .sort((left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id));
  for (const note of ordered) {
    for (const tag of note.tags) {
      counts.set(tag, (counts.get(tag) ?? 0) + 1);
      if (!fresh.includes(tag)) fresh.push(tag);
    }
  }
  const tags = previous.filter((tag) => counts.has(tag));
  for (const tag of fresh) {
    if (!tags.includes(tag)) tags.push(tag);
  }
  return tags.map((tag) => ({ tag, count: counts.get(tag) ?? 0 }));
}

export function emptyNoteCopy(filter: NoteFilter, query: string, tag = "") {
  if (query.trim()) return "没有匹配的备忘录";
  if (tag) {
    if (filter === "archived") return `归档里没有「${tag}」`;
    if (filter === "due") return `「${tag}」还没有设到点`;
    if (filter === "recent") return `最近 7 天没有改过「${tag}」`;
    return `没有带「${tag}」的备忘录`;
  }
  if (filter === "archived") return "没有归档的备忘录";
  if (filter === "due") return "还没有设到点的备忘录";
  if (filter === "recent") return "最近 7 天没有改过备忘录";
  return "记一条会挂在时间上的笔记";
}

export function activeNoteCount(notes: NoteIndex[]) {
  return notes.filter((note) => !note.archived).length;
}

export function editedLabel(at: number, now: number) {
  const diff = Math.round((startOfDay(at) - startOfDay(now)) / DAY);
  const time = clockTime(at);
  if (diff === 0) return `今天 ${time}`;
  if (diff === -1) return `昨天 ${time}`;
  const date = new Date(at);
  return `${date.getMonth() + 1}/${date.getDate()}`;
}

export function dueLabel(at: number, now: number) {
  const diff = Math.round((startOfDay(at) - startOfDay(now)) / DAY);
  const time = clockTime(at);
  if (diff === 0) return `今天 ${time}`;
  if (diff === 1) return `明天 ${time}`;
  if (diff === -1) return `昨天 ${time}`;
  const date = new Date(at);
  return `${date.getMonth() + 1}/${date.getDate()} ${time}`;
}

export function savedLabel(at: number) {
  return clockTime(at);
}

export function describeDoc(doc: NoteNode | null | undefined) {
  const attachments: string[] = [];
  let thumb: string | undefined;
  const chunks: string[] = [];
  walk(doc);
  const text = clipBytes(chunks.join(""), 4096);
  const summary = clipChars(text.replace(/\s+/g, " ").trim(), 140);
  return { text, summary, attachments, thumb };

  function walk(node?: NoteNode | null) {
    if (!node) return;
    if (node.type === "text" && node.text) {
      chunks.push(node.text);
      return;
    }
    if (node.type === "hardBreak") {
      chunks.push("\n");
      return;
    }
    if (node.type === "image") {
      const hash = typeof node.attrs?.hash === "string" ? node.attrs.hash : "";
      if (hash) {
        if (!attachments.includes(hash)) attachments.push(hash);
        thumb ??= hash;
      }
      chunks.push("[图片]");
    }
    if (node.type === "noteRef") {
      chunks.push(typeof node.attrs?.title === "string" ? node.attrs.title : "");
    }
    for (const child of node.content ?? []) walk(child);
    if (node.type === "paragraph" || node.type === "heading" || node.type === "blockquote" || node.type === "codeBlock" || node.type === "listItem" || node.type === "taskItem") {
      chunks.push("\n");
    }
  }
}

export function countImages(doc: NoteNode | null | undefined) {
  let count = 0;
  function walk(node?: NoteNode | null) {
    if (!node) return;
    if (node.type === "image") count += 1;
    for (const child of node.content ?? []) walk(child);
  }
  walk(doc);
  return count;
}

export function docBytes(doc: unknown) {
  return new TextEncoder().encode(JSON.stringify(doc ?? null)).length;
}

export function clipChars(value: string, max: number) {
  return [...value].slice(0, max).join("");
}

export function clipBytes(value: string, max: number) {
  let out = "";
  for (const ch of value) {
    if (new TextEncoder().encode(out + ch).length > max) break;
    out += ch;
  }
  return out;
}

export function noteReminderHits(notes: NoteIndex[], now: number): RemindedHit[] {
  const hits: RemindedHit[] = [];
  for (const note of notes) {
    if (!note.reminded || note.archived || note.dueAt == null || note.dueAt > now) continue;
    hits.push({
      key: `note:${note.id}:${note.dueAt}`,
      title: `备忘录 · ${note.title || "未命名"}`,
      detail: note.summary || stageLong(note.dueAt, now),
      time: clockTime(note.dueAt),
      noteId: note.id,
    });
  }
  return hits;
}
