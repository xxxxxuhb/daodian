import { ref } from "vue";
import { loadTemplates } from "../api/library";
import { errorText } from "../domain/feedback";
import type { PlanTemplate } from "../domain/types";
import { BUILTIN_PLAN_TEMPLATES } from "../features/plans/logic";

const templates = ref<PlanTemplate[]>(BUILTIN_PLAN_TEMPLATES.map((item) => ({ ...item, stages: item.stages.map((stage) => ({ ...stage })) })));
const templateError = ref("");

export function useTemplates() {
  async function reloadTemplates() {
    try {
      templates.value = await loadTemplates();
      templateError.value = "";
    } catch (error) {
      templateError.value = errorText(error);
    }
  }

  return { templates, templateError, reloadTemplates };
}
