<script setup lang="ts">
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { readFile } from "@tauri-apps/plugin-fs";
import {
	computed,
	nextTick,
	onBeforeUnmount,
	onMounted,
	shallowRef,
	useTemplateRef,
	watch,
} from "vue";
import { errorText } from "../../domain/feedback";
import {
	boxLocked,
	commitTextShape,
	drawShapes,
	escapeAction,
	handlePoint,
	handles,
	insideBox,
	moveBox,
	resizeBox,
	selectionDragEnd,
	selectionDragShouldClear,
	shouldConsumeTextPointer,
	shouldIgnoreCompositionEnter,
	snapBox,
	textBlurAction,
	textKeyAction,
	translateShapes,
	type Box,
	type Handle,
	type Point,
	type Shape,
} from "./annotate";
import { blobToBase64, renderShotBlob, shotSaveUsesBytes } from "./exportShot";
import ShotBar from "./ShotBar.vue";
import { shotColors, shotTools, type ShotDrag, type ShotTool } from "./palette";

const colors = shotColors;
const tools = shotTools;
type Tool = ShotTool;
type Drag = ShotDrag;

const shotQuery = new URLSearchParams(location.search);
const screen = Number(shotQuery.get("screen") ?? "0");
const openedLive = shotQuery.get("live") === "1";
const openedBackdrop = shotQuery.get("backdrop") === "1";
const stageRef = useTemplateRef("stage");
const imageRef = useTemplateRef("image");
const canvasRef = useTemplateRef("canvas");
const textRef = useTemplateRef("text");

const src = shallowRef("");
const ready = shallowRef(openedLive);
const live = shallowRef(openedLive);
const backdrop = shallowRef(openedBackdrop);
const cropped = shallowRef(false);
const backdropOn = computed(
	() => backdrop.value && !live.value && !cropped.value,
);
const keysLive = shallowRef(false);
const error = shallowRef("");
const busy = shallowRef(false);
const notice = shallowRef("");
const savePath = shallowRef("");
const phase = shallowRef<"select" | "edit">("select");
const tool = shallowRef<Tool>("move");
const color = shallowRef(colors[0]);
const selection = shallowRef<Box | null>(null);
const shapes = shallowRef<Shape[]>([]);
const drag = shallowRef<Drag | null>(null);
const textAt = shallowRef<Point | null>(null);
const textValue = shallowRef("");
const composing = shallowRef(false);
let pendingBlur = false;
let compositionEndedAt = 0;
let lastCommitAt = 0;
let textSession = 0;
let suppressBlurCommit = false;
let textFocusTimer = 0;
const spaceDown = shallowRef(false);
const freezing = computed(() => !ready.value && !error.value);
const shadeStyle = computed(() => {
	const box = selection.value;
	if (!ready.value || !box || box.w < 1 || box.h < 1) return null;
	return {
		left: `${box.x}px`,
		top: `${box.y}px`,
		width: `${box.w}px`,
		height: `${box.h}px`,
	};
});

const showHint = computed(
	() => ready.value && !error.value && !selection.value && !drag.value,
);
const imageStyle = computed(() => {
	if (!cropped.value || !selection.value) return undefined;
	const box = selection.value;
	return {
		left: `${box.x}px`,
		top: `${box.y}px`,
		width: `${box.w}px`,
		height: `${box.h}px`,
	};
});
const showBar = computed(
	() =>
		phase.value === "edit" && !!selection.value && !drag.value && !textAt.value,
);
const visibleHandles = computed(() => {
	const box = selection.value;
	if (!showBar.value || !box) return [];
	const names =
		box.w < 24 || box.h < 24 ? (["nw", "ne", "se", "sw"] as const) : handles;
	return names.map((id) => ({ id, ...handlePoint(box, id) }));
});
const sizeLabel = computed(() => physicalSize(liveBox()));
const sizeStyle = computed(() => {
	const box = liveBox();
	if (!box) return {};
	const top = box.y > 28 ? box.y - 26 : box.y + 8;
	return { left: `${box.x}px`, top: `${top}px` };
});
const barStyle = computed(() => {
	const box = selection.value;
	const stage = stageRef.value;
	if (!box || !stage) return {};
	const barHeight = 52;
	const barWidth = Math.min(480, Math.max(220, stage.clientWidth - 16));
	const below = box.y + box.h + 10;
	let top = below;
	if (below + barHeight > stage.clientHeight - 8) {
		top =
			box.y >= barHeight + 18 ? box.y - 10 - barHeight : Math.max(8, box.y + 8);
	}
	top = Math.min(
		Math.max(8, top),
		Math.max(8, stage.clientHeight - barHeight - 8),
	);
	const left = Math.min(
		Math.max(8, box.x),
		Math.max(8, stage.clientWidth - barWidth - 8),
	);
	return { left: `${left}px`, top: `${top}px` };
});
const noticeStyle = computed(() => {
	const bar = barStyle.value;
	const top = Number.parseFloat(String(bar.top ?? "8"));
	return { left: bar.left ?? "8px", top: `${Math.max(8, top - 40)}px` };
});

