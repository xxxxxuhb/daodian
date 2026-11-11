import test from "node:test";
import assert from "node:assert/strict";
import { reminderStillPending } from "../src/domain/reminderSync.ts";

const todo = (done, dueAt) => [{ id: "1", done, dueAt }];
const plans = (done, at) => [{ id: "p", done: false, stages: [{ id: "s", done, at }] }];

test("an open reminder stays until it is completed or rescheduled", () => {
  const hit = { key: "todo:1:1000", todoId: "1" };
  assert.equal(reminderStillPending(hit, todo(false, 1000), [], []), true);
  assert.equal(reminderStillPending(hit, todo(true, 1000), [], []), false);
  assert.equal(reminderStillPending(hit, todo(false, 2000), [], []), false);
  assert.equal(reminderStillPending(hit, [], [], []), false);
});

test("a stage reminder follows the stage, not the other status", () => {
  const hit = { key: "stage:p:s:1000", planId: "p", stageId: "s" };
  assert.equal(reminderStillPending(hit, [], plans(false, 1000), []), true);
  assert.equal(reminderStillPending(hit, [], plans(true, 1000), []), false);
  assert.equal(reminderStillPending(hit, [], plans(false, 2500), []), false);
});

test("a note reminder closes when the due time changes", () => {
  const hit = { key: "note:n:1000", noteId: "n" };
  assert.equal(
    reminderStillPending(hit, [], [], [{ id: "n", archived: false, dueAt: 1000 }]),
    true,
  );
  assert.equal(
    reminderStillPending(hit, [], [], [{ id: "n", archived: false, dueAt: 4000 }]),
    false,
  );
});
