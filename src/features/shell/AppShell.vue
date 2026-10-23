<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, onMounted, onUnmounted, provide, ref } from "vue";
import { useLibrary } from "../../data/useLibrary";
import { useNow } from "../../domain/clock";
import Icon from "../../ui/Icon.vue";
import Mark from "../../ui/Mark.vue";
import PlanPage from "../plans/PlanPage.vue";
import { PLAN_FILTERS, planCounts, type PlanFilter } from "../plans/logic";
import TodoPage from "../todos/TodoPage.vue";
import { TODO_FILTERS, todoCounts, type TodoFilter } from "../todos/logic";
import ReminderToast from "./ReminderToast.vue";
import { createTickKey } from "./create";
import { setTheme, themeName } from "./theme";
import { tierKey } from "./tier";

const section = ref<"todo" | "plan">("todo");
const todoFilter = ref<TodoFilter>("today");
const planFilter = ref<PlanFilter>("active");
const query = ref("");
const searching = ref(false);
const searchEl = ref<HTMLInputElement | null>(null);
const createTick = ref(0);
provide(createTickKey, createTick);

const now = useNow();
const { todos, plans, loadError, reload } = useLibrary();
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
  void rememberFrame();
}

async function rememberFrame() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    const factor = await win.scaleFactor();
    const size = await win.innerSize();
    const pos = await win.outerPosition();
    const w = Math.round(size.width / factor);
    const h = Math.round(size.height / factor);
    const name = w < 720 ? "mini" : w < 1180 ? "standard" : "wide";
    const saved = readFrames();
    saved[name] = { w, h, x: Math.round(pos.x / factor), y: Math.round(pos.y / factor) };
    localStorage.setItem("daodian-frames", JSON.stringify(saved));
  } catch {
    // 浏览器预览没有窗口接口。
  }
}

function readFrames(): Record<string, { w: number; h: number; x?: number; y?: number }> {
  try {
    return JSON.parse(localStorage.getItem("daodian-frames") || "{}") as Record<
      string,
      { w: number; h: number; x?: number; y?: number }
    >;
  } catch {
    return {};
  }
}

async function toggleWindow() {
  try {
    const { getCurrentWindow, LogicalPosition, LogicalSize } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    await rememberFrame();
    const factor = await win.scaleFactor();
    const size = await win.innerSize();
    const target = size.width / factor < 720 ? "standard" : "mini";
    const fallback = target === "mini" ? { w: 380, h: 552 } : { w: 900, h: 600 };
    const frame = readFrames()[target] ?? fallback;
    await win.setSize(new LogicalSize(frame.w, frame.h));
    if (frame.x != null && frame.y != null) await win.setPosition(new LogicalPosition(frame.x, frame.y));
    win.show();
  } catch {
    width.value = width.value < 720 ? 900 : 380;
  }
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
}

function onKey(event: KeyboardEvent) {
  const key = event.key.toLowerCase();
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

onMounted(async () => {
  await reload();
  try {
    unlisten = await listen("data-changed", () => {
      reload();
    });
  } catch {
    // 浏览器里没有 Tauri 事件。
  }
  window.addEventListener("keydown", onKey);
  window.addEventListener("resize", onResize);
  if (!localStorage.getItem("daodian-frames") && window.screen.availWidth < 1024) void toggleWindow();
});

onUnmounted(() => {
  unlisten?.();
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
      </nav>
      <div class="rail-foot">
        <label class="search" :class="{ filled: query.trim() }">
          <Icon name="search" small />
          <input ref="searchEl" v-model="query" type="search" placeholder="搜索标题、备注或阶段" />
          <span class="q">Ctrl K</span>
        </label>
        <p class="tip">关闭窗口只是收进托盘，<br />提醒会继续跑。</p>
        <div class="themes">
          <button type="button" :class="{ on: themeName === 'celadon' }" @click="setTheme('celadon')">青瓷</button>
          <button type="button" :class="{ on: themeName === 'night' }" @click="setTheme('night')">夜航</button>
        </div>
      </div>
    </aside>
    <TodoPage
      v-if="section === 'todo'"
      :filter="todoFilter"
      :query="keyword"
      :load-error="loadError"
      @update:filter="todoFilter = $event"
    />
    <PlanPage v-else :filter="planFilter" :query="keyword" :load-error="loadError" />
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
      <button type="button" class="nav" @click="setTheme(themeName === 'night' ? 'celadon' : 'night')"><Icon name="moon" /></button>
    </aside>
    <div class="std-fill">
      <label v-if="searching" class="std-search">
        <Icon name="search" small />
        <input ref="searchEl" v-model="query" type="search" placeholder="搜索标题、备注或阶段" @keydown.esc="searching = false" />
      </label>
      <TodoPage
        v-if="section === 'todo'"
        :filter="todoFilter"
        :query="keyword"
        :load-error="loadError"
        @update:filter="todoFilter = $event"
      />
      <PlanPage v-else :filter="planFilter" :query="keyword" :load-error="loadError" />
    </div>
  </div>

  <div v-else class="mini-app">
    <div class="mini-head">
      <Mark :size="19" />
      <b>到点</b>
      <span class="date">{{ miniDate }}</span>
    </div>
    <label v-if="searching" class="std-search mini-search">
      <input ref="searchEl" v-model="query" type="search" placeholder="搜索" @keydown.esc="searching = false" />
    </label>
    <TodoPage
      v-if="section === 'todo'"
      :filter="todoFilter"
      :query="keyword"
      :load-error="loadError"
      @update:filter="todoFilter = $event"
    />
    <PlanPage v-else :filter="planFilter" :query="keyword" :load-error="loadError" />
    <nav class="mini-foot">
      <button type="button" class="tab" :class="{ on: section === 'todo' && todoFilter !== 'done' }" @click="pickTodo('today')">
        <Icon name="list" /><span>待办</span>
      </button>
      <button type="button" class="tab" :class="{ on: section === 'plan' && planFilter !== 'done' }" @click="pickPlan('active')">
        <Icon name="layers" /><span>计划</span>
      </button>
      <button
        type="button"
        class="tab"
        :class="{ on: (section === 'todo' && todoFilter === 'done') || (section === 'plan' && planFilter === 'done') }"
        @click="section === 'plan' ? pickPlan('done') : pickTodo('done')"
      >
        <Icon name="check" /><span>已完成</span>
      </button>
      <button type="button" class="icon-btn" @click="setTheme(themeName === 'night' ? 'celadon' : 'night')">
        <Icon name="moon" />
      </button>
    </nav>
  </div>
  <ReminderToast />
</template>
