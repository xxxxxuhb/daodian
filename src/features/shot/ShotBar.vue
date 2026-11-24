<script setup lang="ts">
import { shotColors, shotTools, type ShotTool } from "./palette";

defineProps<{
	tool: ShotTool;
	color: string;
	canUndo: boolean;
	cropped: boolean;
	busy: boolean;
	error: string;
}>();

const emit = defineEmits<{
	"update:tool": [tool: ShotTool];
	"update:color": [color: string];
	undo: [];
	reselect: [];
	longshot: [];
	gif: [];
	ocr: [];
	copy: [];
	pin: [];
	save: [];
	dismiss: [];
}>();
</script>

<template>
	<div class="shot-bar" @pointerdown.stop>
		<button
			v-for="item in shotTools"
			:key="item.id"
			type="button"
			class="shot-icon"
			:class="{ 'is-on': tool === item.id }"
			:aria-label="`${item.label} ${item.key}`"
			:title="`${item.label} ${item.key}`"
			@click="emit('update:tool', item.id)"
		>
			<svg viewBox="0 0 16 16" aria-hidden="true">
				<path
					v-if="item.id === 'move'"
					d="M8 1.5v13M1.5 8h13M8 1.5 6 3.5M8 1.5 10 3.5M8 14.5 6 12.5M8 14.5 10 12.5M1.5 8 3.5 6M1.5 8 3.5 10M14.5 8 12.5 6M14.5 8 12.5 10"
				/>
				<rect
					v-else-if="item.id === 'rect'"
					x="2.5"
					y="3.5"
					width="11"
					height="9"
					rx="1"
				/>
				<path v-else-if="item.id === 'arrow'" d="M3 13 13 3M13 3H8M13 3v5" />
				<path
					v-else-if="item.id === 'pen'"
					d="M3 13 11.5 4.5 13 3l-1.5 1.5L3 13zm0 0 2.2.4L3.4 11z"
				/>
				<path v-else d="M3 3.5h10M8 3.5V13" />
			</svg>
		</button>
		<span class="shot-gap" />
		<span class="shot-colors">
			<button
				v-for="item in shotColors"
				:key="item"
				type="button"
				class="shot-swatch"
				:class="{ 'is-on': color === item }"
				:style="{ background: item }"
				:aria-label="item"
				:title="item"
				@click="emit('update:color', item)"
			/>
		</span>
		<span class="shot-gap" />
		<button
			type="button"
			class="shot-icon"
			:disabled="!canUndo"
			aria-label="撤销 Ctrl+Z"
			title="撤销 Ctrl+Z"
			@click="emit('undo')"
		>
			<svg viewBox="0 0 16 16" aria-hidden="true">
				<path d="M4 7h7a3 3 0 0 1 0 6H8M4 7 6.5 4.5M4 7l2.5 2.5" />
			</svg>
		</button>
		<button
			v-if="cropped"
			type="button"
			class="shot-textbtn"
			:disabled="busy"
			@pointerdown.stop.prevent="emit('reselect')"
		>
			重新选
		</button>
		<button
			type="button"
			class="shot-textbtn"
			:disabled="busy"
			aria-label="长截图"
			title="长截图"
			@pointerdown.stop.prevent="emit('longshot')"
		>
			长截图
		</button>
		<button
			type="button"
			class="shot-textbtn"
			:disabled="busy"
			aria-label="GIF"
			title="GIF"
			@pointerdown.stop.prevent="emit('gif')"
		>
			GIF
		</button>
		<button
			type="button"
			class="shot-textbtn"
			:disabled="busy"
			aria-label="取字 Ctrl+Shift+O"
			title="取字 Ctrl+Shift+O"
			@pointerdown.stop.prevent="emit('ocr')"
		>
			取字
		</button>
		<button
			type="button"
			class="is-primary"
			:disabled="busy"
			@pointerdown.stop.prevent="emit('copy')"
		>
			复制
		</button>
		<button
			type="button"
			class="shot-icon"
			:disabled="busy"
			aria-label="贴图 Ctrl+T"
			title="贴图 Ctrl+T"
			@pointerdown.stop.prevent="emit('pin')"
		>
			<svg viewBox="0 0 16 16" aria-hidden="true">
				<path d="M8 2.5v7M6 4.2 8 2.5l2 1.7M5 8.5h6v5H5z" />
			</svg>
		</button>
		<button
			type="button"
			class="shot-icon"
			:disabled="busy"
			aria-label="另存 Ctrl+S"
			title="另存 Ctrl+S"
			@pointerdown.stop.prevent="emit('save')"
		>
			<svg viewBox="0 0 16 16" aria-hidden="true">
				<path d="M3 2.5h8l2 2V13.5H3zM5.5 2.5V6h5M5.5 13.5v-4h5v4" />
			</svg>
		</button>
		<button
			type="button"
			class="shot-icon"
			aria-label="退出"
			title="退出"
			@pointerdown.stop.prevent="emit('dismiss')"
		>
			<svg viewBox="0 0 16 16" aria-hidden="true">
				<path d="M4 4l8 8M12 4 4 12" />
			</svg>
		</button>
		<p v-if="error" class="shot-bar__error">{{ error }}</p>
	</div>
</template>

<style scoped>
.shot-bar {
	position: absolute;
	z-index: 5;
	display: flex;
	flex-wrap: wrap;
	align-items: center;
	gap: 4px;
	max-width: min(640px, calc(100vw - 16px));
	min-height: 44px;
	padding: 7px;
	border-radius: 12px;
	background: rgba(12, 31, 26, 0.94);
	box-shadow: 0 12px 40px rgba(0, 0, 0, 0.38);
}

.shot-bar button {
	border: 0;
	color: #e9f1ec;
	background: transparent;
	cursor: pointer;
}

.shot-bar button:focus-visible {
	outline: 2px solid #8fd0ea;
	outline-offset: 1px;
}

.shot-textbtn {
	height: 30px;
	padding: 0 10px;
	border: 0;
	border-radius: 7px;
	background: transparent;
	color: #e9f1ec;
	font:
		600 12px/30px "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
	cursor: pointer;
}

.shot-icon,
.shot-bar .is-primary {
	display: grid;
	width: 30px;
	height: 30px;
	place-items: center;
	padding: 0;
	border-radius: 7px;
}

.shot-icon svg {
	width: 16px;
	height: 16px;
	fill: none;
	stroke: currentColor;
	stroke-width: 1.7;
	stroke-linecap: round;
	stroke-linejoin: round;
}

.shot-icon.is-on {
	background: rgba(121, 183, 204, 0.18);
	color: #79b7cc;
}

.shot-bar .is-primary {
	width: 60px;
	background: #e9f1ec;
	color: #0c1f1a;
	font:
		600 12px/1 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
}

.shot-bar button:disabled {
	opacity: 0.4;
}

.shot-gap {
	width: 1px;
	height: 20px;
	margin: 0 2px;
	background: rgba(233, 241, 236, 0.14);
}

.shot-colors {
	display: flex;
	gap: 4px;
}

.shot-swatch {
	width: 18px;
	height: 18px;
	padding: 0;
	border-radius: 50%;
	box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.35);
}

.shot-swatch.is-on {
	outline: 2px solid #e9f1ec;
	outline-offset: 1px;
}

.shot-bar__error {
	flex-basis: 100%;
	margin: 0;
	color: #e9646b;
	font:
		12px/1.4 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
}
</style>
