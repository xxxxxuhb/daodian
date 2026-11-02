<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { computed, nextTick, onMounted, onUnmounted, provide, ref, watch } from "vue";
import { useLibrary } from "../../data/useLibrary";
import { useCalendar } from "../../data/useCalendar";
import { useSettings } from "../../data/useSettings";
import { useNow } from "../../domain/clock";
import Icon from "../../ui/Icon.vue";
import Mark from "../../ui/Mark.vue";
import PlanPage from "../plans/PlanPage.vue";
import { PLAN_FILTERS, planCounts, type PlanFilter } from "../plans/logic";
import NotesPage from "../notes/NotesPage.vue";
import { activeNoteCount, noteTagEntries } from "../notes/logic";
import { openLinkedKey, openNoteKey, type LinkedTarget } from "../notes/open";
import { bindNoteLifecycle, useNotes } from "../notes/useNotes";
import TodoPage from "../todos/TodoPage.vue";
import { TODO_FILTERS, todoBucket, todoCounts, type TodoFilter } from "../todos/logic";
import ReminderToast from "./ReminderToast.vue";
import SettingsDrawer from "../settings/SettingsDrawer.vue";
import { createTickKey } from "./create";
import { setTheme, themeName } from "./theme";
import { tierKey } from "./tier";

const section = ref<"todo" | "plan" | "note">("todo");
const noteFocus = ref<{ id: string; token: number } | null>(null);
const todoFilter = ref<TodoFilter>("today");
const planFilter = ref<PlanFilter>("active");
const planView = ref<"cards" | "gantt">("cards");
const query = ref("");
const searching = ref(false);
const settingsOpen = ref(false);
const searchEl = ref<HTMLInputElement | null>(null);
const createTick = ref(0);
const quickCaptureActive = ref(false);
let captureSection: {
  section: "todo" | "plan" | "note";
  todoFilter: TodoFilter;
  planFilter: PlanFilter;
  planView: "cards" | "gantt";
} | null = null;
provide(createTickKey, createTick);

const now = useNow();
const { todos, plans, loadError, reload } = useLibrary();
const { notes, reload: reloadNotes } = useNotes();
bindNoteLifecycle();
const noteCount = computed(() => activeNoteCount(notes.value));
const noteTag = ref("");
const noteTagOrder = ref<string[]>([]);
const noteTags = computed(() => noteTagEntries(noteTagOrder.value, notes.value));
watch(noteTags, (items) => {
  const next = items.map((item) => item.tag);
  if (next.length !== noteTagOrder.value.length || next.some((tag, index) => tag !== noteTagOrder.value[index])) {
    noteTagOrder.value = next;
  }
  if (noteTag.value && !next.includes(noteTag.value)) noteTag.value = "";
});
const { refresh: refreshCalendar, remove: removeCalendar } = useCalendar();
const { settings, load: loadSettings, update: updateSettings } = useSettings();
const keyword = computed(() => query.value.trim().toLowerCase());
const todoTally = computed(() => todoCounts(todos.value, now.value));
const planTally = computed(() => planCounts(plans.value));
const width = ref(window.innerWidth);
const tier = computed(() => (width.value < 720 ? "mini" : width.value < 1180 ? "standard" : "wide"));
provide(tierKey, tier);
const miniDate = computed(() => {
  const date = new Date(now.value);
  const week = "日一二三四五六"[date.getDay()];
  return `${date.getMonth() + 1}月${date.getDate()}日 周${week}`;
});

function onResize() {
  width.value = window.innerWidth;
}

async function toggleWindow() {
  try {
    await invoke("toggle_window_tier");
  } catch {
    width.value = width.value < 720 ? 900 : 380;
  }
}

function chooseTheme(name: "celadon" | "night") {
  setTheme(name);
  void updateSettings((draft) => {
    draft.theme = name;
  });
}

function openSearch() {
  searching.value = true;
  requestAnimationFrame(() => searchEl.value?.focus());
}

function pickTodo(filter: TodoFilter) {
  section.value = "todo";
  todoFilter.value = filter;
}

