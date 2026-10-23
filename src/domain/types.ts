import type { Repeat } from "./repeat";

export type { Repeat } from "./repeat";

export type Priority = "high" | "normal" | "low";

export interface Todo {
  id: string;
  title: string;
  note: string;
  dueAt: number | null;
  priority: Priority;
  done: boolean;
  createdAt: number;
  updatedAt: number;
  reminded: boolean;
  repeat: Repeat | null;
  leadMs: number;
  leadReminded: boolean;
}

export interface Stage {
  id: string;
  title: string;
  at: number | null;
  done: boolean;
  reminded: boolean;
}

export interface Plan {
  id: string;
  title: string;
  note: string;
  priority: Priority;
  stages: Stage[];
  done: boolean;
  createdAt: number;
  updatedAt: number;
  templateId: string | null;
}

export interface Library {
  todos: Todo[];
  plans: Plan[];
}

export interface TodoInput {
  id: string | null;
  title: string;
  note: string;
  dueAt: number | null;
  priority: Priority;
  repeat: Repeat | null;
  leadMs: number;
}

export interface StageInput {
  id: string | null;
  title: string;
  at: number | null;
  done: boolean;
}

export interface PlanInput {
  id: string | null;
  title: string;
  note: string;
  priority: Priority;
  stages: StageInput[];
  templateId: string | null;
}
