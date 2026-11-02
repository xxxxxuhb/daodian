<script setup lang="ts">
import { computed, ref } from "vue";
import type { NoteIndex } from "../../domain/notes";
import Icon from "../../ui/Icon.vue";
import NoteCard from "./NoteCard.vue";
import { NOTE_FILTERS, VIRTUAL_AFTER, type NoteFilter } from "./logic";

const props = defineProps<{
  notes: NoteIndex[];
  filter: NoteFilter;
  query: string;
  activeId: string | null;
  now: number;
  total: number;
  page: number;
  pages: number;
  empty: string;
}>();
const emit = defineEmits<{
  "update:filter": [NoteFilter];
  "update:query": [string];
  "update:page": [number];
  open: [string];
  create: [];
  pin: [string];
  archive: [string];
  remove: [string];
  export: [string, "md" | "html"];
}>();

const scrollTop = ref(0);
const row = 86;
const virtual = computed(() => props.notes.length > VIRTUAL_AFTER);
const start = computed(() => (virtual.value ? Math.max(0, Math.floor(scrollTop.value / row) - 3) : 0));
const end = computed(() => (virtual.value ? Math.min(props.notes.length, start.value + 16) : props.notes.length));
const slice = computed(() => props.notes.slice(start.value, end.value));
const padTop = computed(() => (virtual.value ? start.value * row : 0));
const padBottom = computed(() => (virtual.value ? (props.notes.length - end.value) * row : 0));

function onScroll(event: Event) {
  scrollTop.value = (event.target as HTMLElement).scrollTop;
}
</script>

<template>
  <aside class="note-side">
    <label class="note-search">
      <Icon name="search" small />
      <input :value="query" type="search" placeholder="搜标题和正文" @input="emit('update:query', ($event.target as HTMLInputElement).value)" />
    </label>
    <div class="chips">
      <button
        v-for="item in NOTE_FILTERS"
        :key="item.id"
        type="button"
        class="chip"
        :class="{ on: filter === item.id }"
        @click="emit('update:filter', item.id)"
      >
        {{ item.label }}
      </button>
      <button type="button" class="chip create" @click="emit('create')">+ 新建</button>
    </div>
    <div v-if="notes.length" class="note-list" @scroll="onScroll">
      <div :style="{ height: padTop + 'px' }"></div>
      <NoteCard
        v-for="note in slice"
        :key="note.id"
        :note="note"
        :active="note.id === activeId"
        :now="now"
        @open="emit('open', note.id)"
        @pin="emit('pin', note.id)"
        @archive="emit('archive', note.id)"
        @remove="emit('remove', note.id)"
        @export="emit('export', note.id, $event)"
      />
      <div :style="{ height: padBottom + 'px' }"></div>
    </div>
    <div v-else class="note-blank">
      <p>{{ empty }}</p>
      <button type="button" class="btn btn--primary btn--sm" @click="emit('create')">新建</button>
    </div>
    <div v-if="pages > 1" class="pager">
      <button type="button" :disabled="page <= 0" @click="emit('update:page', page - 1)">上一页</button>
      <span>{{ page + 1 }} / {{ pages }}</span>
      <button type="button" :disabled="page + 1 >= pages" @click="emit('update:page', page + 1)">下一页</button>
    </div>
    <p class="note-count">{{ total }} 篇</p>
  </aside>
</template>
