<script setup lang="ts">
import { NodeViewWrapper, nodeViewProps } from "@tiptap/vue-3";
import { computed, inject, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { mediaDirKey } from "../open";
import { mediaUrl } from "../upload";

const props = defineProps(nodeViewProps);
const dir = inject(mediaDirKey, ref(""));
const open = ref(false);
const root = ref<HTMLElement | null>(null);
const bar = ref<{ top: number; left: number } | null>(null);
const stage = ref<"full" | "thumb" | "dead">("full");
const hash = computed(() => String(props.node.attrs.hash ?? ""));
const ext = computed(() => String(props.node.attrs.ext ?? "png"));
const alt = computed(() => String(props.node.attrs.alt ?? ""));
const src = computed(() => mediaUrl(dir.value, `${hash.value}.${ext.value}`));
const thumb = computed(() => mediaUrl(dir.value, `${hash.value}@480.webp`));
const shown = computed(() => {
  if (stage.value === "dead") return "";
  if (stage.value === "thumb") return thumb.value;
  return src.value || thumb.value;
});

function onImgError() {
  if (stage.value === "full" && thumb.value && thumb.value !== src.value) stage.value = "thumb";
  else stage.value = "dead";
}

function place() {
  const el = root.value;
  if (!props.selected || !el) {
    bar.value = null;
    return;
  }
  const rect = el.getBoundingClientRect();
  bar.value = { top: Math.max(8, rect.top - 40), left: Math.max(8, rect.left) };
}

watch(
  () => props.selected,
  async () => {
    await nextTick();
    place();
  },
);

onMounted(() => {
  window.addEventListener("scroll", place, true);
  window.addEventListener("resize", place);
});
onUnmounted(() => {
  window.removeEventListener("scroll", place, true);
  window.removeEventListener("resize", place);
});
</script>

<template>
  <node-view-wrapper class="note-image" :class="{ 'is-selected': selected }">
    <span ref="root">
      <img v-if="shown" :src="shown" :alt="alt" @error="onImgError" @dblclick="open = true" />
      <span v-else>图片缺失</span>
    </span>
    <div v-if="bar" class="bubble" :style="{ top: bar.top + 'px', left: bar.left + 'px' }" @mousedown.prevent>
      <button type="button" @mousedown.prevent="open = true">查看</button>
      <button type="button" @mousedown.prevent="deleteNode()">删除</button>
    </div>
    <div v-if="open" class="note-lightbox" @click="open = false">
      <img :src="src || thumb" :alt="alt" />
    </div>
  </node-view-wrapper>
</template>