watch([selection, shapes, drag, phase, ready], schedulePaint);
watch(
	backdropOn,
	(on) => {
		document.documentElement.classList.toggle("is-backdrop", on);
	},
	{ immediate: true },
);

let stopRefresh: UnlistenFn | undefined;
let stopEscape: UnlistenFn | undefined;
let stopActive: UnlistenFn | undefined;
let stopArm: UnlistenFn | undefined;
let stopLive: UnlistenFn | undefined;
let noticeTimer = 0;

onMounted(() => {
	rememberStage();
	window.addEventListener("keydown", onKey);
	window.addEventListener("keyup", onKeyUp);
	window.addEventListener("resize", rememberStage);
	window.addEventListener("contextmenu", blockMenu);
	void listen<{
		index: number;
		path?: string;
		raw?: boolean;
		active?: boolean;
		crop?: boolean;
		backdrop?: boolean;
	}>("shot-refresh", (event) => {
		if (Number(event.payload?.index) !== screen) return;
		if (event.payload?.crop) {
			backdrop.value = false;
			live.value = false;
			cropped.value = true;
			ready.value = false;
			document.documentElement.classList.remove("is-live");
			error.value = "";
			phase.value = "edit";
			markShot("shot-visible");
			void showFrame({ raw: true });
			return;
		}
		resetCapture();
		backdrop.value = !!event.payload?.backdrop;
		if (event.payload?.active) keysLive.value = true;
		const path = event.payload?.path;
		const raw = !!event.payload?.raw;
		if (!raw && !path) {
			void load();
			return;
		}
		error.value = "";
		markShot("shot-visible");
		void showFrame({ path, raw });
	}).then((stop) => {
		stopRefresh = stop;
	});
	void listen("shot-arm", () => {
		armCapture();
	}).then((stop) => {
		stopArm = stop;
	});
	void listen("shot-live", () => {
		enterLive();
	}).then((stop) => {
		stopLive = stop;
	});
	void listen<number>("shot-active", (event) => {
		keysLive.value = Number(event.payload) === screen;
	}).then((stop) => {
		stopActive = stop;
	});
	void listen<number>("shot-escape", (event) => {
		void invoke("shot_escape_ack", { tick: Number(event.payload) });
		applyEscape();
	}).then((stop) => {
		stopEscape = stop;
	});
	void load();
});

onBeforeUnmount(() => {
	stopRefresh?.();
	stopEscape?.();
	stopActive?.();
	stopArm?.();
	stopLive?.();
	if (paintFrame) cancelAnimationFrame(paintFrame);
	releaseExportFrame();
	releaseFrameUrl();
	window.clearTimeout(noticeTimer);
	window.clearTimeout(textFocusTimer);
	window.removeEventListener("keydown", onKey);
	window.removeEventListener("keyup", onKeyUp);
	window.removeEventListener("resize", rememberStage);
	window.removeEventListener("contextmenu", blockMenu);
});

function armCapture() {
	resetCapture();
	releaseFrameUrl();
	src.value = "";
}

function resetCapture() {
	phase.value = "select";
	selection.value = null;
	shapes.value = [];
	drag.value = null;
	textAt.value = null;
	textValue.value = "";
	composing.value = false;
	pendingBlur = false;
	compositionEndedAt = 0;
	lastCommitAt = 0;
	textSession += 1;
	suppressBlurCommit = false;
	window.clearTimeout(textFocusTimer);
	ready.value = false;
	live.value = false;
	cropped.value = false;
	document.documentElement.classList.remove("is-live");
	error.value = "";
	notice.value = "";
	savePath.value = "";
	window.clearTimeout(noticeTimer);
	releaseExportFrame();
}

function enterLive() {
	if (live.value) return;
	backdrop.value = false;
	void invoke("shot_backdrop_hide").catch(() => {});
	resetCapture();
	releaseFrameUrl();
	src.value = "";
	live.value = true;
	ready.value = true;
	keysLive.value = true;
	document.documentElement.classList.add("is-live");
}

function reselect() {
	backdrop.value = false;
	void invoke("shot_backdrop_hide").catch(() => {});
	releaseFrameUrl();
	src.value = "";
	shapes.value = [];
	selection.value = null;
	drag.value = null;
	textAt.value = null;
	phase.value = "select";
	cropped.value = false;
	ready.value = true;
	error.value = "";
	notice.value = "";
	live.value = true;
	document.documentElement.classList.add("is-live");
}

async function grabLive() {
	const box = selection.value;
	if (!box || box.w < 3 || box.h < 3) return;
	if (!stageRect.width) rememberStage();
	try {
		await invoke("shot_grab", {
			grab: {
				index: screen,
				x: box.x,
				y: box.y,
				width: box.w,
				height: box.h,
				stageWidth: stageRect.width,
				stageHeight: stageRect.height,
			},
		});
	} catch (cause) {
		error.value = errorText(cause);
		selection.value = null;
		phase.value = "select";
	}
}

function blockMenu(event: Event) {
	event.preventDefault();
}

