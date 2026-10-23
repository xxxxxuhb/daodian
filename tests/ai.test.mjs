import test from "node:test";
import assert from "node:assert/strict";
import {
  applyTodoPreview,
  generatedPlanFields,
  shouldParseTodo,
  todoComposerAction,
} from "../src/domain/ai.ts";

test("todo AI avoids short titles and requires configured opt-in", () => {
  assert.equal(shouldParseTodo("买牛奶", true, true), false);
  assert.equal(shouldParseTodo("明天下午三点提交周报", true, true), true);
  assert.equal(shouldParseTodo("明天下午三点提交周报", false, true), false);
  assert.equal(shouldParseTodo("明天下午三点提交周报", true, false), false);
});

test("Tab accepts a preview while Enter always commits", () => {
  assert.equal(todoComposerAction({ key: "Tab" }, true), "accept");
  assert.equal(todoComposerAction({ key: "Tab" }, false), null);
  assert.equal(todoComposerAction({ key: "Enter" }, true), "commit");
  assert.equal(todoComposerAction({ key: "Enter", shiftKey: true }, true), null);
});

test("todo preview maps title and due date into composer fields", () => {
  const accepted = applyTodoPreview({
    title: " 提交周报 ",
    dueAt: new Date(2026, 9, 23, 15, 30).getTime(),
  });
  assert.equal(accepted.title, "提交周报");
  assert.equal(accepted.chip, "custom");
  assert.match(accepted.custom, /^2026-10-23T15:30$/);

  assert.deepEqual(applyTodoPreview({ title: "整理资料", dueAt: null }), {
    title: "整理资料",
    chip: "none",
    custom: "",
  });
});

test("generated plan maps into a new editable draft shape", () => {
  let key = 0;
  const result = generatedPlanFields(
    {
      title: " 发布 ",
      note: " 可调整 ",
      priority: "high",
      stages: [{ title: " 提测 ", at: 123 }],
    },
    () => `key-${++key}`,
  );
  assert.equal(result.title, "发布");
  assert.equal(result.note, "可调整");
  assert.equal(result.stages[0].key, "key-1");
  assert.equal(result.stages[0].id, null);
  assert.equal(result.stages[0].done, false);
});
