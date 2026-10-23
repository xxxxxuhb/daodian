<script setup lang="ts">
import { computed, inject, ref, watch } from "vue";
import { tierKey } from "../shell/tier";
import { deleteTodo, saveTodo, setTodoDone } from "../../api/library";
import { useLibrary } from "../../data/useLibrary";
import { useNow } from "../../domain/clock";
import {
  atClock,
  clockFace,
  clockTime,
  countdown,
  fromLocalInput,
  weekSummary,
  splitToday,
  todayProgress,
  todayTimeline,
  upcoming,
} from "../../domain/present";
import { startOfDay } from "../../domain/time";
import { useRun } from "../../domain/useRun";
import Icon from "../../ui/Icon.vue";
import TodayStrip from "../shell/TodayStrip.vue";
import { createTickKey } from "../shell/create";
import {
  compareTodos,
  emptyTodoCopy,
  emptyTodoDraft,
  matchesTodo,
  TODO_FILTERS,
  toTodoDraft,
  todoMatchesQuery,
  type TodoDraft,
  type TodoFilter,
} from "./logic";
import TodoEditor from "./TodoEditor.vue";
import TodoRow from "./TodoRow.vue";

const props = defineProps<{ filter: TodoFilter; query: string; loadError: string }>();
const emit = defineEmits<{ "update:filter": [TodoFilter] }>();

const now = useNow();
const { todos, plans, loading, reload } = useLibrary();
const { saving, error, run } = useRun();
const face = computed(() => clockFace(now.value));
const heading = computed(() => TODO_FILTERS.find((item) => item.id === props.filter) ?? TODO_FILTERS[0]!);
const view = ref<"list" | "agenda">("list");
const chip = ref<"none" | "today15" | "tomorrow9" | "custom">("today15");
const custom = ref("");
const titleText = ref("");
const composer = ref<HTMLInputElement | null>(null);
const draft = ref<TodoDraft | null>(null);
const weekOpen = ref(false);
const weekText = ref("");
const copied = ref(false);
const week = computed(() => weekSummary(todos.value, plans.value, now.value));
const createTick = inject(createTickKey, null);
const tier = inject(tierKey, computed(() => "wide" as const));

const visible = computed(() =>
  todos.value
    .filter((todo) => matchesTodo(todo, props.filter, now.value) && todoMatchesQuery(todo, props.query))
    .sort(compareTodos),
);
const groups = computed(() => splitToday(visible.value, now.value));
const agenda = computed(() => visible.value.filter((todo) => todo.dueAt != null));
const unset = computed(() => visible.value.filter((todo) => todo.dueAt == null));
const nowId = computed(() => agenda.value.find((todo) => (todo.dueAt ?? 0) >= now.value)?.id ?? null);
const progress = computed(() => todayProgress(todos.value, now.value));
const timeline = computed(() => todayTimeline(todos.value, plans.value, now.value));
const nowIndex = computed(() => {
  const index = timeline.value.findIndex((item) => item.at >= now.value);
  return index === -1 ? timeline.value.length : index;
});
const keyId = computed(() => timeline.value.find((item) => !item.done && item.at >= now.value)?.key ?? "");
const next = computed(() => upcoming(todos.value, plans.value, now.value));
const points = computed(() => {
  const start = startOfDay(now.value);
  return todos.value.filter(
    (todo) => !todo.done && todo.dueAt != null && todo.dueAt >= now.value && todo.dueAt < start + 86_400_000,
  ).length;
});
const empty = computed(() => !loading.value && visible.value.length === 0);

watch(weekOpen, (open) => {
  if (open) weekText.value = week.value.text;
});

async function copyWeek() {
  await navigator.clipboard.writeText(weekText.value);
  copied.value = true;
  window.setTimeout(() => {
    copied.value = false;
  }, 2000);
}

watch(
  () => createTick?.value,
  () => {
    if (draft.value) return;
    composer.value?.focus();
  },
);

function chosenDue() {
  if (chip.value === "today15") return atClock(now.value, 0, 15, 0);
  if (chip.value === "tomorrow9") return atClock(now.value, 1, 9, 0);
  if (chip.value === "custom") return fromLocalInput(custom.value);
  return null;
}