async function load() {
	error.value = "";
	try {
		const info = await invoke<{
			path: string;
			raw?: boolean;
			active?: boolean;
			live?: boolean;
		}>("shot_screen", { index: screen });
		if (info.live) {
			enterLive();
			return;
		}
		if (info.active) keysLive.value = true;
		markShot("shot-visible");
		await showFrame(info);
	} catch (cause) {
		const message = errorText(cause);
		if (src.value || /关掉|找不到/.test(message)) return;
		releaseFrameUrl();
		src.value = "";
		ready.value = false;
		error.value = message;
	}
}

let frameUrl = "";

function releaseFrameUrl() {
	if (!frameUrl) return;
	URL.revokeObjectURL(frameUrl);
	frameUrl = "";
}

async function showFrame(info: { path?: string; raw?: boolean }) {
	try {
		if (info.raw) {
			const bytes = await invoke<ArrayBuffer>("shot_frame", { index: screen });
			swapFrameUrl(
				URL.createObjectURL(new Blob([bytes], { type: "image/bmp" })),
			);
			return;
		}
		const path = info.path ?? "";
		if (!path) throw new Error("找不到这一屏");
		const bytes = await readFile(path);
		const next = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
		swapFrameUrl(next);
	} catch (cause) {
		if (info.raw || !info.path) {
			if (!src.value) {
				ready.value = false;
				error.value = errorText(cause);
			}
			return;
		}
		src.value = convertFileSrc(info.path);
		if (!src.value) {
			ready.value = false;
			error.value = errorText(cause);
		}
	}
}

function swapFrameUrl(next: string) {
	const previous = frameUrl;
	frameUrl = next;
	src.value = next;
	if (previous) URL.revokeObjectURL(previous);
}

function markShot(name: string) {
	performance.mark(name);
}

function onImage() {
	markShot("shot-image-ready");
	ready.value = true;
	error.value = "";
	const image = imageRef.value;
	if (!image?.naturalWidth) return;
	void createImageBitmap(image)
		.then((bitmap) => {
			exportFrame?.close();
			exportFrame = bitmap;
		})
		.catch(() => {});
	void invoke("shot_ready", { index: screen });
}

function onImageError() {
	ready.value = false;
	error.value = "冻结画面没有显示出来";
}

let stageRect = { left: 0, top: 0, width: 0, height: 0 };
let paintFrame = 0;
let exportFrame: ImageBitmap | null = null;

function rememberStage() {
	const stage = stageRef.value;
	if (!stage) return;
	const rect = stage.getBoundingClientRect();
	stageRect = {
		left: rect.left,
		top: rect.top,
		width: rect.width,
		height: rect.height,
	};
}

function bounds(): Box {
	if (!stageRect.width || !stageRect.height) rememberStage();
	return { x: 0, y: 0, w: stageRect.width, h: stageRect.height };
}

function pixelScale() {
	const image = imageRef.value;
	if (!image?.naturalWidth || !image.clientWidth)
		return window.devicePixelRatio || 1;
	return image.naturalWidth / image.clientWidth;
}

function liveBox() {
	return selection.value;
}

function physicalSize(box: Box | null) {
	const image = imageRef.value;
	if (
		!box ||
		box.w < 2 ||
		box.h < 2 ||
		!image?.naturalWidth ||
		!image.clientWidth
	)
		return "";
	const scale = image.naturalWidth / image.clientWidth;
	return `${Math.round(box.w * scale)} × ${Math.round(box.h * scale)}`;
}

function pointOf(event: PointerEvent): Point {
	if (!stageRect.width) rememberStage();
	return {
		x: Math.min(Math.max(0, event.clientX - stageRect.left), stageRect.width),
		y: Math.min(Math.max(0, event.clientY - stageRect.top), stageRect.height),
	};
}

function focusScreen() {
	keysLive.value = true;
	void invoke("shot_focus", { index: screen });
}

function onDown(event: PointerEvent) {
	if (event.button === 2) {
		applyEscape();
		return;
	}
	if (event.button !== 0) return;
	if (consumeTextPointer(event)) {
		focusScreen();
		return;
	}
	const stage = stageRef.value;
	if (!stage) return;
	rememberStage();
	const point = pointOf(event);
	const current = selection.value;
	const inside = current ? insideBox(current, point.x, point.y) : false;
	if (
		phase.value === "edit" &&
		current &&
		inside &&
		tool.value === "text" &&
		!spaceDown.value
	) {
		event.preventDefault();
		placeText(point);
		return;
	}
	focusScreen();
	stage.setPointerCapture(event.pointerId);
	if (
		phase.value === "edit" &&
		current &&
		inside &&
		(tool.value === "move" || spaceDown.value)
	) {
		drag.value = {
			kind: "move",
			origin: point,
			point,
			box: current,
			shapes: shapes.value,
		};
		return;
	}
	if (
		phase.value === "edit" &&
		current &&
		inside &&
		tool.value !== "move" &&
		tool.value !== "text"
	) {
		drag.value = {
			kind: tool.value,
			origin: point,
			point,
			points: [point],
			square: event.shiftKey,
		};
		return;
	}
	drag.value = {
		kind: "select",
		origin: point,
		point,
		square: event.shiftKey,
		previousSelection: current,
		previousShapes: shapes.value,
		cleared: false,
	};
}

