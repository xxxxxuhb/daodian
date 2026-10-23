<script setup lang="ts">
import { computed, inject } from "vue";
import { relative, rowState, whenParts } from "../../domain/present";
import { repeatLabel } from "../../domain/repeat";
import { isContainerShortcut } from "../../domain/shortcuts";
import type { Todo } from "../../domain/types";
import { tierKey } from "../shell/tier";
import Icon from "../../ui/Icon.vue";

const props = defineProps<{ todo: Todo; now: number }>();
const emit = defineEmits<{ open: []; repeat: []; toggle: [] }>();
const tier = inject(tierKey, computed(() => "wide" as const));

const when = computed(() => whenParts(props.todo.dueAt, props.now));
const pill = computed(() => (props.todo.done || props.todo.dueAt == null ? null : relative(props.todo.dueAt, props.now)));
const repeat = computed(() => (props.todo.repeat ? repeatLabel(props.todo.repeat, props.todo.dueAt) : ""));
const miniState = computed(() => {
  const state = rowState(props.todo, props.now);
  if (state === "is-overdue") return "overdue";
  if (state === "is-soon") return "soon";
  return "";
});

function repeatFromKey(event: KeyboardEvent) {
  if (!isContainerShortcut(event, "r", Boolean(document.querySelector('[role="dialog"]')))) return;
  event.preventDefault();
  emit("repeat");
}
</script>

<template>
  <article
    v-if="tier === 'mini'"
    class="r"
    :class="miniState"
    tabindex="0"
    @click="emit('open')"
    @keydown.enter.prevent="emit('open')"
    @keydown="repeatFromKey"
    @keydown.space.prevent="emit('toggle')"
  >
    <button
      class="r__c"
      :class="{ on: todo.done }"
      type="button"
      :aria-label="todo.done ? '标为未完成' : '标为完成'"
      @click.stop="emit('toggle')"
    ></button>
    <div>
      <div class="r__t">
        <span v-if="todo.priority === 'high'" class="pin"></span>
        {{ todo.title }}
      </div>
      <div v-if="pill" class="r__m" :class="{ red: pill.tone === 'red', amber: pill.tone === 'amber' }">{{ pill.text }}</div>
    </div>
    <span class="r__s">
      {{ when.time }}
      <Icon v-if="todo.repeat" name="repeat" />
    </span>
  </article>
  <article
    v-else
    class="row"
    :class="rowState(todo, now)"
    tabindex="0"
    @click="emit('open')"
    @keydown.enter.prevent="emit('open')"
    @keydown="repeatFromKey"
    @keydown.space.prevent="emit('toggle')"
  >
    <button
      class="check"
      :class="{ 'is-done': todo.done }"
      type="button"
      :aria-label="todo.done ? '标为未完成' : '标为完成'"
      @click.stop="emit('toggle')"
      @keydown.space.stop
      @keydown.enter.stop
    >
      <Icon v-if="todo.done" name="check" small />
    </button>
    <div class="when">
      <span class="t">{{ when.time }}</span>
      <span class="d">{{ when.day }}</span>
    </div>
    <div>
      <h3>
        <i v-if="todo.priority === 'high'" class="dotpin"></i>
        <i v-else-if="todo.priority === 'low'" class="dotpin low"></i>
        {{ todo.title }}
      </h3>
      <p v-if="todo.note">{{ todo.note }}</p>
    </div>
    <div class="row__side">
      <span v-if="todo.skipped" class="pill pill--line">已跳过</span>
      <span v-else-if="repeat" class="pill pill--line">{{ repeat }}</span>
      <span v-else-if="pill" class="pill" :class="`pill--${pill.tone}`">{{ pill.text }}</span>
    </div>
  </article>
</template>
