export interface Point {
  x: number;
  y: number;
}

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type Shape =
  | ({ kind: "rect"; color: string } & Box)
  | { kind: "arrow"; x1: number; y1: number; x2: number; y2: number; color: string }
  | { kind: "pen"; points: Point[]; color: string }
  | { kind: "text"; x: number; y: number; text: string; color: string };

export function boxFromPoints(x1: number, y1: number, x2: number, y2: number): Box {
  return {
    x: Math.min(x1, x2),
    y: Math.min(y1, y2),
    w: Math.abs(x2 - x1),
    h: Math.abs(y2 - y1),
  };
}

export function insideBox(box: Box, x: number, y: number) {
  return x >= box.x && y >= box.y && x <= box.x + box.w && y <= box.y + box.h;
}

export type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

export const handles: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

export type EscapeAction = "cancel-text" | "cancel-drag" | "clear-selection" | "exit";

export function escapeAction(input: { typing: boolean; dragging: boolean; hasSelection: boolean }): EscapeAction {
  if (input.typing) return "cancel-text";
  if (input.dragging) return "cancel-drag";
  if (input.hasSelection) return "clear-selection";
  return "exit";
}

export const TEXT_COMMIT_GUARD_MS = 220;

export type TextKeyAction = "ignore" | "commit" | "cancel";

export function textKeyAction(input: { composing: boolean; key: string }): TextKeyAction {
  if (input.composing) return "ignore";
  if (input.key === "Enter") return "commit";
  if (input.key === "Escape") return "cancel";
  return "ignore";
}

export type TextBlurAction = "defer" | "refocus" | "commit";

export function textBlurAction(input: { composing: boolean; settling: boolean }): TextBlurAction {
  if (input.composing) return "defer";
  if (input.settling) return "refocus";
  return "commit";
}

export function shouldConsumeTextPointer(input: { typing: boolean; sinceCommitMs: number | null }): boolean {
  if (input.typing) return true;
  return input.sinceCommitMs !== null && input.sinceCommitMs >= 0 && input.sinceCommitMs < TEXT_COMMIT_GUARD_MS;
}

export const COMPOSITION_ENTER_GUARD_MS = 50;

export function shouldIgnoreCompositionEnter(sinceCompositionEndMs: number | null): boolean {
  return (
    sinceCompositionEndMs !== null &&
    sinceCompositionEndMs >= 0 &&
    sinceCompositionEndMs < COMPOSITION_ENTER_GUARD_MS
  );
}

export function commitTextShape(input: {
  shapes: Shape[];
  point: Point | null;
  text: string;
  color: string;
}): { shapes: Shape[]; added: boolean } {
  if (!input.point) return { shapes: input.shapes, added: false };
  const text = input.text.trim();
  if (!text) return { shapes: input.shapes, added: false };
  return {
    shapes: [...input.shapes, { kind: "text", x: input.point.x, y: input.point.y, text, color: input.color }],
    added: true,
  };
}

export function selectionDragShouldClear(box: Box, cleared: boolean): boolean {
  return !cleared && box.w >= 3 && box.h >= 3;
}

export function selectionDragEnd(input: {
  box: Box;
  previousSelection: Box | null;
  previousShapes: Shape[];
}): { committed: boolean; selection: Box | null; shapes: Shape[]; phase: "select" | "edit" } {
  if (input.box.w < 3 || input.box.h < 3) {
    const selection = input.previousSelection;
    return {
      committed: false,
      selection,
      shapes: input.previousShapes,
      phase: selection && selection.w >= 3 && selection.h >= 3 ? "edit" : "select",
    };
  }
  return { committed: true, selection: input.box, shapes: [], phase: "edit" };
}

