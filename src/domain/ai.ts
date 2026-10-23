import type { GeneratedPlan, ParsedTodo } from "../api/ai";
import type { Priority } from "./types";

export const AI_TODO_MIN_LENGTH = 6;

export function shouldParseTodo(
  text: string,
  enabled: boolean,
  hasApiKey: boolean,
) {
  return enabled && hasApiKey && text.trim().length >= AI_TODO_MIN_LENGTH;
}

export function todoComposerAction(event: {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
}, hasPreview: boolean): "accept" | "commit" | null {
  if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return null;
  if (event.key === "Tab" && hasPreview) return "accept";
  if (event.key === "Enter") return "commit";
  return null;
}

export function applyTodoPreview(preview: ParsedTodo) {
  return {
    title: preview.title.trim(),
    chip: preview.dueAt == null ? "none" as const : "custom" as const,
    custom: preview.dueAt == null ? "" : toLocalInput(preview.dueAt),
  };
}

export function generatedPlanFields(
  plan: GeneratedPlan,
  keyFactory: () => string,
): {
  title: string;
  note: string;
  priority: Priority;
  stages: {
    key: string;
    id: null;
    title: string;
    at: number | null;
    done: false;
    dependsOn: null;
    completedAt: null;
  }[];
} {
  return {
    title: plan.title.trim(),
    note: plan.note.trim(),
    priority: plan.priority,
    stages: plan.stages.map((stage) => ({
      key: keyFactory(),
      id: null,
      title: stage.title.trim(),
      at: stage.at,
      done: false,
      dependsOn: null,
      completedAt: null,
    })),
  };
}

function toLocalInput(at: number) {
  const date = new Date(at);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