function consumeTextPointer(event: PointerEvent) {
	const sinceCommitMs =
		lastCommitAt > 0 ? performance.now() - lastCommitAt : null;
	if (
		!shouldConsumeTextPointer({ typing: textAt.value !== null, sinceCommitMs })
	)
		return false;
	event.preventDefault();
	if (textAt.value) commitText();
	return true;
}

function onHandleDown(event: PointerEvent, handle: Handle) {
	if (event.button !== 0 || !selection.value) return;
	focusScreen();
	rememberStage();
	const stage = stageRef.value;
	stage?.setPointerCapture(event.pointerId);
	drag.value = {
		kind: "resize",
		handle,
		point: pointOf(event),
		box: selection.value,
		square: event.shiftKey,
	};
}

function onMove(event: PointerEvent) {
	const current = drag.value;
	if (!current) return;
	const point = pointOf(event);
	if (current.kind === "select") {
		const square = event.shiftKey;
		const next = boxLocked(current.origin, point, square);
		let cleared = current.cleared;
		if (selectionDragShouldClear(next, cleared)) {
			shapes.value = [];
			cleared = true;
		}
		if (cleared) {
			selection.value = next;
			phase.value = "select";
		}
		drag.value = { ...current, point, square, cleared };
	} else if (current.kind === "move") {
		const next = moveBox(
			current.box,
			point.x - current.origin.x,
			point.y - current.origin.y,
			bounds(),
		);
		selection.value = next;
		shapes.value = translateShapes(
			current.shapes,
			next.x - current.box.x,
			next.y - current.box.y,
		);
		drag.value = { ...current, point };
	} else if (current.kind === "resize") {
		drag.value = { ...current, point, square: event.shiftKey };
		selection.value = resizeBox(
			current.box,
			current.handle,
			point,
			bounds(),
			event.shiftKey,
		);
	} else if (current.kind === "pen") {
		current.points.push(point);
		current.point = point;
		schedulePaint();
	} else {
		drag.value = { ...current, point, square: event.shiftKey };
	}
}

function onUp() {
	const current = drag.value;
	drag.value = null;
	if (!current) return;
	if (current.kind === "select") {
		finishSelect(current);
		return;
	}
	if (current.kind === "move" || current.kind === "resize") {
		const box = selection.value;
		if (!box) {
			selection.value = null;
			phase.value = "select";
			return;
		}
		const snapped = snapBox(box, pixelScale());
		if (current.kind !== "resize") {
			shapes.value = translateShapes(
				shapes.value,
				snapped.x - box.x,
				snapped.y - box.y,
			);
		}
		selection.value = snapped;
		phase.value = "edit";
		return;
	}
	const shape = shapeFromDrag(current);
	if (shape) shapes.value = [...shapes.value, shape];
}

function onDouble() {
	if (
		tool.value !== "move" ||
		phase.value !== "edit" ||
		!selection.value ||
		textAt.value
	)
		return;
	void copyImage();
}

function shapeFromDrag(
	current: Extract<Drag, { kind: "rect" | "arrow" | "pen" }>,
): Shape | null {
	if (current.kind === "pen") {
		return current.points.length > 1
			? { kind: "pen", points: current.points, color: color.value }
			: null;
	}
	if (current.kind === "rect") {
		const box = boxLocked(current.origin, current.point, current.square);
		if (box.w < 2 && box.h < 2) return null;
		return { kind: "rect", ...box, color: color.value };
	}
	if (
		Math.hypot(
			current.point.x - current.origin.x,
			current.point.y - current.origin.y,
		) < 2
	)
		return null;
	return {
		kind: "arrow",
		x1: current.origin.x,
		y1: current.origin.y,
		x2: current.point.x,
		y2: current.point.y,
		color: color.value,
	};
}

function previewShape(): Shape | null {
	const current = drag.value;
	if (
		!current ||
		current.kind === "select" ||
		current.kind === "move" ||
		current.kind === "resize"
	)
		return null;
	return shapeFromDrag(current);
}

function schedulePaint() {
	if (paintFrame) return;
	paintFrame = requestAnimationFrame(() => {
		paintFrame = 0;
		paint();
	});
}

function paint() {
	const canvas = canvasRef.value;
	const box = selection.value;
	if (!canvas || !box || box.w < 1 || box.h < 1) return;
	const preview = previewShape();
	const dpr = window.devicePixelRatio || 1;
	const pixelWidth = Math.max(1, Math.round(box.w * dpr));
	const pixelHeight = Math.max(1, Math.round(box.h * dpr));
	if (!shapes.value.length && !preview) {
		if (canvas.width !== 0) {
			canvas.width = 0;
			canvas.height = 0;
		}
		return;
	}
	if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
		canvas.width = pixelWidth;
		canvas.height = pixelHeight;
	}
	const ctx = canvas.getContext("2d");
	if (!ctx) return;
	ctx.setTransform(dpr, 0, 0, dpr, -box.x * dpr, -box.y * dpr);
	ctx.clearRect(box.x, box.y, box.w, box.h);
	drawShapes(ctx, shapes.value);
	if (preview) drawShapes(ctx, [preview]);
}

