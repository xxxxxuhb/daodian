import { ref } from "vue";
import { errorText } from "./feedback";

export function useRun() {
  const saving = ref(false);
  const error = ref("");

  async function run(task: () => Promise<void>) {
    if (saving.value) return false;
    saving.value = true;
    error.value = "";
    try {
      await task();
      return true;
    } catch (caught) {
      error.value = errorText(caught);
      return false;
    } finally {
      saving.value = false;
    }
  }

  return { saving, error, run };
}
