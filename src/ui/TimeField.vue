<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { stageLong } from "../domain/present";
import Icon from "./Icon.vue";

const props = defineProps<{ at: number | null; now: number }>();
const emit = defineEmits<{ "update:at": [number | null] }>();

const openKey = ref<string | null>(null);
const id = `time-${Math.random().toString(36).slice(2, 8)}`;
const open = computed(() => openKey.value === id);

const years = computed(() => {
  const year = new Date(props.now).getFullYear();
  return Array.from({ length: 8 }, (_, index) => year - 1 + index);
});

function shown() {
  const date = new Date(props.at ?? props.now);
  return {
    year: date.getFullYear(),
    month: date.getMonth() + 1,
    day: date.getDate(),
    hour: props.at == null ? 9 : date.getHours(),
    minute: props.at == null ? 0 : date.getMinutes(),
  };
}

function days() {
  const { year, month } = shown();
  return new Date(year, month, 0).getDate();
}

function write(next: ReturnType<typeof shown>) {
  const max = new Date(next.year, next.month, 0).getDate();
  const day = Math.min(next.day, max);
  emit("update:at", new Date(next.year, next.month - 1, day, next.hour, next.minute).getTime());
}

function setPart(key: "year" | "month" | "day" | "hour" | "minute", event: Event) {
  const value = Number((event.target as HTMLSelectElement).value);
  write({ ...shown(), [key]: value });
}

function apply() {
  write(shown());
  openKey.value = null;
}

function toggle() {
  openKey.value = open.value ? null : id;
}

function close() {
  if (openKey.value === id) openKey.value = null;
}

onMounted(() => window.addEventListener("click", close));
onUnmounted(() => window.removeEventListener("click", close));
</script>

<template>
  <div class="stage__when">
    <button class="stage__pick" type="button" @click.stop="toggle">
      <Icon name="cal" small />
      <span>{{ at == null ? "选一个时间" : stageLong(at, now) }}</span>
    </button>
    <div v-if="open" class="timepop" @click.stop>
      <select :value="shown().year" aria-label="年" @change="setPart('year', $event)">
        <option v-for="year in years" :key="year" :value="year">{{ year }}年</option>
      </select>
      <select :value="shown().month" aria-label="月" @change="setPart('month', $event)">
        <option v-for="month in 12" :key="month" :value="month">{{ month }}月</option>
      </select>
      <select :value="shown().day" aria-label="日" @change="setPart('day', $event)">
        <option v-for="day in days()" :key="day" :value="day">{{ day }}日</option>
      </select>
      <select :value="shown().hour" aria-label="时" @change="setPart('hour', $event)">
        <option v-for="hour in 24" :key="hour" :value="hour - 1">{{ String(hour - 1).padStart(2, "0") }}时</option>
      </select>
      <select :value="shown().minute" aria-label="分" @change="setPart('minute', $event)">
        <option v-for="minute in 60" :key="minute" :value="minute - 1">{{ String(minute - 1).padStart(2, "0") }}分</option>
      </select>
      <button class="btn btn--sm" type="button" @click="apply">用这个时间</button>
      <button v-if="at != null" class="btn btn--sm btn--ghost" type="button" @click="emit('update:at', null)">清除</button>
    </div>
  </div>
</template>