function finishSelect(current: Extract<Drag, { kind: "select" }>) {
	const box = boxLocked(current.origin, current.point, current.square);
	const ended = selectionDragEnd({
		box,
		previousSelection: current.previousSelection,
		previousShapes: current.previousShapes,
	});
	if (!ended.committed) {
		selection.value = ended.selection;
		shapes.value = ended.shapes;
		phase.value = ended.phase;
		return;
	}
	selection.value = snapBox(
		box,
		live.value ? window.devicePixelRatio || 1 : pixelScale(),
	);
	shapes.value = [];
	if (live.value) {
		void grabLive();
		return;
	}
	phase.value = "edit";
}

function placeText(point: Point) {
	const session = ++textSession;
	window.clearTimeout(textFocusTimer);
	suppressBlurCommit = true;
	keysLive.value = true;
	textAt.value = point;
	textValue.value = "";
	void focusPlacedText(session);
}

async function focusPlacedText(session: number) {
	try {
		await invoke("shot_focus", { index: screen });
	} catch {
		// 窗口没抢到焦点时，仍然把光标放进输入框。
	}
	if (session !== textSession || !textAt.value) return;
	await nextTick();
	if (session !== textSession || !textAt.value) return;
	focusTextField();
	textFocusTimer = window.setTimeout(() => {
		if (session !== textSession) return;
		suppressBlurCommit = false;
		if (textAt.value && document.activeElement !== textRef.value)
			focusTextField();
	}, 120);
}

function focusTextField() {
	const field = textRef.value;
	if (!field) return;
	try {
		field.focus({ preventScroll: true });
	} catch {
		field.focus();
	}
}

function commitText() {
	const point = textAt.value;
	if (!point) return;
	if (composing.value) {
		pendingBlur = true;
		textRef.value?.blur();
		return;
	}
	const raw = textRef.value?.value ?? textValue.value;
	const result = commitTextShape({
		shapes: shapes.value,
		point,
		text: raw,
		color: color.value,
	});
	debugText("commit", raw.trim().length);
	closeText();
	lastCommitAt = performance.now();
	if (!result.added) return;
	shapes.value = result.shapes;
	tool.value = "move";
	schedulePaint();
}

function cancelText() {
	if (!textAt.value) return;
	debugText("cancel", textValue.value.length);
	closeText();
}

function closeText() {
	textSession += 1;
	suppressBlurCommit = false;
	window.clearTimeout(textFocusTimer);
	textAt.value = null;
	textValue.value = "";
	composing.value = false;
	pendingBlur = false;
	compositionEndedAt = 0;
}

function debugText(action: "commit" | "cancel", len: number) {
	if (!import.meta.env.DEV) return;
	console.debug(
		`text ${action} len=${len} composing=${composing.value ? 1 : 0}`,
	);
}

function onCompositionStart() {
	composing.value = true;
	compositionEndedAt = 0;
}

function onCompositionEnd() {
	composing.value = false;
	compositionEndedAt = performance.now();
	if (!pendingBlur) return;
	pendingBlur = false;
	commitText();
}

function onTextBlur() {
	const action = textBlurAction({
		composing: composing.value,
		settling: suppressBlurCommit,
	});
	if (action === "defer") {
		pendingBlur = true;
		return;
	}
	if (action === "refocus") {
		void nextTick(() => {
			if (textAt.value) focusTextField();
		});
		return;
	}
	commitText();
}

function onTextKey(event: KeyboardEvent) {
	if (event.isComposing || composing.value || event.keyCode === 229) return;
	const sinceCompositionEnd =
		compositionEndedAt > 0 ? performance.now() - compositionEndedAt : null;
	if (
		event.key === "Enter" &&
		shouldIgnoreCompositionEnter(sinceCompositionEnd)
	)
		return;
	const action = textKeyAction({ composing: false, key: event.key });
	if (action === "ignore") return;
	event.preventDefault();
	event.stopPropagation();
	if (action === "commit") commitText();
	else cancelText();
}

function undo() {
	if (textAt.value) {
		cancelText();
		return;
	}
	shapes.value = shapes.value.slice(0, -1);
}

function selectAll() {
	rememberStage();
	const stage = bounds();
	if (!stage.w || !stage.h) return;
	selection.value = snapBox(
		{ x: 0, y: 0, w: stage.w, h: stage.h },
		pixelScale(),
	);
	phase.value = "edit";
}

function nudge(event: KeyboardEvent, dx: number, dy: number) {
	const box = selection.value;
	if (!box) return;
	const step = (event.shiftKey ? 10 : 1) / pixelScale();
	const next = snapBox(
		moveBox(box, dx * step, dy * step, bounds()),
		pixelScale(),
	);
	shapes.value = translateShapes(shapes.value, next.x - box.x, next.y - box.y);
	selection.value = next;
	phase.value = "edit";
}

