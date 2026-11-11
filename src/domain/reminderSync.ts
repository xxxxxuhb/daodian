export const REMINDER_SETTLED = "reminder-settled";

export interface ReminderSettled {
  key: string;
}

export interface ReminderIdentity {
  key: string;
  todoId?: string;
  planId?: string;
  stageId?: string;
  noteId?: string;
}

interface TodoSnap {
  id: string;
  done: boolean;
  dueAt: number | null;
}

interface StageSnap {
  id: string;
  done: boolean;
  at: number | null;
}

interface PlanSnap {
  id: string;
  done: boolean;
  stages: StageSnap[];
}

interface NoteSnap {
  id: string;
  archived: boolean;
  dueAt?: number | null;
}

function stampedAt(key: string) {
  const at = Number(key.slice(key.lastIndexOf(":") + 1));
  return Number.isFinite(at) ? at : null;
}

/** 弹窗还该留着：事项还在，没完成，而且到点时间没被改掉。 */
export function reminderStillPending(
  hit: ReminderIdentity,
  todos: TodoSnap[],
  plans: PlanSnap[],
  notes: NoteSnap[] = [],
) {
  const at = stampedAt(hit.key);
  if (at == null) return true;
  if (hit.noteId) {
    const note = notes.find((item) => item.id === hit.noteId);
    return !!note && !note.archived && note.dueAt === at;
  }
  if (hit.todoId) {
    const todo = todos.find((item) => item.id === hit.todoId);
    return !!todo && !todo.done && todo.dueAt === at;
  }
  if (hit.planId && hit.stageId) {
    const plan = plans.find((item) => item.id === hit.planId);
    const stage = plan?.stages.find((item) => item.id === hit.stageId);
    return !!plan && !plan.done && !!stage && !stage.done && stage.at === at;
  }
  return true;
}
