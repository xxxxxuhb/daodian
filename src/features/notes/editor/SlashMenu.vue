<script setup lang="ts">
defineProps<{ items: { id: string; label: string }[]; active: number }>();
defineEmits<{ pick: [string] }>();
</script>

<template>
  <div class="slash" role="listbox">
    <button
      v-for="(item, index) in items"
      :key="item.id"
      type="button"
      role="option"
      :class="{ on: index === active }"
      @mousedown.prevent="$emit('pick', item.id)"
    >
      {{ item.label }}
    </button>
  </div>
</template>