function applyEscape() {
	if (composing.value) return;
	const action = escapeAction({
		typing: textAt.value !== null,
		dragging: drag.value !== null,
		hasSelection:
			!!selection.value && selection.value.w >= 3 && selection.value.h >= 3,
	});
	if (action === "cancel-text") {
		cancelText();
		return;
	}
	if (action === "cancel-drag") {
		const current = drag.value;
		drag.value = null;
		if (current?.kind === "select") {
			const ended = selectionDragEnd({
				box: { x: 0, y: 0, w: 0, h: 0 },
				previousSelection: current.previousSelection,
				previousShapes: current.previousShapes,
			});
			selection.value = ended.selection;
			shapes.value = ended.shapes;
			phase.value = ended.phase;
		} else if (current?.kind === "move") {
			selection.value = current.box;
			shapes.value = current.shapes;
		} else if (current?.kind === "resize") {
			selection.value = current.box;
		}
		return;
	}
	if (action === "clear-selection") {
		selection.value = null;
		shapes.value = [];
		textAt.value = null;
		phase.value = "select";
		return;
	}
	void dismiss();
}

function onKey(event: KeyboardEvent) {
	if (!keysLive.value) return;
	if (savePath.value && event.key === "Escape") {
		event.preventDefault();
		finishSaved();
		return;
	}
	if (busy.value) {
		if (
			event.key === "Escape" ||
			(event.ctrlKey && event.key.toLowerCase() === "s")
		) {
			event.preventDefault();
		}
		return;
	}
	const typing =
		event.target instanceof HTMLInputElement ||
		event.target instanceof HTMLTextAreaElement;
	if (event.key === "Escape") {
		if (typing) return;
		event.preventDefault();
		applyEscape();
		return;
	}
	if (event.code === "Space" && !typing) {
		spaceDown.value = true;
		event.preventDefault();
		return;
	}
	if (typing) return;
	const key = event.key.toLowerCase();
	if (event.ctrlKey && event.shiftKey && key === "z") return;
	if (event.ctrlKey && key === "z") {
		event.preventDefault();
		undo();
		return;
	}
	if (event.ctrlKey && key === "a") {
		event.preventDefault();
		selectAll();
		return;
	}
	if (event.key === "Enter" || (event.ctrlKey && key === "c")) {
		event.preventDefault();
		void copyImage();
		return;
	}
	if (event.ctrlKey && key === "s") {
		event.preventDefault();
		void saveImage();
		return;
	}
	if ((event.ctrlKey && key === "t") || event.key === "F3") {
		event.preventDefault();
		void pinImage();
		return;
	}
	if (!event.ctrlKey && !event.altKey && !event.metaKey) {
		const next = tools.find((item) => item.key.toLowerCase() === key);
		if (next) {
			tool.value = next.id;
			return;
		}
		const arrow = {
			ArrowLeft: [-1, 0],
			ArrowRight: [1, 0],
			ArrowUp: [0, -1],
			ArrowDown: [0, 1],
		} as const;
		const step = arrow[event.key as keyof typeof arrow];
		if (step) {
			event.preventDefault();
			nudge(event, step[0], step[1]);
		}
	}
}

function onKeyUp(event: KeyboardEvent) {
	if (event.code === "Space") spaceDown.value = false;
}

function releaseExportFrame() {
	exportFrame?.close();
	exportFrame = null;
}

async function renderBlob(box: Box | null) {
	const image = imageRef.value;
	if (!image?.naturalWidth || !image.clientWidth)
		throw new Error("画面还没准备好");
	if (!exportFrame) exportFrame = await createImageBitmap(image);
	return renderShotBlob({
		image,
		stage: stageRef.value,
		box,
		shapes: shapes.value,
		frame: exportFrame,
	});
}

async function renderPng(box: Box | null) {
	return blobToBase64(await renderBlob(box));
}

function chosenBox() {
	const box = selection.value;
	if (!box || box.w < 3 || box.h < 3) throw new Error("先拖一块区域");
	return box;
}

async function copyImage() {
	if (busy.value) return;
	busy.value = true;
	error.value = "";
	try {
		commitText();
		const box =
			selection.value && selection.value.w >= 3 && selection.value.h >= 3
				? selection.value
				: null;
		const png = await renderPng(box);
		await invoke("shot_copy", { png });
		const image = imageRef.value;
		savePath.value = "";
		notice.value = `已复制 · ${
			box && image
				? physicalSize(box)
				: image
					? `${image.naturalWidth} × ${image.naturalHeight}`
					: "整屏"
		}`;
		window.clearTimeout(noticeTimer);
		noticeTimer = window.setTimeout(() => {
			notice.value = "";
			void dismiss();
		}, 700);
	} catch (cause) {
		error.value = errorText(cause);
	} finally {
		busy.value = false;
	}
}

async function pinImage() {
	if (busy.value) return;
	busy.value = true;
	error.value = "";
	try {
		commitText();
		const box = chosenBox();
		const scale = pixelScale();
		const png = await renderPng(box);
		await invoke("shot_pin", {
			png,
			x: Math.round(box.x * scale),
			y: Math.round(box.y * scale),
			width: Math.max(1, Math.round(box.w * scale)),
			height: Math.max(1, Math.round(box.h * scale)),
		});
	} catch (cause) {
		error.value = errorText(cause);
	} finally {
		busy.value = false;
	}
}

interface ShotSaveResult {
	saved: boolean;
	path: string;
	warning: string;
}

function armSaveClose() {
	if (!savePath.value) return;
	window.clearTimeout(noticeTimer);
	noticeTimer = window.setTimeout(finishSaved, 700);
}

function holdSaveNotice() {
	window.clearTimeout(noticeTimer);
}

