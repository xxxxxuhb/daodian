import { invoke } from "@tauri-apps/api/core";
import type { Library, Plan, PlanInput, Todo, TodoInput } from "../domain/types";

export function loadLibrary() {
  return invoke<Library>("load_library");
}

export function saveTodo(input: TodoInput) {
  return invoke<Todo>("save_todo", { input });
}

export function setTodoDone(id: string, done: boolean) {
  return invoke<Todo>("set_todo_done", { id, done });
}

export function deleteTodo(id: string) {
  return invoke<void>("delete_todo", { id });
}

export function savePlan(input: PlanInput) {
  return invoke<Plan>("save_plan", { input });
}

export function setPlanDone(id: string, done: boolean) {
  return invoke<Plan>("set_plan_done", { id, done });
}

export function setStageDone(planId: string, stageId: string, done: boolean) {
  return invoke<Plan>("set_stage_done", { planId, stageId, done });
}

export function deletePlan(id: string) {
  return invoke<void>("delete_plan", { id });
}