export function handlePoint(box: Box, handle: Handle): Point {
  const right = box.x + box.w;
  const bottom = box.y + box.h;
  const midX = box.x + box.w / 2;
  const midY = box.y + box.h / 2;
  switch (handle) {
    case "nw":
      return { x: box.x, y: box.y };
    case "n":
      return { x: midX, y: box.y };
    case "ne":
      return { x: right, y: box.y };
    case "e":
      return { x: right, y: midY };
    case "se":
      return { x: right, y: bottom };
    case "s":
      return { x: midX, y: bottom };
    case "sw":
      return { x: box.x, y: bottom };
    case "w":
      return { x: box.x, y: midY };
  }
}

export function hitHandle(box: Box, point: Point, radius = 8): Handle | null {
  const names = box.w < 24 || box.h < 24 ? (["nw", "ne", "se", "sw"] as const) : handles;
  let best: Handle | null = null;
  let bestDist = radius * radius;
  for (const handle of names) {
    const at = handlePoint(box, handle);
    const dx = point.x - at.x;
    const dy = point.y - at.y;
    const dist = dx * dx + dy * dy;
    if (dist <= bestDist) {
      best = handle;
      bestDist = dist;
    }
  }
  return best;
}

export function moveBox(box: Box, dx: number, dy: number, bounds: Box): Box {
  const maxX = bounds.x + Math.max(0, bounds.w - box.w);
  const maxY = bounds.y + Math.max(0, bounds.h - box.h);
  return {
    ...box,
    x: clamp(box.x + dx, bounds.x, maxX),
    y: clamp(box.y + dy, bounds.y, maxY),
  };
}

export function resizeBox(box: Box, handle: Handle, point: Point, bounds: Box, keepRatio: boolean): Box {
  let x1 = box.x;
  let y1 = box.y;
  let x2 = box.x + box.w;
  let y2 = box.y + box.h;
  if (handle.includes("w")) x1 = point.x;
  if (handle.includes("e")) x2 = point.x;
  if (handle.includes("n")) y1 = point.y;
  if (handle.includes("s")) y2 = point.y;
  if (keepRatio && box.w > 0 && box.h > 0) {
    const ratio = box.w / box.h;
    let width = Math.abs(x2 - x1);
    let height = Math.abs(y2 - y1);
    if (handle === "n" || handle === "s") width = height * ratio;
    else if (handle === "e" || handle === "w") height = width / ratio;
    else if (height === 0 || width / height > ratio) height = width / ratio;
    else width = height * ratio;
    const anchorX = handle.includes("w") ? box.x + box.w : box.x;
    const anchorY = handle.includes("n") ? box.y + box.h : box.y;
    if (handle === "n" || handle === "s") {
      x1 = anchorX - width / 2;
      x2 = anchorX + width / 2;
      y1 = handle === "n" ? anchorY - height : anchorY;
      y2 = handle === "n" ? anchorY : anchorY + height;
    } else if (handle === "e" || handle === "w") {
      y1 = anchorY - height / 2;
      y2 = anchorY + height / 2;
      x1 = handle === "w" ? anchorX - width : anchorX;
      x2 = handle === "w" ? anchorX : anchorX + width;
    } else {
      x1 = handle.includes("w") ? anchorX - width : anchorX;
      x2 = handle.includes("e") ? anchorX + width : anchorX;
      y1 = handle.includes("n") ? anchorY - height : anchorY;
      y2 = handle.includes("s") ? anchorY + height : anchorY;
    }
  }
  const next = clampBox(boxFromPoints(x1, y1, x2, y2), bounds);
  return next.w < 1 || next.h < 1 ? box : next;
}

export function snapBox(box: Box, scale: number): Box {
  if (!Number.isFinite(scale) || scale <= 0) return box;
  const x = Math.round(box.x * scale) / scale;
  const y = Math.round(box.y * scale) / scale;
  const right = Math.round((box.x + box.w) * scale) / scale;
  const bottom = Math.round((box.y + box.h) * scale) / scale;
  return { x, y, w: Math.max(0, right - x), h: Math.max(0, bottom - y) };
}

