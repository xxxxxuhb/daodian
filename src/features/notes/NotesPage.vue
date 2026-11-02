<script setup lang="ts">
import { save } from "@tauri-apps/plugin-dialog";
import { computed, inject, onMounted, onUnmounted, provide, ref, watch } from "vue";
import { exportNote } from "../../api/notes";
import { useNow } from "../../domain/clock";
import { errorText } from "../../domain/feedback";
import type { NoteNode } from "../../domain/notes";
import { clockFace } from "../../domain/present";
import { createTickKey } from "../shell/create";
import { tierKey } from "../shell/tier";
import NoteEditor from "./NoteEditor.vue";
import NoteList from "./NoteList.vue";
import { emptyNoteCopy, filterNotes, PAGE_AFTER, PAGE_SIZE, activeNoteCount, type NoteFilter } from "./logic";
import { mediaDirKey } from "./open";
import { useNotes } from "./useNotes";

const props = defineProps<{ query: string; tag?: string; focus: { id: string; token: number } | null }>();

const now = useNow();
const tier = inject(tierKey, computed(() => "standard" as const));
const tick = inject(createTickKey, ref(0));
const { notes, attachmentsDir, activeId, current, loadError, saveState, saveError, savedAt, open, create, edit, flush, updateMeta, remove } = useNotes();
provide(mediaDirKey, attachmentsDir);

const filter = ref<NoteFilter>("all");
const localQuery = ref("");
const page = ref(0);
const miniEditing = ref(false);
const focusTitle = ref(false);
const actionError = ref("");
const face = computed(() => clockFace(now.value));
const keyword = computed(() => localQuery.value.trim() || props.query.trim());
const filtered = computed(() => filterNotes(notes.value, filter.value, keyword.value, now.value, props.tag ?? ""));
const pageCount = computed(() => (filtered.value.length > PAGE_AFTER ? Math.ceil(filtered.value.length / PAGE_SIZE) : 1));
const visible = computed(() => {
  if (filtered.value.length <= PAGE_AFTER) return filtered.value;
  const start = page.value * PAGE_SIZE;
  return filtered.value.slice(start, start + PAGE_SIZE);
});
const empty = computed(() => emptyNoteCopy(filter.value, keyword.value, props.tag ?? ""));
const count = computed(() => activeNoteCount(notes.value));
const showList = computed(() => tier.value !== "mini" || !miniEditing.value);
const showEditor = computed(() => !!current.value && (tier.value !== "mini" || miniEditing.value));

watch(
  () => props.query,
  (value) => {
    if (value) localQuery.value = value;
  },
);
watch([filter, localQuery, () => props.tag], () => {
  page.value = 0;
});
watch(tick, () => {
  void make(true);
});
watch(
  () => props.focus?.token,
  () => {
    if (!props.focus) return;
    void select(props.focus.id);
  },
  { immediate: true },
);

async function make(focus = false) {
  actionError.value = "";
  try {
    await create();
    focusTitle.value = focus;
    miniEditing.value = true;
  } catch (error) {
    actionError.value = errorText(error);
  }
}

async function select(id: string) {
  actionError.value = "";
  try {
    await open(id);
    miniEditing.value = tier.value === "mini";
    if (keyword.value) focusTitle.value = false;
  } catch (error) {
    actionError.value = errorText(error);
  }
}

async function onExport(id: string, format: "md" | "html") {
  actionError.value = "";
  try {
    await flush();
    const note = notes.value.find((item) => item.id === id);
    const path = await save({
      defaultPath: `${note?.title || "备忘录"}.${format === "html" ? "html" : "md"}`,
      filters: [{ name: format === "html" ? "HTML" : "Markdown", extensions: [format === "html" ? "html" : "md"] }],
    });
    if (!path) return;
    await exportNote(id, format, path);
  } catch (error) {
    actionError.value = errorText(error);
  }
}

function togglePinned(id: string) {
  const note = notes.value.find((item) => item.id === id);
  if (!note) return;
  void updateMeta(id, { pinned: !note.pinned }).catch((error) => {
    actionError.value = errorText(error);
  });
}

function toggleArchived(id: string) {
  const note = notes.value.find((item) => item.id === id);
  if (!note) return;
  void updateMeta(id, { archived: !note.archived }).catch((error) => {
    actionError.value = errorText(error);
  });
}

async function onRemove(id: string) {
  actionError.value = "";
  try {
    await remove(id);
    if (activeId.value == null) miniEditing.value = false;
  } catch (error) {
    actionError.value = errorText(error);
  }
}

function onKey(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    void flush();
  }
}

onMounted(() => window.addEventListener("keydown", onKey));
onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  void flush();
});
</script>

<template>
  <section class="notes-root" :class="{ 'is-mini': tier === 'mini' }">
    <header v-if="showList" class="notes-head">
      <h1>备忘录</h1>
      <span class="num">{{ count }}</span>
      <time>{{ face.date }} {{ face.rest }}</time>
    </header>
    <p v-if="loadError || actionError" class="banner notes-banner">{{ actionError || loadError }}</p>
    <div class="notes-body">
      <NoteList
        v-show="showList"
        :notes="visible"
        :filter="filter"
        :query="localQuery"
        :active-id="activeId"
        :now="now"
        :total="filtered.length"
        :page="page"
        :pages="pageCount"
        :empty="empty"
        @update:filter="filter = $event"
        @update:query="localQuery = $event"
        @update:page="page = $event"
        @open="select"
        @create="make(true)"
        @pin="togglePinned"
        @archive="toggleArchived"
        @remove="onRemove"
        @export="onExport"
      />
      <NoteEditor
        v-if="showEditor && current"
        :note="current"
        :save-state="saveState"
        :save-error="saveError"
        :saved-at="savedAt"
        :focus-title="focusTitle"
        :query="keyword"
        @update:title="edit({ title: $event })"
        @update:doc="edit({ doc: $event as NoteNode })"
        @update:tags="edit({ tags: $event })"
        @update:due="edit({ dueAt: $event })"
        @toggle-pin="edit({ pinned: !current.pinned })"
        @toggle-archive="edit({ archived: !current.archived })"
        @remove="onRemove(current.id)"
        @export="onExport(current.id, $event)"
        @back="miniEditing = false"
        @focus-consumed="focusTitle = false"
        @error="actionError = $event"
      />
      <div v-else-if="tier !== 'mini'" class="note-placeholder">
        <p>选一篇备忘录，或新建一篇。</p>
        <button type="button" class="btn btn--primary btn--sm" @click="make(true)">新建</button>
      </div>
    </div>
  </section>
</template>