async function commit() {
  const title = titleText.value.trim();
  if (!title) {
    error.value = "先写个标题";
    composer.value?.focus();
    return;
  }
  const dueAt = chosenDue();
  if (chip.value === "custom" && dueAt == null) {
    error.value = "选一个时间";
    return;
  }
  const ok = await run(async () => {
    await saveTodo({ id: null, title, note: "", dueAt, priority: "normal", repeat: null, leadMs: 0 });
    await reload();
  });
  if (!ok) return;
  titleText.value = "";
  custom.value = "";
}

async function toggle(id: string, done: boolean) {
  await run(async () => {
    await setTodoDone(id, done);
    await reload();
  });
}

function openTodo(id: string) {
  const todo = todos.value.find((item) => item.id === id);
  if (todo) draft.value = toTodoDraft(todo);
}

async function saveDraft() {
  const current = draft.value;
  if (!current) return;
  const ok = await run(async () => {
    await saveTodo({
      id: current.id,
      title: current.title,
      note: current.note,
      dueAt: current.dueAt,
      priority: current.priority,
      repeat:
        current.repeat && current.dueAt != null
          ? {
              rule: current.repeat.rule,
              weekday: current.repeat.rule === "weekly" ? new Date(current.dueAt).getDay() : null,
            }
          : null,
      leadMs: current.leadMs,
    });
    await reload();
  });
  if (ok) draft.value = null;
}

async function removeDraft() {
  const id = draft.value?.id;
  if (!id) return;
  const ok = await run(async () => {
    await deleteTodo(id);
    await reload();
  });
  if (ok) draft.value = null;
}

async function toggleDraft() {
  const current = draft.value;
  if (!current?.id) return;
  const ok = await run(async () => {
    const saved = await setTodoDone(current.id!, !current.done);
    await reload();
    if (draft.value?.id === saved.id) draft.value.done = saved.done;
  });
  if (!ok) return;
}
</script>