function pickPlan(filter: PlanFilter) {
  section.value = "plan";
  planFilter.value = filter;
  planView.value = "cards";
}

function pickTimeline() {
  section.value = "plan";
  planFilter.value = "active";
  planView.value = "gantt";
}

function pickNote(tag = "") {
  section.value = "note";
  noteTag.value = tag;
}

function openNote(id: string) {
  settingsOpen.value = false;
  section.value = "note";
  noteFocus.value = { id, token: Date.now() };
}

function openLinked(target: LinkedTarget) {
  settingsOpen.value = false;
  searching.value = false;
  query.value = target.title;
  if (target.kind === "todo") {
    section.value = "todo";
    const todo = todos.value.find((item) => item.id === target.id);
    todoFilter.value = todo ? todoBucket(todo, now.value) : "today";
    return;
  }
  section.value = "plan";
  planFilter.value = "active";
  planView.value = "cards";
}

provide(openNoteKey, openNote);
provide(openLinkedKey, openLinked);

async function onKey(event: KeyboardEvent) {
  const key = event.key.toLowerCase();
  if (key === "escape" && quickCaptureActive.value) {
    event.preventDefault();
    event.stopPropagation();
    try {
      const restored = await invoke<boolean>("dismiss_quick_capture");
      if (restored && captureSection) {
        section.value = captureSection.section;
        todoFilter.value = captureSection.todoFilter;
        planFilter.value = captureSection.planFilter;
        planView.value = captureSection.planView;
        captureSection = null;
        quickCaptureActive.value = false;
      }
    } catch {
      // 保持快速记录状态，用户可以再次按 Esc。
    }
    return;
  }
  if (!(event.ctrlKey || event.metaKey)) return;
  if (key === "k") {
    event.preventDefault();
    searching.value = true;
    searchEl.value?.focus();
    searchEl.value?.select();
  }
  if (key === "n") {
    event.preventDefault();
    createTick.value += 1;
  }
  if (event.shiftKey && key === "m") {
    event.preventDefault();
    void toggleWindow();
  }
}

let unlisten: UnlistenFn | null = null;
let unlistenQuickCapture: UnlistenFn | null = null;
let unlistenPetTarget: UnlistenFn | null = null;

interface PetOpenTarget {
  kind: "todo" | "stage";
  title: string;
  todoId?: string;
  planId?: string;
  stageId?: string;
}

function openPetTarget(target: PetOpenTarget) {
  query.value = target.title;
  searching.value = false;
  settingsOpen.value = false;
  if (target.kind === "todo") {
    section.value = "todo";
    const todo = todos.value.find((item) => item.id === target.todoId);
    todoFilter.value = todo ? todoBucket(todo, now.value) : "today";
  } else {
    section.value = "plan";
    planFilter.value = "active";
    planView.value = "cards";
  }
}

async function enterQuickCaptureUi() {
  if (quickCaptureActive.value) {
    await nextTick();
    createTick.value += 1;
    return;
  }
  captureSection = {
    section: section.value,
    todoFilter: todoFilter.value,
    planFilter: planFilter.value,
    planView: planView.value,
  };
  quickCaptureActive.value = true;
  settingsOpen.value = false;
  searching.value = false;
  section.value = "todo";
  todoFilter.value = "today";
  await nextTick();
  createTick.value += 1;
}

watch(
  () => settings.value.theme,
  (theme) => setTheme(theme),
  { immediate: true },
);

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("resize", onResize);
  try {
    unlisten = await listen("data-changed", () => {
      reload();
    });
    unlistenQuickCapture = await listen("quick-capture", enterQuickCaptureUi);
    unlistenPetTarget = await listen<PetOpenTarget>("pet-open-target", ({ payload }) => {
      openPetTarget(payload);
    });
    if (await invoke<boolean>("is_quick_capture_active")) {
      await enterQuickCaptureUi();
    }
  } catch {
    // 浏览器里没有 Tauri 事件。
  }

  await Promise.all([reload(), reloadNotes(), loadSettings()]);
  if (settings.value.calendar.enabled && settings.value.calendar.path) {
    try {
      const result = await refreshCalendar(settings.value.calendar.path);
      const status = result.errors.length
        ? `${result.status}：${result.errors.join("；")}`
        : result.status;
      if (status !== settings.value.calendar.lastStatus) {
        void updateSettings((draft) => {
          draft.calendar.lastStatus = status;
        });
      }
    } catch (cause) {
      const status = cause instanceof Error ? cause.message : String(cause);
      void updateSettings((draft) => {
        draft.calendar.lastStatus = status;
      });
    }
  } else {
    removeCalendar();
  }
});

