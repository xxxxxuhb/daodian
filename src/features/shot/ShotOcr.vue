<script setup lang="ts">
import { onMounted, useTemplateRef } from "vue";

defineProps<{
	text: string;
	hint: string;
	notice: string;
	settings: boolean;
}>();

const emit = defineEmits<{
	"update:text": [value: string];
	copy: [];
	note: [];
	todo: [];
	settings: [];
	close: [];
}>();

const field = useTemplateRef<HTMLTextAreaElement>("field");

onMounted(() => {
	field.value?.focus();
});

function onInput(event: Event) {
	const target = event.target;
	if (target instanceof HTMLTextAreaElement) emit("update:text", target.value);
}
</script>

<template>
	<section class="ocr" @pointerdown.stop>
		<header class="ocr__head">
			<strong>取字</strong>
			<button type="button" aria-label="关闭取字" title="关闭" @click="emit('close')">
				关闭
			</button>
		</header>
		<textarea
			ref="field"
			class="ocr__text"
			:value="text"
			rows="6"
			spellcheck="false"
			aria-label="识别出的文字"
			@input="onInput"
		/>
		<p v-if="hint" class="ocr__hint">{{ hint }}</p>
		<p v-if="notice" class="ocr__notice">{{ notice }}</p>
		<div class="ocr__actions">
			<button type="button" class="is-primary" @click="emit('copy')">复制</button>
			<button type="button" @click="emit('note')">放入笔记</button>
			<button type="button" @click="emit('todo')">变成待办</button>
			<button v-if="settings" type="button" @click="emit('settings')">
				语言设置
			</button>
		</div>
	</section>
</template>

<style scoped>
.ocr {
	position: absolute;
	z-index: 6;
	display: flex;
	flex-direction: column;
	gap: 8px;
	width: 280px;
	padding: 10px;
	border-radius: 12px;
	background: rgba(12, 31, 26, 0.96);
	box-shadow: 0 12px 40px rgba(0, 0, 0, 0.38);
	color: #e9f1ec;
}

.ocr__head,
.ocr__actions {
	display: flex;
	align-items: center;
	gap: 6px;
}

.ocr__head {
	justify-content: space-between;
}

.ocr__head strong,
.ocr button {
	font:
		600 12px/1 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
}

.ocr button {
	height: 28px;
	padding: 0 8px;
	border: 0;
	border-radius: 7px;
	background: transparent;
	color: #e9f1ec;
	cursor: pointer;
}

.ocr button.is-primary {
	background: #e9f1ec;
	color: #0c1f1a;
}

.ocr button:focus-visible,
.ocr__text:focus-visible {
	outline: 2px solid #8fd0ea;
	outline-offset: 1px;
}

.ocr__text {
	width: 100%;
	min-height: 120px;
	resize: vertical;
	box-sizing: border-box;
	padding: 8px;
	border: 0;
	border-radius: 8px;
	background: rgba(233, 241, 236, 0.08);
	color: #e9f1ec;
	font:
		13px/1.5 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
}

.ocr__hint,
.ocr__notice {
	margin: 0;
	font:
		12px/1.4 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
}

.ocr__hint {
	color: #f0c36a;
}

.ocr__notice {
	color: #9dcec0;
}

.ocr__actions {
	flex-wrap: wrap;
}
</style>