export function boxLocked(origin: Point, point: Point, square: boolean): Box {
  if (!square) return boxFromPoints(origin.x, origin.y, point.x, point.y);
  const dx = point.x - origin.x;
  const dy = point.y - origin.y;
  const side = Math.max(Math.abs(dx), Math.abs(dy));
  const sx = dx < 0 ? -1 : 1;
  const sy = dy < 0 ? -1 : 1;
  return boxFromPoints(origin.x, origin.y, origin.x + sx * side, origin.y + sy * side);
}

export function translateShapes(shapes: Shape[], dx: number, dy: number): Shape[] {
  if (dx === 0 && dy === 0) return shapes;
  return shapes.map((shape) => {
    if (shape.kind === "rect" || shape.kind === "text") return { ...shape, x: shape.x + dx, y: shape.y + dy };
    if (shape.kind === "arrow") {
      return { ...shape, x1: shape.x1 + dx, y1: shape.y1 + dy, x2: shape.x2 + dx, y2: shape.y2 + dy };
    }
    return { ...shape, points: shape.points.map((point) => ({ x: point.x + dx, y: point.y + dy })) };
  });
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function clampBox(box: Box, bounds: Box): Box {
  let { x, y, w, h } = box;
  if (x < bounds.x) {
    w -= bounds.x - x;
    x = bounds.x;
  }
  if (y < bounds.y) {
    h -= bounds.y - y;
    y = bounds.y;
  }
  if (x + w > bounds.x + bounds.w) w = bounds.x + bounds.w - x;
  if (y + h > bounds.y + bounds.h) h = bounds.y + bounds.h - y;
  return { x, y, w: Math.max(0, w), h: Math.max(0, h) };
}

export function drawShape(ctx: CanvasRenderingContext2D, shape: Shape) {
  ctx.save();
  ctx.lineWidth = 3;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.strokeStyle = shape.color;
  ctx.fillStyle = shape.color;
  if (shape.kind === "rect") {
    ctx.strokeRect(shape.x, shape.y, shape.w, shape.h);
  } else if (shape.kind === "arrow") {
    drawArrow(ctx, shape.x1, shape.y1, shape.x2, shape.y2);
  } else if (shape.kind === "pen" && shape.points.length > 1) {
    ctx.beginPath();
    ctx.moveTo(shape.points[0].x, shape.points[0].y);
    for (let index = 1; index < shape.points.length; index += 1) {
      const point = shape.points[index];
      ctx.lineTo(point.x, point.y);
    }
    ctx.stroke();
  } else if (shape.kind === "text") {
    ctx.font = '600 18px "Segoe UI", "Microsoft YaHei UI", sans-serif';
    ctx.textBaseline = "top";
    ctx.lineWidth = 4;
    ctx.strokeStyle = "rgba(255,255,255,0.92)";
    ctx.strokeText(shape.text, shape.x, shape.y);
    ctx.fillStyle = shape.color;
    ctx.fillText(shape.text, shape.x, shape.y);
  }
  ctx.restore();
}

export function drawShapes(ctx: CanvasRenderingContext2D, shapes: Shape[]) {
  for (const shape of shapes) drawShape(ctx, shape);
}

function drawArrow(ctx: CanvasRenderingContext2D, x1: number, y1: number, x2: number, y2: number) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const length = Math.hypot(dx, dy);
  if (length < 1) return;
  const angle = Math.atan2(dy, dx);
  const head = Math.min(32, length * 0.45);
  const wing = 0.42;
  ctx.lineWidth = 6;
  ctx.lineCap = "butt";
  const inset = Math.min(head * 0.72, length - 1);
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x2 - Math.cos(angle) * inset, y2 - Math.sin(angle) * inset);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(x2, y2);
  ctx.lineTo(x2 - head * Math.cos(angle - wing), y2 - head * Math.sin(angle - wing));
  ctx.lineTo(x2 - head * Math.cos(angle + wing), y2 - head * Math.sin(angle + wing));
  ctx.closePath();
  ctx.fill();
}
