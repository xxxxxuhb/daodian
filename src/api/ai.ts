import { invoke } from "@tauri-apps/api/core";

export interface ParsedTodo {
  title: string;
  dueAt: number | null;
}

export interface GeneratedPlan {
  title: string;
  note: string;
  priority: "low" | "normal" | "high";
  stages: { title: string; at: number | null }[];
}

function context() {
  return {
    nowMs: Date.now(),
    timezoneOffsetMinutes: -new Date().getTimezoneOffset(),
  };
}

export function testDeepSeek() {
  return invoke<string>("test_deepseek");
}

export function parseTodo(text: string) {
  return invoke<ParsedTodo>("ai_parse_todo", { text, ...context() });
}

export function generatePlan(text: string) {
  return invoke<GeneratedPlan>("ai_generate_plan", { text, ...context() });
}
