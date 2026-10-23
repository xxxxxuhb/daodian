<script setup lang="ts">
withDefaults(defineProps<{ size?: number }>(), { size: 38 });
</script>

<template>
  <svg class="mark" :width="size" :height="size" viewBox="0 0 64 64" fill="none" aria-hidden="true">
    <circle cx="32" cy="32" r="18.4" stroke="currentColor" stroke-width="4.8" />
    <path d="M32 32 40.5 23.5" stroke="currentColor" stroke-width="4.6" stroke-linecap="round" />
    <circle class="bead" cx="43.7" cy="20.3" r="6.8" />
  </svg>
</template>
