<script setup lang="ts">
import { computed, inject, ref } from "vue";
import type { NoteIndex } from "../../domain/notes";
import Icon from "../../ui/Icon.vue";
import { mediaDirKey } from "./open";
import { dueLabel, editedLabel } from "./logic";
import { mediaUrl } from "./upload";

const props = defineProps<{ note: NoteIndex; active: boolean; now: number }>();
const emit = defineEmits<{
  open: [];
  pin: [];
  archive: [];
  remove: [];
  export: ["md" | "html"];
}>();

const dir = inject(mediaDirKey, ref(""));
const broken = ref(false);
const thumb = computed(() => (props.note.thumb ? mediaUrl(dir.value, `${props.note.thumb}@480.webp`) : ""));
</script>

<template>
  <article class="ncard" :class="{ on: active, 'is-archived': note.archived, 'is-pinned': note.pinned }" @click="emit('open')">
    <div class="ncard__body">
      <b>{{ note.title || "未命名" }}</b>
      <p>{{ note.summary || "还没有正文" }}</p>
      <div class="meta">
        <span v-if="note.tags[0]" class="tag">#{{ note.tags[0] }}</span>
        <span>{{ editedLabel(note.updatedAt, now) }}</span>
        <template v-if="note.dueAt">
          <span>·</span>
          <em>{{ dueLabel(note.dueAt, now) }}</em>
        </template>
      </div>
    </div>
    <img v-if="thumb && !broken" class="cover" :src="thumb" alt="" @error="broken = true" />
    <div class="more-wrap" @click.stop>
      <span v-if="note.pinned" class="ncard__pin" title="置顶"><Icon name="pin" /></span>
      <button type="button" class="more" aria-label="更多">⋯</button>
      <div class="note-menu card-menu">
        <button type="button" @click="emit('pin')">{{ note.pinned ? "取消置顶" : "置顶" }}</button>
        <button type="button" @click="emit('archive')">{{ note.archived ? "取消归档" : "归档" }}</button>
        <button type="button" @click="emit('export', 'md')">导出 Markdown</button>
        <button type="button" class="danger" @click="emit('remove')">删除</button>
      </div>
    </div>
  </article>
</template>
