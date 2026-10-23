<script setup lang="ts">
defineProps<{ name: string; small?: boolean }>();
</script>

<template>
  <svg :class="['ico', { s: small }]" aria-hidden="true">
    <use :href="`#i-${name}`" />
  </svg>
</template>
