<script setup lang="ts">
import { NodeViewWrapper, nodeViewProps } from "@tiptap/vue-3";
import { computed, inject } from "vue";
import { openLinkedKey, type LinkedTarget } from "../open";

const props = defineProps(nodeViewProps);
const openLinked = inject(openLinkedKey, null);
const target = computed<LinkedTarget>(() => {
  const kind = props.node.attrs.kind === "plan" || props.node.attrs.kind === "stage" ? props.node.attrs.kind : "todo";
  return {
    kind,
    id: String(props.node.attrs.id ?? ""),
    planId: String(props.node.attrs.planId ?? "") || undefined,
    title: String(props.node.attrs.title ?? "未命名"),
  };
});
</script>

<template>
  <node-view-wrapper class="note-ref">
    <button type="button" @click="openLinked?.(target)">
      <small>{{ target.kind === "todo" ? "待办" : target.kind === "plan" ? "计划" : "阶段" }}</small>
      <b>{{ target.title }}</b>
    </button>
  </node-view-wrapper>
</template>
