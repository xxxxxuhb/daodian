import test from "node:test";
import assert from "node:assert/strict";
import { visiblePlans } from "../src/features/plans/logic.ts";

function stage(id, at) {
  return { id, title: id, at, done: false, reminded: false, dependsOn: null, completedAt: null };
}

function plan(partial) {
  return {
    id: partial.id,
    title: partial.id,
    note: "",
    priority: partial.priority ?? "normal",
    stages: partial.stages ?? [],
    done: partial.done ?? false,
    createdAt: 1,
    updatedAt: 1,
    templateId: null,
  };
}

const plans = [
  plan({ id: "active-in", stages: [stage("a", 1500)] }),
  plan({ id: "active-out", stages: [stage("b", 3000)] }),
  plan({ id: "active-undated", stages: [stage("c", null)] }),
  plan({ id: "done-in", done: true, stages: [stage("d", 1500)] }),
  plan({ id: "important-out", priority: "high", stages: [stage("e", 3000)] }),
];

function ids(filter, range, important = false) {
  return visiblePlans(plans, {
    filter,
    range,
    rangeStart: 1000,
    rangeEnd: 2000,
    important,
    query: "",
  }).map((item) => item.id).sort();
}

test("all plans ignores in-progress and completed", () => {
  const everything = ["active-in", "active-out", "active-undated", "done-in", "important-out"];
  assert.deepEqual(ids("done", "all"), everything);
  assert.deepEqual(ids("active", "all"), everything);
});

test("this week stays inside the current status and the week window", () => {
  assert.deepEqual(ids("active", "week"), ["active-in", "active-undated"]);
  assert.deepEqual(ids("done", "week"), ["done-in"]);
});

test("important only stacks on the current status and range", () => {
  assert.deepEqual(ids("active", "all", true), ["important-out"]);
  assert.deepEqual(ids("done", "all", true), ["important-out"]);
  assert.deepEqual(ids("active", "week", true), []);
  assert.deepEqual(ids("done", "week", true), []);
});
