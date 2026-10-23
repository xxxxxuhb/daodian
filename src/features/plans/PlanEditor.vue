<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { PRIORITIES } from "../../domain/priority";
import Icon from "../../ui/Icon.vue";
import TimeField from "../../ui/TimeField.vue";
import { emptyStage, MAX_STAGES, stagePlaceholder, type PlanDraft } from "./logic";

const props = defineProps<{ draft: PlanDraft; now: number; saving: boolean; error: string }>();
const emit = defineEmits<{ close: []; save: []; remove: []; toggle: [] }>();
const dragFrom = ref(-1);

function addStage() {
  if (props.draft.stages.length >= MAX_STAGES) return;
  props.draft.stages.push(emptyStage());
}

function removeStage(index: number) {
  props.draft.stages.splice(index, 1);
}

function onDragStart(index: number, event: DragEvent) {
  dragFrom.value = index;
  event.dataTransfer?.setData("text/plain", String(index));
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}

function onDrop(index: number, event: DragEvent) {
  const from = Number(event.dataTransfer?.getData("text/plain") || dragFrom.value);
  dragFrom.value = -1;
  if (!Number.isInteger(from) || from < 0 || from === index) return;
  const next = props.draft.stages.slice();
  const [item] = next.splice(from, 1);
  if (!item) return;
  next.splice(index, 0, item);
  props.draft.stages = next;
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
    <div class="drawer" role="dialog" aria-modal="true">
      <div class="drawer__head">
        <div>
          <p class="eyebrow">计划 · {{ draft.id ? "编辑" : "新建" }}</p>
          <div class="drawer__title">{{ draft.title.trim() || "新的计划" }}</div>
        </div>
        <button class="icon-btn" type="button" aria-label="关闭" @click="emit('close')">
          <Icon name="x" small />
        </button>
      </div>
      <div class="drawer__body">
        <div class="field">
          <div class="field__label"><span>计划名称</span><span>Ctrl S 保存</span></div>
          <div class="input input--big">
            <input v-model="draft.title" maxlength="80" placeholder="例如 1.2 版本" />
          </div>
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
          <div class="field__label">
            <span>阶段</span>
            <span>{{ draft.stages.length }} 个 · 到点分别提醒</span>
          </div>
          <div class="field__hint" style="margin: -2px 0 10px">每个阶段一个时间点。发布往后挪，观察跟着挪。</div>
          <div
            v-for="(stage, index) in draft.stages"
            :key="stage.key"
            class="stage"
            @dragover.prevent
            @drop="onDrop(index, $event)"
          >
            <div class="stage__idx">{{ String(index + 1).padStart(2, "0") }}</div>
            <div>
              <div class="stage__name">
                <input v-model="stage.title" maxlength="40" :placeholder="stagePlaceholder(index)" />
                <span v-if="stage.done" class="pill pill--moss">已完成</span>
              </div>
              <TimeField :at="stage.at" :now="now" @update:at="stage.at = $event" />
            </div>
            <div class="stage__tools">
              <button class="icon-btn" type="button" aria-label="删除阶段" @click="removeStage(index)">
                <Icon name="trash" small />
              </button>
              <button class="stage__grip" type="button" draggable="true" aria-label="拖动排序" @dragstart="onDragStart(index, $event)">
                <Icon name="grip" small />
              </button>
            </div>
          </div>
          <button class="btn btn--dash" type="button" style="width: 100%; justify-content: center; margin-top: 6px" @click="addStage">
            <Icon name="plus" small />再加一个阶段
          </button>
        </div>
        <div class="field">
          <div class="field__label"><span>备注</span><span>{{ draft.note.length }} / 500</span></div>
          <div class="input textarea">
            <textarea v-model="draft.note" maxlength="500" placeholder="这段计划要盯住什么"></textarea>
          </div>
        </div>
        <p v-if="error" class="banner">{{ error }}</p>
      </div>
      <div class="drawer__foot">
        <button class="btn btn--primary" type="button" :disabled="saving" @click="emit('save')">保存</button>
        <button v-if="draft.id" class="btn" type="button" :disabled="saving" @click="emit('toggle')">
          {{ draft.done ? "标为未完成" : "整份标为完成" }}
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
