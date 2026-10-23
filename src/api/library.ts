import { invoke } from "@tauri-apps/api/core";
import type {
  Library,
  Plan,
  PlanInput,
  PlanTemplate,
  TemplateInput,
  Todo,
  TodoInput,
} from "../domain/types";

export function loadLibrary() {
  return invoke<Library>("load_library");
}

export function saveTodo(input: TodoInput) {
  return invoke<Todo>("save_todo", { input });
}

export function setTodoDone(id: string, done: boolean) {
  return invoke<Todo>("set_todo_done", { id, done });
}

export function skipTodoOccurrence(id: string) {
  return invoke<Todo>("skip_todo_occurrence", { id });
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

export function loadTemplates() {
  return invoke<PlanTemplate[]>("load_templates");
}

export function saveTemplate(input: TemplateInput) {
  return invoke<PlanTemplate>("save_template", { input });
}

export function renameTemplate(id: string, label: string) {
  return invoke<PlanTemplate>("rename_template", { id, label });
}

export function deleteTemplate(id: string) {
  return invoke<void>("delete_template", { id });
}