onUnmounted(() => {
  unlisten?.();
  unlistenQuickCapture?.();
  unlistenPetTarget?.();
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("resize", onResize);
});
</script>

<template>
  <div v-if="tier === 'wide'" class="app">
    <aside class="rail">
      <div class="brand">
        <Mark :size="38" />
        <div>
          <b>到点</b>
          <small>临时的事用待办<br />分阶段的用计划</small>
        </div>
      </div>
      <nav class="nav">
        <div class="nav-group">
          <p>待办</p>
          <button
            v-for="item in TODO_FILTERS"
            :key="item.id"
            type="button"
            class="nav-item"
            :class="{ 'is-active': section === 'todo' && todoFilter === item.id }"
            @click="pickTodo(item.id)"
          >
            <span>{{ item.label }}</span>
            <i class="n">{{ todoTally[item.id] }}</i>
          </button>
        </div>
        <div class="nav-group">
          <p>计划</p>
          <button
            v-for="item in PLAN_FILTERS"
            :key="item.id"
            type="button"
            class="nav-item"
            :class="{ 'is-active': section === 'plan' && planFilter === item.id }"
            @click="pickPlan(item.id)"
          >
            <span>{{ item.label }}</span>
            <i class="n">{{ planTally[item.id] }}</i>
          </button>
        </div>
        <div class="nav-group">
          <p>备忘录</p>
          <button type="button" class="nav-item" :class="{ 'is-active': section === 'note' && !noteTag }" @click="pickNote()">
            <span>全部</span>
            <i class="n">{{ noteCount }}</i>
          </button>
          <button
            v-for="item in noteTags"
            :key="item.tag"
            type="button"
            class="nav-item"
            :class="{ 'is-active': section === 'note' && noteTag === item.tag }"
            :title="item.tag"
            @click="pickNote(item.tag)"
          >
            <span>#{{ item.tag }}</span>
            <i class="n">{{ item.count }}</i>
          </button>
        </div>
      </nav>
      <div class="rail-foot">
        <label class="search" :class="{ filled: query.trim() }">
          <Icon name="search" small />
          <input ref="searchEl" v-model="query" type="search" :placeholder="section === 'note' ? '搜索标题或正文' : '搜索标题、备注或阶段'" />
          <span class="q">Ctrl K</span>
        </label>
        <p class="tip">关闭窗口只是收进托盘，<br />提醒会继续跑。</p>
        <div class="themes">
          <button type="button" :class="{ on: themeName === 'celadon' }" @click="chooseTheme('celadon')">青瓷</button>
          <button type="button" :class="{ on: themeName === 'night' }" @click="chooseTheme('night')">夜航</button>
        </div>
        <button type="button" class="settings-trigger tip" @click="settingsOpen = true">设置</button>
      </div>
    </aside>
    <NotesPage v-if="section === 'note'" :query="keyword" :tag="noteTag" :focus="noteFocus" />
    <TodoPage
      v-else-if="section === 'todo'"
      :filter="todoFilter"
      :query="keyword"
      :load-error="loadError"
      @update:filter="todoFilter = $event"
    />
    <PlanPage v-else :filter="planFilter" :query="keyword" :load-error="loadError" :requested-view="planView" />
  </div>

  <div v-else-if="tier === 'standard'" class="std">
    <aside class="rail2">
      <Mark :size="22" />
      <button type="button" class="nav" :class="{ on: section === 'todo' && todoFilter !== 'done' }" @click="pickTodo('today')">
        <Icon name="list" />
        <i class="n">{{ todoTally.today }}</i>
      </button>
      <button type="button" class="nav" :class="{ on: section === 'plan' && planFilter !== 'done' }" @click="pickPlan('active')">
        <Icon name="layers" />
        <i class="n">{{ planTally.active }}</i>
      </button>
      <button type="button" class="nav" :class="{ on: section === 'note' }" aria-label="备忘录" @click="pickNote()">
        <Icon name="note" />
        <i class="n">{{ noteCount }}</i>
      </button>
      <button
        type="button"
        class="nav"
        :class="{ on: (section === 'todo' && todoFilter === 'done') || (section === 'plan' && planFilter === 'done') }"
        @click="section === 'plan' ? pickPlan('done') : pickTodo('done')"
      >
        <Icon name="check" />
      </button>
      <span class="spacer"></span>
      <button type="button" class="nav" @click="openSearch"><Icon name="search" /></button>
      <button type="button" class="nav" @click="chooseTheme(themeName === 'night' ? 'celadon' : 'night')"><Icon name="moon" /></button>
      <button type="button" class="nav" aria-label="设置" @click="settingsOpen = true">⚙</button>
    </aside>
    <div class="std-fill">
      <label v-if="searching" class="std-search">
        <Icon name="search" small />
        <input ref="searchEl" v-model="query" type="search" :placeholder="section === 'note' ? '搜索标题或正文' : '搜索标题、备注或阶段'" @keydown.esc="searching = false" />
      </label>
      <NotesPage v-if="section === 'note'" :query="keyword" :tag="noteTag" :focus="noteFocus" />
      <TodoPage
        v-else-if="section === 'todo'"
        :filter="todoFilter"
        :query="keyword"
        :load-error="loadError"
        @update:filter="todoFilter = $event"
      />
      <PlanPage v-else :filter="planFilter" :query="keyword" :load-error="loadError" :requested-view="planView" />
    </div>
  </div>

  <div v-else class="mini-app">
    <div class="mini-head">
      <Mark :size="19" />
      <b>到点</b>
      <span class="date">{{ miniDate }}</span>
    </div>
    <label v-if="searching" class="std-search mini-search">
      <input ref="searchEl" v-model="query" type="search" :placeholder="section === 'note' ? '搜索备忘录' : '搜索'" @keydown.esc="searching = false" />
    </label>
    <NotesPage v-if="section === 'note'" :query="keyword" :tag="noteTag" :focus="noteFocus" />
    <TodoPage
      v-else-if="section === 'todo'"
      :filter="todoFilter"
      :query="keyword"
      :load-error="loadError"
      @update:filter="todoFilter = $event"
    />
    <PlanPage v-else :filter="planFilter" :query="keyword" :load-error="loadError" :requested-view="planView" />
    <nav class="mini-foot">
      <button type="button" class="tab" :class="{ on: section === 'plan' && planView === 'gantt' }" @click="pickTimeline">
        <Icon name="timeline" /><span>时间轴</span>
      </button>
      <button type="button" class="tab" :class="{ on: section === 'todo' && todoFilter !== 'done' }" @click="pickTodo('today')">
        <Icon name="list" /><span>待办</span>
      </button>
      <button type="button" class="tab" :class="{ on: section === 'plan' && planView === 'cards' }" @click="pickPlan('active')">
        <Icon name="layers" /><span>计划</span>
      </button>
      <button type="button" class="tab" :class="{ on: section === 'note' }" @click="pickNote()">
        <Icon name="note" /><span>备忘录</span>
      </button>
      <button type="button" class="icon-btn" @click="chooseTheme(themeName === 'night' ? 'celadon' : 'night')">
        <Icon name="moon" />
      </button>
      <button type="button" class="icon-btn" aria-label="设置" @click="settingsOpen = true">⚙</button>
    </nav>
  </div>
  <ReminderToast />
  <SettingsDrawer :open="settingsOpen" @close="settingsOpen = false" />
</template>
