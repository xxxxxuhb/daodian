import test from "node:test";
import assert from "node:assert/strict";
import {
  boxFromPoints,
  commitTextShape,
  escapeAction,
  hitHandle,
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
} from "../src/features/shot/annotate.ts";

test("selection box follows the drag direction", () => {
  assert.deepEqual(boxFromPoints(40, 20, 10, 80), { x: 10, y: 20, w: 30, h: 60 });
});

test("a point on the selection edge still counts as inside", () => {
  const box = { x: 10, y: 20, w: 30, h: 40 };
  assert.equal(insideBox(box, 10, 20), true);
  assert.equal(insideBox(box, 40, 60), true);
  assert.equal(insideBox(box, 9, 20), false);
});

test("escape peels one layer at a time", () => {
  assert.equal(escapeAction({ typing: true, dragging: true, hasSelection: true }), "cancel-text");
  assert.equal(escapeAction({ typing: false, dragging: true, hasSelection: true }), "cancel-drag");
  assert.equal(escapeAction({ typing: false, dragging: false, hasSelection: true }), "clear-selection");
  assert.equal(escapeAction({ typing: false, dragging: false, hasSelection: false }), "exit");
});

test("text commit keeps blank input off the canvas and stores trimmed text", () => {
  const shapes = [{ kind: "rect", x: 1, y: 2, w: 3, h: 4, color: "#111111" }];
  assert.deepEqual(commitTextShape({ shapes, point: null, text: "hello", color: "#E23B3B" }), {
    shapes,
    added: false,
  });
  assert.deepEqual(commitTextShape({ shapes, point: { x: 8, y: 9 }, text: "   ", color: "#E23B3B" }), {
    shapes,
    added: false,
  });
  assert.deepEqual(commitTextShape({ shapes, point: { x: 8, y: 9 }, text: " 你好 ", color: "#E23B3B" }), {
    shapes: [...shapes, { kind: "text", x: 8, y: 9, text: "你好", color: "#E23B3B" }],
    added: true,
  });
});

test("opening the text field survives the window focus change", () => {
  assert.equal(textBlurAction({ composing: false, settling: true }), "refocus");
  assert.equal(textBlurAction({ composing: true, settling: true }), "defer");
  assert.equal(textBlurAction({ composing: false, settling: false }), "commit");
});

test("composition enter selects a candidate instead of committing", () => {
  assert.equal(textKeyAction({ composing: true, key: "Enter" }), "ignore");
  assert.equal(textKeyAction({ composing: true, key: "Escape" }), "ignore");
  assert.equal(textKeyAction({ composing: false, key: "Enter" }), "commit");
  assert.equal(textKeyAction({ composing: false, key: "Escape" }), "cancel");
  assert.equal(shouldIgnoreCompositionEnter(0), true);
  assert.equal(shouldIgnoreCompositionEnter(49), true);
  assert.equal(shouldIgnoreCompositionEnter(50), false);
  assert.equal(shouldIgnoreCompositionEnter(null), false);
});

test("a pointer that commits text does not also start a selection", () => {
  assert.equal(shouldConsumeTextPointer({ typing: true, sinceCommitMs: null }), true);
  assert.equal(shouldConsumeTextPointer({ typing: false, sinceCommitMs: 0 }), true);
  assert.equal(shouldConsumeTextPointer({ typing: false, sinceCommitMs: 219 }), true);
  assert.equal(shouldConsumeTextPointer({ typing: false, sinceCommitMs: 220 }), false);
  assert.equal(shouldConsumeTextPointer({ typing: false, sinceCommitMs: null }), false);
});

test("a click outside keeps annotations until the new selection is a real drag", () => {
  const previousSelection = { x: 10, y: 20, w: 40, h: 30 };
  const previousShapes = [{ kind: "text", x: 12, y: 22, text: "hello", color: "#111111" }];
  assert.equal(selectionDragShouldClear({ x: 0, y: 0, w: 2, h: 2 }, false), false);
  assert.equal(selectionDragShouldClear({ x: 0, y: 0, w: 4, h: 4 }, false), true);
  assert.equal(selectionDragShouldClear({ x: 0, y: 0, w: 4, h: 4 }, true), false);
  assert.deepEqual(
    selectionDragEnd({ box: { x: 80, y: 90, w: 2, h: 1 }, previousSelection, previousShapes }),
    { committed: false, selection: previousSelection, shapes: previousShapes, phase: "edit" },
  );
  assert.deepEqual(
    selectionDragEnd({ box: { x: 80, y: 90, w: 12, h: 8 }, previousSelection, previousShapes }),
    { committed: true, selection: { x: 80, y: 90, w: 12, h: 8 }, shapes: [], phase: "edit" },
  );
});

test("a tiny selection only keeps the corner handles", () => {
  const box = { x: 10, y: 10, w: 20, h: 20 };
  assert.equal(hitHandle(box, { x: 10, y: 10 }), "nw");
  assert.equal(hitHandle(box, { x: 20, y: 10 }), null);
});

test("resizing from the bottom right keeps the opposite corner", () => {
  const box = { x: 10, y: 20, w: 40, h: 30 };
  const bounds = { x: 0, y: 0, w: 400, h: 300 };
  assert.deepEqual(resizeBox(box, "se", { x: 80, y: 90 }, bounds, false), {
    x: 10,
    y: 20,
    w: 70,
    h: 70,
  });
});

test("a selection cannot be dragged off the screen", () => {
  const box = { x: 10, y: 10, w: 20, h: 20 };
  const bounds = { x: 0, y: 0, w: 100, h: 80 };
  assert.deepEqual(moveBox(box, 1000, 1000, bounds), { x: 80, y: 60, w: 20, h: 20 });
});

test("snap lands on physical pixels", () => {
  assert.deepEqual(snapBox({ x: 1.2, y: 2.2, w: 3.3, h: 4.4 }, 2), {
    x: 1,
    y: 2,
    w: 3.5,
    h: 4.5,
  });
});