<template>
  <main :class="tier === 'mini' ? 'mini-body' : tier === 'standard' ? 'std-main' : 'main'">
    <header v-if="tier !== 'mini'" :class="tier === 'standard' ? 'std-head' : 'topbar'">
      <div>
        <p class="eyebrow">待办</p>
        <h1>{{ heading.label }}</h1>
        <p v-if="tier === 'wide'" class="sub">{{ heading.hint }}</p>
      </div>
      <div :class="tier === 'standard' ? 'right' : 'topbar__right'">
        <div class="date"><b>{{ face.date }}</b>{{ face.rest }}</div>
        <div class="seg">
          <button type="button" :class="{ on: view === 'list' }" @click="view = 'list'">列表</button>
          <button type="button" :class="{ on: view === 'agenda' }" @click="view = 'agenda'">日程</button>
        </div>
        <button class="btn btn--primary" :class="{ 'btn--sm': tier === 'standard' }" type="button" @click="draft = emptyTodoDraft()">
          <Icon name="plus" small />新建
        </button>
      </div>
    </header>
    <div v-else class="mini-title">
      <h1>{{ heading.label }}</h1>
      <span class="today-fig">{{ progress.done }} / {{ progress.total }}</span>
      <span class="grow"></span>
      <span>{{ clockTime(now) }}</span>
    </div>
    <TodayStrip v-if="tier === 'standard'" />

    <div v-if="tier === 'mini'" class="mini-add">
      <div class="mini-add__line">
        <span class="mini-add__dot"></span>
        <input
          ref="composer"
          v-model="titleText"
          maxlength="80"
          placeholder="记一件…"
          @keydown.enter.prevent="commit"
        />
        <button class="mini-add__go" type="button" :disabled="saving" @click="commit">
          <Icon name="plus" />
        </button>
      </div>
      <div class="mini-add__row">
        <button type="button" class="chip" :class="{ on: chip === 'none' }" @click="chip = 'none'">不设时间</button>
        <button type="button" class="chip" :class="{ on: chip === 'today15' }" @click="chip = 'today15'">今天 15:00</button>
        <button type="button" class="chip" :class="{ on: chip === 'tomorrow9' }" @click="chip = 'tomorrow9'">明天 09:00</button>
        <button type="button" class="chip" :class="{ on: chip === 'custom' }" @click="chip = 'custom'">自选</button>
      </div>
      <input v-if="chip === 'custom'" v-model="custom" class="chip-time mini-custom-time" type="datetime-local" />
    </div>

    <div :class="tier === 'mini' ? 'mini-scroll' : 'main__scroll'">
      <p v-if="error || loadError" class="banner">{{ error || loadError }}</p>
      <div v-if="tier !== 'mini'" class="composer">
        <div class="composer__line">
          <span class="composer__dot"></span>
          <input
            ref="composer"
            v-model="titleText"
            maxlength="80"
            placeholder="记一件临时的事，按回车定个时间点"
            @keydown.enter.prevent="commit"
          />
          <span class="kbd">Ctrl N</span>
        </div>
        <div class="composer__row">
          <button type="button" class="chip" :class="{ on: chip === 'none' }" @click="chip = 'none'">不设时间</button>
          <button type="button" class="chip" :class="{ on: chip === 'today15' }" @click="chip = 'today15'">今天 15:00</button>
          <button type="button" class="chip" :class="{ on: chip === 'tomorrow9' }" @click="chip = 'tomorrow9'">明天 09:00</button>
          <button type="button" class="chip" :class="{ on: chip === 'custom' }" @click="chip = 'custom'">
            <Icon name="cal" small />自选
          </button>
          <input v-if="chip === 'custom'" v-model="custom" class="chip-time" type="datetime-local" />
          <span class="grow"></span>
          <button class="btn btn--primary btn--sm" type="button" :disabled="saving" @click="commit">记下</button>
        </div>
      </div>

      <p v-if="loading && visible.length === 0" class="empty">正在读取…</p>
      <p v-else-if="empty" class="empty">{{ emptyTodoCopy(filter, Boolean(query)) }}</p>

      <template v-else-if="filter === 'today' && view === 'list'">
        <template v-if="groups.past.length">
          <div :class="tier === 'mini' ? 'mini-group' : 'group'">
            <h2>已过时间</h2>
            <i class="cnt">{{ groups.past.length }}</i>
            <span class="rule"></span>
            <button class="act" type="button" @click="emit('update:filter', 'overdue')">全部逾期 →</button>
          </div>
          <div :class="tier === 'mini' ? 'mini-list' : 'list'">
            <TodoRow
              v-for="todo in groups.past"
              :key="todo.id"
              :todo="todo"
              :now="now"
              @open="openTodo(todo.id)"
              @toggle="toggle(todo.id, !todo.done)"
            />
          </div>
        </template>
        <template v-if="groups.later.length">
          <div :class="tier === 'mini' ? 'mini-group' : 'group'">
            <h2>今天</h2>
            <i class="cnt">{{ groups.later.length }}</i>
            <span class="rule"></span>
          </div>
          <div :class="tier === 'mini' ? 'mini-list' : 'list'">
            <TodoRow
              v-for="todo in groups.later"
              :key="todo.id"
              :todo="todo"
              :now="now"
              @open="openTodo(todo.id)"
              @toggle="toggle(todo.id, !todo.done)"
            />
          </div>
        </template>
        <template v-if="groups.unset.length">
          <div :class="tier === 'mini' ? 'mini-group' : 'group'">
            <h2>没定时间</h2>
            <i class="cnt">{{ groups.unset.length }}</i>
            <span class="rule"></span>
          </div>
          <div :class="tier === 'mini' ? 'mini-list' : 'list'">
            <TodoRow
              v-for="todo in groups.unset"
              :key="todo.id"
              :todo="todo"
              :now="now"
              @open="openTodo(todo.id)"
              @toggle="toggle(todo.id, !todo.done)"
            />
          </div>
        </template>
      </template>

      <template v-else>
        <div v-if="agenda.length" :class="tier === 'mini' ? 'mini-list' : 'list'">
          <template v-for="todo in agenda" :key="todo.id">
            <div v-if="view === 'agenda' && todo.id === nowId" class="nowline"><i>现在 {{ clockTime(now) }}</i><span></span></div>
            <TodoRow :todo="todo" :now="now" @open="openTodo(todo.id)" @toggle="toggle(todo.id, !todo.done)" />
          </template>
        </div>
        <template v-if="unset.length">
          <div :class="tier === 'mini' ? 'mini-group' : 'group'"><h2>没定时间</h2><i class="cnt">{{ unset.length }}</i><span class="rule"></span></div>
          <div :class="tier === 'mini' ? 'mini-list' : 'list'">
            <TodoRow
              v-for="todo in unset"
              :key="todo.id"
              :todo="todo"
              :now="now"
              @open="openTodo(todo.id)"
              @toggle="toggle(todo.id, !todo.done)"
            />
          </div>
        </template>
      </template>
    </div>

    <div v-if="tier !== 'mini'" class="main-foot">
      <span v-if="filter === 'today'"><b>{{ visible.length }}</b> 件没做完 · 今天还有 {{ points }} 个时间点</span>
      <span v-else-if="filter === 'done'"><b>{{ visible.length }}</b> 件已完成</span>
      <span v-else-if="filter === 'overdue'"><b>{{ visible.length }}</b> 件已经过了点</span>
      <span v-else><b>{{ visible.length }}</b> 件排在以后</span>
      <span class="k">
        <span><span class="kbd">空格</span>完成</span>
        <span><span class="kbd">Enter</span>编辑</span>
        <span><span class="kbd">Ctrl N</span>新建</span>
      </span>
    </div>
    <TodoEditor
      v-if="draft"
      :draft="draft"
      :now="now"
      :saving="saving"
      :error="error"
      @close="draft = null"
      @save="saveDraft"
      @remove="removeDraft"
      @toggle="toggleDraft"
    />
  </main>

  <aside v-if="tier === 'wide'" class="aside">
    <div class="aside__head">
      <p class="eyebrow">今天</p>
      <div class="big"><b>{{ progress.done }}</b><span>/ {{ progress.total }} 件做完</span></div>
      <div class="bar"><i :style="{ width: `${Math.round(progress.ratio * 100)}%` }"></i></div>
    </div>
    <div class="tl">
      <template v-for="(item, index) in timeline" :key="item.key">
        <div v-if="index === nowIndex" class="tl__now"><i>现在 {{ clockTime(now) }}</i><span></span></div>
        <div class="tl__item" :class="{ 'is-done': item.done, 'is-key': item.key === keyId }">
          <i class="tl__t">{{ item.time }}</i>
          <span class="tl__dot"></span>
          <span><span class="nm">{{ item.title }}</span><span class="meta">{{ item.meta }}</span></span>
        </div>
      </template>
      <div v-if="nowIndex === timeline.length" class="tl__now"><i>现在 {{ clockTime(now) }}</i><span></span></div>
    </div>
    <div class="next-card">
      <template v-if="next">
        <p class="eyebrow">下一次提醒</p>
        <h4>{{ next.title }}</h4>
        <p v-if="next.kind === 'stage'">{{ next.context }} · 阶段 {{ next.index }} / {{ next.total }}</p>
        <p v-else>待办 · {{ clockFace(next.at).date }} {{ clockTime(next.at) }}</p>
        <div class="cd"><b>{{ countdown(next.at, now) }}</b><span>后到点</span></div>
      </template>
      <template v-else>
        <p class="eyebrow">下一次提醒</p>
        <h4>没有即将到点的事</h4>
        <p>记下时间点之后，这里会开始倒计时</p>
      </template>
    </div>
    <div class="week-card">
      <button type="button" class="week-card__toggle" @click="weekOpen = !weekOpen">本周</button>
      <div v-if="weekOpen" class="week-card__body">
        <p>{{ week.done }} 件完成 / {{ week.overdue }} 件逾期 / {{ week.advanced }} 个计划有进展</p>
        <textarea v-model="weekText" rows="6"></textarea>
        <button type="button" class="btn btn--sm" @click="copyWeek">{{ copied ? "已复制" : "复制成文本" }}</button>
      </div>
    </div>
  </aside>
</template>
