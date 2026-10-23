import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";
import { loadLibrary } from "../api/library";
import { errorText } from "../domain/feedback";
import type { Plan, Todo } from "../domain/types";

const todos = ref<Todo[]>([]);
const plans = ref<Plan[]>([]);
const loading = ref(true);
const loadError = ref("");

export function useLibrary() {
  async function reload() {
    try {
      const data = await loadLibrary();
      todos.value = data.todos;
      plans.value = data.plans;
      loadError.value = "";
      // 数据一变就重算一遍提醒，别等腰斩到下一轮轮询。
      invoke("sync_reminders").catch(() => {});
    } catch (error) {
      loadError.value = errorText(error);
    } finally {
      loading.value = false;
    }
  }

  return { todos, plans, loading, loadError, reload };
}
