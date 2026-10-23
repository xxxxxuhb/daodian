<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import { PRIORITIES } from "../../domain/priority";
import { atClock } from "../../domain/present";
import { LEADS, nextLabel, repeatLabel, type RepeatRule } from "../../domain/repeat";
import Icon from "../../ui/Icon.vue";
import TimeField from "../../ui/TimeField.vue";
import type { TodoDraft } from "./logic";

const props = defineProps<{ draft: TodoDraft; now: number; saving: boolean; error: string }>();
const emit = defineEmits<{ close: []; save: []; remove: []; toggle: [] }>();

const none = computed(() => props.draft.dueAt == null);
const today15 = computed(() => props.draft.dueAt != null && Math.abs(props.draft.dueAt - atClock(props.now, 0, 15, 0)) < 60_000);
const tomorrow9 = computed(() => props.draft.dueAt != null && Math.abs(props.draft.dueAt - atClock(props.now, 1, 9, 0)) < 60_000);

const rules: { id: RepeatRule | "none"; label: string }[] = [
  { id: "none", label: "不重复" },
  { id: "weekly", label: "每周" },
  { id: "weekdays", label: "每个工作日" },
  { id: "monthEnd", label: "每月最后一天" },
];
const following = computed(() => nextLabel(props.draft.repeat, props.draft.dueAt));

function setRepeat(id: RepeatRule | "none") {
  if (id === "none" || props.draft.dueAt == null) {
    props.draft.repeat = null;
    return;
  }
  props.draft.repeat = {
    rule: id,
    weekday: id === "weekly" ? new Date(props.draft.dueAt).getDay() : null,
  };
}

function pick(kind: "none" | "today15" | "tomorrow9") {
  if (kind === "none") props.draft.dueAt = null;
  if (kind === "today15") props.draft.dueAt = atClock(props.now, 0, 15, 0);
  if (kind === "tomorrow9") props.draft.dueAt = atClock(props.now, 1, 9, 0);
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") emit("close");
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    emit("save");
  }
}

onMounted(() => window.addEventListener("keydown", onKey));
onUnmounted(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Teleport to="body">
    <div class="scrim" @click="emit('close')"></div>
    <div class="drawer drawer--sm" role="dialog" aria-modal="true">
      <div class="drawer__head">
        <div>
          <p class="eyebrow">待办 · {{ draft.id ? "编辑" : "新建" }}</p>
          <div class="drawer__title">{{ draft.title.trim() || "新的待办" }}</div>
        </div>
        <button class="icon-btn" type="button" aria-label="关闭" @click="emit('close')">
          <Icon name="x" small />
        </button>
      </div>
      <div class="drawer__body">
        <div class="field">
          <div class="field__label"><span>标题</span><span>Ctrl S 保存</span></div>
          <div class="input input--big">
            <input v-model="draft.title" maxlength="80" placeholder="先写个标题" />
          </div>
        </div>
        <div class="field">
          <div class="field__label"><span>时间点</span><span>到点提醒一次</span></div>
          <div class="composer__row" style="border: 0; margin: 0; padding: 0">
            <button type="button" class="chip" :class="{ on: none }" @click="pick('none')">不设时间</button>
            <button type="button" class="chip" :class="{ on: today15 }" @click="pick('today15')">今天 15:00</button>
            <button type="button" class="chip" :class="{ on: tomorrow9 }" @click="pick('tomorrow9')">明天 09:00</button>
          </div>
          <TimeField :at="draft.dueAt" :now="now" @update:at="draft.dueAt = $event" />
          <div class="composer__row" style="border: 0; margin: 8px 0 0; padding: 0">
            <button
              v-for="item in rules"
              :key="item.id"
              type="button"
              class="chip"
              :class="{ on: (draft.repeat?.rule ?? 'none') === item.id }"
              @click="setRepeat(item.id)"
            >
              {{ item.id === "weekly" && draft.repeat?.rule === "weekly" ? repeatLabel(draft.repeat, draft.dueAt) : item.label }}
            </button>
            <select class="lead" :value="draft.leadMs" @change="draft.leadMs = Number(($event.target as HTMLSelectElement).value)">
              <option v-for="item in LEADS" :key="item.ms" :value="item.ms">{{ item.label }}</option>
            </select>
          </div>
          <p v-if="following" class="field__hint">{{ following }}</p>
        </div>
        <div class="field">
          <div class="field__label"><span>重要程度</span></div>
          <div class="seg">
            <button
              v-for="item in PRIORITIES"
              :key="item.value"
              type="button"
              :class="{ on: draft.priority === item.value }"
              @click="draft.priority = item.value"
            >
              {{ item.label }}
            </button>
          </div>
        </div>
        <div class="field">
          <div class="field__label"><span>备注</span><span>{{ draft.note.length }} / 500</span></div>
          <div class="input textarea">
            <textarea v-model="draft.note" maxlength="500" placeholder="空下来再看的细节"></textarea>
          </div>
        </div>
        <p v-if="error" class="banner">{{ error }}</p>
      </div>
      <div class="drawer__foot">
        <button class="btn btn--primary" type="button" :disabled="saving" @click="emit('save')">保存</button>
        <button v-if="draft.id" class="btn" type="button" :disabled="saving" @click="emit('toggle')">
          {{ draft.done ? "标为未完成" : "标为完成" }}
        </button>
        <button
          v-if="draft.id"
          class="btn btn--ghost"
          type="button"
          style="margin-left: auto; color: var(--danger)"
          :disabled="saving"
          @click="emit('remove')"
        >
          删除
        </button>
      </div>
    </div>
  </Teleport>
</template>
