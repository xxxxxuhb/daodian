<script setup lang="ts">
import { computed } from "vue";
import { useLibrary } from "../../data/useLibrary";
import { useNow } from "../../domain/clock";
import { clockTime, todayProgress, upcoming } from "../../domain/present";

const now = useNow();
const { todos, plans } = useLibrary();
const progress = computed(() => todayProgress(todos.value, plans.value, now.value));
const nextUp = computed(() => upcoming(todos.value, plans.value, now.value));
const ratio = computed(() => `${Math.round(progress.value.ratio * 100)}%`);
</script>

<template>
  <div class="strip">
    <span>今天</span>
    <b>{{ progress.done }} / {{ progress.total }}</b>
    <span class="bar"><i :style="{ width: ratio }"></i></span>
    <span class="next">
      下一个
      <b>{{ nextUp ? clockTime(nextUp.at) : "没有了" }}</b>
      <template v-if="nextUp"> · {{ nextUp.title }}</template>
    </span>
  </div>
</template>
