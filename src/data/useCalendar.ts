import { readonly, ref } from "vue";
import { refreshCalendar, type CalendarEvent } from "../api/calendar";
import { errorText } from "../domain/feedback";

const mutableEvents = ref<CalendarEvent[]>([]);
const events = readonly(mutableEvents);
const loading = ref(false);
const error = ref("");

async function refresh(path: string) {
  if (!path.trim()) throw new Error("请先选择 .ics 日历文件");
  loading.value = true;
  try {
    const result = await refreshCalendar(path);
    mutableEvents.value = [...result.events];
    error.value = result.errors.join("；");
    return result;
  } catch (cause) {
    error.value = errorText(cause);
    throw cause;
  } finally {
    loading.value = false;
  }
}

function remove() {
  mutableEvents.value = [];
  error.value = "";
}

export function useCalendar() {
  return { events, loading: readonly(loading), error: readonly(error), refresh, remove };
}
