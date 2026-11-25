<script setup lang="ts">
import { computed } from "vue";
import { hairColor, hairId } from "./look";

const props = defineProps<{
  look?: { hair?: string | null; hairColor?: string | null } | null;
}>();
const hair = computed(() => hairId(props.look?.hair));
const color = computed(() => hairColor(props.look?.hairColor));
</script>

<template>
  <!-- 头发画在本体之后、眼睛之前：盖得住头顶，盖不住眼睛；也吃得到 .pet-creature 的盖章/缩身 -->
  <g
    v-if="hair !== 'none'"
    class="pet-hair"
    :class="'pet-hair--' + hair"
    :style="{ '--pet-hair': color }"
  >
    <template v-if="hair === 'side'">
      <path d="M31.6 50A16.4 16.4 0 0 1 64.4 50Q62 45.2 56.6 43.4Q51 41.6 47.6 43.4Q42 46.4 36.8 44.2Q33.2 46 31.6 50Z" />
    </template>
    <template v-else-if="hair === 'tails'">
      <path d="M33.4 48.6C30.4 51.6 29.6 56.4 30.8 60.8C31.4 63 33.4 63 34 60.8C33.2 57 33.8 53 36.2 50.2Z" />
      <path d="M62.6 48.6C65.6 51.6 66.4 56.4 65.2 60.8C64.6 63 62.6 63 62 60.8C62.8 57 62.2 53 59.8 50.2Z" />
      <path d="M31.8 50.6C31.2 41.6 36.2 34.4 48 34.2C59.8 34.4 64.8 41.6 64.2 50.6C59.4 49 55.2 47.4 48 47.4C40.8 47.4 36.6 49 31.8 50.6Z" />
    </template>
    <template v-else-if="hair === 'spike'">
      <path d="M31.8 50.6C31.2 41.6 36.2 34.4 48 34.2C59.8 34.4 64.8 41.6 64.2 50.6C59.4 49 55.2 47.4 48 47.4C40.8 47.4 36.6 49 31.8 50.6Z" />
      <path d="M35.6 37.4L32.8 31.8L38.6 34.4Z" />
      <path d="M41.4 35.8L40.2 30L44.4 33.6Z" />
      <path d="M60.4 37.4L63.2 31.8L57.4 34.4Z" />
      <path d="M54.6 35.8L55.8 30L51.6 33.6Z" />
    </template>
    <template v-else-if="hair === 'long'">
      <path d="M35 45.6C30.4 49.4 28.6 56 29.2 65.4C29.4 70 30.8 72 32.6 71.6C34.4 71.2 35.2 68.6 35.2 64.4C35.2 57.6 36.4 50.8 39.6 46.4Z" />
      <path d="M61 45.6C65.6 49.4 67.4 56 66.8 65.4C66.6 70 65.2 72 63.4 71.6C61.6 71.2 60.8 68.6 60.8 64.4C60.8 57.6 59.6 50.8 56.4 46.4Z" />
      <path d="M31.6 50A16.4 16.4 0 0 1 64.4 50Q62 45.2 56.6 43.4Q51 41.6 47.6 43.4Q42 46.4 36.8 44.2Q33.2 46 31.6 50Z" />
    </template>
    <template v-else-if="hair === 'asym'">
      <path d="M35 45.6C30.4 49.4 28.6 56 29.2 65.4C29.4 70 30.8 72 32.6 71.6C34.4 71.2 35.2 68.6 35.2 64.4C35.2 57.6 36.4 50.8 39.6 46.4Z" />
      <path d="M31.6 50A16.4 16.4 0 0 1 64.4 50Q62 45.2 56.6 43.4Q51 41.6 47.6 43.4Q42 46.4 36.8 44.2Q33.2 46 31.6 50Z" />
    </template>
    <template v-else-if="hair === 'buns'">
      <path d="M31.8 50C30.8 40 34 33.6 38.4 32.4C39.2 28.6 43 27.8 45 30.4C46.2 32 47 33.4 48 34.6C49 33.4 49.8 32 51 30.4C53 27.8 56.8 28.6 57.6 32.4C62 33.8 65 40 64.2 50C59.4 49 55.2 47.4 48 47.4C40.8 47.4 36.6 49 31.8 50Z" />
    </template>
    <template v-else-if="hair === 'pin'">
      <path d="M31.6 50A16.4 16.4 0 0 1 64.4 50Q62 45.2 56.6 43.4Q51 41.6 47.6 43.4Q42 46.4 36.8 44.2Q33.2 46 31.6 50Z" />
      <g class="pet-hair__pin" transform="rotate(-24 38.6 40.4)">
        <rect x="33" y="38.9" width="11.2" height="3" rx="1.5" fill="#E8DCC0" stroke="#0E1613" stroke-width=".5" />
        <circle cx="33.6" cy="40.4" r="1.9" fill="#D9C79E" stroke="#0E1613" stroke-width=".5" />
      </g>
    </template>
  </g>
</template>