function finishSaved() {
	window.clearTimeout(noticeTimer);
	notice.value = "";
	savePath.value = "";
	busy.value = false;
	void dismiss();
}

async function openSavedFolder() {
	const path = savePath.value;
	if (!path) return;
	holdSaveNotice();
	try {
		await invoke("shot_show_saved", { path });
	} catch (cause) {
		error.value = errorText(cause);
		armSaveClose();
		return;
	}
	finishSaved();
}

async function saveImage() {
	if (busy.value) return;
	busy.value = true;
	error.value = "";
	savePath.value = "";
	window.clearTimeout(noticeTimer);
	let holding = false;
	try {
		commitText();
		const blob = await renderBlob(chosenBox());
		const result = shotSaveUsesBytes()
			? await invoke<ShotSaveResult>(
					"shot_save_bytes",
					new Uint8Array(await blob.arrayBuffer()),
				)
			: await invoke<ShotSaveResult>("shot_save", {
					png: await blobToBase64(blob),
				});
		if (!result.saved) return;
		releaseExportFrame();
		savePath.value = result.path;
		notice.value = result.warning
			? `已保存到 ${result.path}（${result.warning}）`
			: `已保存到 ${result.path}`;
		holding = true;
		armSaveClose();
	} catch (cause) {
		error.value = errorText(cause);
	} finally {
		if (!holding) busy.value = false;
	}
}

async function dismiss() {
	try {
		await invoke("shot_dismiss");
	} catch (cause) {
		error.value = errorText(cause);
	}
}
</script>

<template>
	<div
		ref="stage"
		class="shot-stage"
		:class="{
			'is-failed': !src && !!error,
			'is-live': live,
			'is-backdrop': backdropOn,
		}"
		@pointerdown="onDown"
		@pointermove="onMove"
		@pointerup="onUp"
		@pointercancel="onUp"
		@dblclick="onDouble"
	>
		<button type="button" class="shot-exit" @pointerdown.stop.prevent="dismiss">
			退出 Esc
		</button>
		<img
			v-if="src"
			ref="image"
			class="shot-image"
			:class="{ 'is-crop': cropped, 'is-held': backdropOn }"
			:style="imageStyle"
			:src="src"
			alt=""
			decoding="async"
			draggable="false"
			@load="onImage"
			@error="onImageError"
		/>
		<div v-if="ready && !shadeStyle" class="shot-veil" />
		<div v-else-if="shadeStyle" class="shot-shade" :style="shadeStyle" />
		<canvas
			v-show="!!shadeStyle"
			ref="canvas"
			class="shot-canvas"
			:style="shadeStyle"
		/>
		<p v-if="freezing" class="shot-hint">正在冻结…</p>
		<p v-else-if="live && showHint" class="shot-hint">
			拖一块区域，松开后截取 · 右键退出
		</p>
		<p v-else-if="showHint" class="shot-hint">拖一块区域 · 右键退出</p>
		<div v-if="error && !ready" class="shot-fail" @pointerdown.stop>
			<p>{{ error }}</p>
			<button type="button" @click="load">重试</button>
			<button type="button" @click="dismiss">退出</button>
		</div>
		<span v-if="sizeLabel" class="shot-size" :style="sizeStyle">{{
			sizeLabel
		}}</span>
		<button
			v-for="handle in visibleHandles"
			:key="handle.id"
			type="button"
			class="shot-handle"
			:class="`is-${handle.id}`"
			:style="{ left: `${handle.x}px`, top: `${handle.y}px` }"
			:aria-label="`调整选区 ${handle.id}`"
			@pointerdown.stop="onHandleDown($event, handle.id)"
		/>
		<input
			v-if="textAt"
			ref="text"
			v-model="textValue"
			class="shot-text"
			type="text"
			autocomplete="off"
			spellcheck="false"
			:style="{ left: `${textAt.x}px`, top: `${textAt.y}px`, color }"
			aria-label="标注文字"
			@pointerdown.stop
			@compositionstart="onCompositionStart"
			@compositionend="onCompositionEnd"
			@keydown="onTextKey"
			@blur="onTextBlur"
		/>
		<div
			v-if="savePath && notice"
			class="shot-notice shot-notice--save"
			:style="noticeStyle"
			@pointerdown.stop
			@pointerenter="holdSaveNotice"
			@pointerleave="armSaveClose"
			@click="finishSaved"
		>
			<span>{{ notice }}</span>
			<button type="button" @click.stop="openSavedFolder">打开文件夹</button>
		</div>
		<p v-else-if="notice" class="shot-notice" :style="noticeStyle">
			{{ notice }}
		</p>
		<ShotBar
			v-if="showBar"
			v-model:tool="tool"
			v-model:color="color"
			:style="barStyle"
			:can-undo="shapes.length > 0"
			:cropped="cropped"
			:busy="busy"
			:error="src && error ? error : ''"
			@undo="undo"
			@reselect="reselect"
			@copy="copyImage"
			@pin="pinImage"
			@save="saveImage"
			@dismiss="dismiss"
		/>
	</div>
</template>

<style scoped>
.shot-stage {
	position: relative;
	width: 100vw;
	height: 100vh;
	overflow: hidden;
	background: #0c1f1a;
	cursor: crosshair;
	user-select: none;
}

