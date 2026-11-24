import { drawShapes, type Box, type Shape } from "./annotate";

export function shotSaveUsesBytes() {
	// 构建时设 VITE_SHOT_SAVE_BASE64=1，或在 localStorage 写入 daodian.shotSaveBase64=1，回到 base64 另存。
	if (import.meta.env.VITE_SHOT_SAVE_BASE64 === "1") return false;
	try {
		return localStorage.getItem("daodian.shotSaveBase64") !== "1";
	} catch {
		return true;
	}
}

export function blobToBase64(blob: Blob) {
	return new Promise<string>((resolve, reject) => {
		const reader = new FileReader();
		reader.onload = () => {
			const url = String(reader.result ?? "");
			const comma = url.indexOf(",");
			resolve(comma >= 0 ? url.slice(comma + 1) : url);
		};
		reader.onerror = () => reject(new Error("这块画面导不出来"));
		reader.readAsDataURL(blob);
	});
}

export interface ShotSurface {
	naturalWidth: number;
	naturalHeight: number;
	clientWidth: number;
	clientHeight: number;
	getBoundingClientRect(): DOMRect;
}

export async function renderShotBlob(input: {
	image: ShotSurface;
	stage: HTMLElement | null;
	box: Box | null;
	shapes: Shape[];
	frame: ImageBitmap;
}) {
	const { image, stage: stageEl, box, shapes } = input;
	if (!image.naturalWidth || !image.clientWidth) throw new Error("画面还没准备好");
	const frame = input.frame;
	const target =
		box && box.w >= 3 && box.h >= 3
			? box
			: { x: 0, y: 0, w: image.clientWidth, h: image.clientHeight };
	const scaleX = image.naturalWidth / image.clientWidth;
	const scaleY = image.naturalHeight / image.clientHeight;
	const stage = stageEl?.getBoundingClientRect();
	const imageBox = image.getBoundingClientRect();
	const rawX = stage ? imageBox.left - stage.left : 0;
	const rawY = stage ? imageBox.top - stage.top : 0;
	const originX = Math.abs(rawX) < 1 ? 0 : rawX;
	const originY = Math.abs(rawY) < 1 ? 0 : rawY;
	const canvas = document.createElement("canvas");
	canvas.width = Math.max(1, Math.round(target.w * scaleX));
	canvas.height = Math.max(1, Math.round(target.h * scaleY));
	const ctx = canvas.getContext("2d");
	if (!ctx) throw new Error("画布不可用");
	ctx.drawImage(
		frame,
		(target.x - originX) * scaleX,
		(target.y - originY) * scaleY,
		target.w * scaleX,
		target.h * scaleY,
		0,
		0,
		canvas.width,
		canvas.height,
	);
	if (box && box.w >= 3 && box.h >= 3) {
		ctx.save();
		ctx.beginPath();
		ctx.rect(0, 0, canvas.width, canvas.height);
		ctx.clip();
		ctx.scale(scaleX, scaleY);
		ctx.translate(-target.x, -target.y);
		drawShapes(ctx, shapes);
		ctx.restore();
	}
	const blob = await new Promise<Blob | null>((resolve) =>
		canvas.toBlob(resolve, "image/png"),
	);
	if (!blob) throw new Error("这块画面导不出来");
	return blob;
}