.shot-stage.is-live,
.shot-stage.is-backdrop {
	background: transparent;
}

.shot-image.is-held {
	opacity: 0;
}

.shot-stage.is-live .shot-veil {
	background: rgba(4, 7, 11, 0.12);
}

.shot-veil {
	position: absolute;
	inset: 0;
	background: rgba(4, 7, 11, 0.5);
	pointer-events: none;
}

.shot-shade {
	position: absolute;
	pointer-events: none;
	box-shadow:
		0 0 0 1.5px #8fd0ea,
		0 0 0 3.5px rgba(255, 255, 255, 0.55),
		0 0 0 99999px rgba(4, 7, 11, 0.5);
}

.shot-exit {
	position: absolute;
	top: 16px;
	right: 16px;
	z-index: 6;
	height: 32px;
	padding: 0 12px;
	border: 1px solid rgba(233, 241, 236, 0.28);
	border-radius: 999px;
	background: rgba(12, 31, 26, 0.94);
	color: #e9f1ec;
	font:
		600 12px/1 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
	cursor: pointer;
}

.shot-image {
	position: absolute;
	inset: 0;
	width: 100%;
	height: 100%;
	object-fit: fill;
	object-position: left top;
	pointer-events: none;
}

.shot-image.is-crop {
	inset: auto;
	width: auto;
	height: auto;
}

.shot-canvas {
	position: absolute;
	pointer-events: none;
}

.shot-hint,
.shot-notice {
	position: absolute;
	z-index: 3;
	margin: 0;
	height: 30px;
	padding: 0 12px;
	border: 1px solid rgba(233, 241, 236, 0.28);
	border-radius: 999px;
	background: rgba(12, 31, 26, 0.94);
	color: #e9f1ec;
	font:
		600 11.5px/30px "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
	pointer-events: none;
}

.shot-notice--save {
	z-index: 7;
	height: auto;
	max-width: min(480px, calc(100vw - 24px));
	padding: 8px 8px 8px 12px;
	line-height: 1.45;
	pointer-events: auto;
	display: flex;
	align-items: center;
	gap: 8px;
	white-space: normal;
	cursor: pointer;
}

.shot-notice--save span {
	min-width: 0;
	overflow-wrap: anywhere;
}

.shot-notice--save button {
	flex: none;
	height: 26px;
	padding: 0 10px;
	border: 0;
	border-radius: 999px;
	background: #2f9e6b;
	color: #fff;
	font:
		600 12px/26px "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
	cursor: pointer;
}

.shot-hint {
	top: 50%;
	left: 50%;
	transform: translate(-50%, -50%);
}

.shot-fail {
	position: absolute;
	top: 50%;
	left: 50%;
	z-index: 5;
	display: flex;
	flex-wrap: wrap;
	gap: 8px;
	width: min(320px, calc(100vw - 32px));
	padding: 16px;
	border-radius: 12px;
	background: rgba(12, 31, 26, 0.94);
	color: #e9f1ec;
	box-shadow: 0 12px 40px rgba(0, 0, 0, 0.38);
	transform: translate(-50%, -50%);
}

.shot-fail p {
	flex-basis: 100%;
	margin: 0 0 4px;
	color: #e9646b;
	font:
		600 13px/1.4 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
}

.shot-fail button {
	height: 30px;
	padding: 0 12px;
	border: 0;
	border-radius: 7px;
	background: rgba(233, 241, 236, 0.12);
	color: #e9f1ec;
	cursor: pointer;
	font:
		600 12px/1 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
}

.shot-size {
	position: absolute;
	z-index: 3;
	padding: 3px 6px;
	border-radius: 6px;
	background: rgba(12, 31, 26, 0.94);
	color: #e9f1ec;
	font:
		600 11px/1 "Segoe UI",
		sans-serif;
	font-variant-numeric: tabular-nums;
	pointer-events: none;
}

.shot-handle {
	position: absolute;
	z-index: 4;
	width: 8px;
	height: 8px;
	padding: 0;
	border: 1px solid #0c1f1a;
	border-radius: 1px;
	background: #fff;
	transform: translate(-50%, -50%);
}

.shot-handle.is-nw,
.shot-handle.is-se {
	cursor: nwse-resize;
}

.shot-handle.is-ne,
.shot-handle.is-sw {
	cursor: nesw-resize;
}

.shot-handle.is-n,
.shot-handle.is-s {
	cursor: ns-resize;
}

.shot-handle.is-e,
.shot-handle.is-w {
	cursor: ew-resize;
}

.shot-text {
	position: absolute;
	z-index: 6;
	min-width: 80px;
	padding: 2px 4px;
	border: 1px dashed rgba(233, 241, 236, 0.8);
	background: transparent;
	color: inherit;
	caret-color: currentColor;
	font:
		600 18px/1.2 "Segoe UI",
		"Microsoft YaHei UI",
		sans-serif;
	outline: none;
	pointer-events: auto;
	user-select: text;
}

.shot-fail button:focus-visible,
.shot-handle:focus-visible {
	outline: 2px solid #8fd0ea;
	outline-offset: 1px;
}
</style>
