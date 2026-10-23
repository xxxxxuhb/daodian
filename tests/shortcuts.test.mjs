import test from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_WEEK_REPORT_TEMPLATE,
  renderWeekReport,
  weekSummary,
} from "../src/domain/present.ts";
import { isContainerShortcut, isEditableTarget } from "../src/domain/shortcuts.ts";

function keyEvent(overrides = {}) {
  const container = { tagName: "ARTICLE" };
  return {
    key: "r",
    target: container,
    currentTarget: container,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...overrides,
  };
}

test("editable controls are excluded from shortcuts", () => {
  assert.equal(isEditableTarget({ tagName: "input" }), true);
  assert.equal(isEditableTarget({ tagName: "TEXTAREA" }), true);
  assert.equal(isEditableTarget({ tagName: "DIV", isContentEditable: true }), true);
  assert.equal(isEditableTarget({ tagName: "ARTICLE" }), false);
});

test("shortcut only fires on the focused container", () => {
  assert.equal(isContainerShortcut(keyEvent(), "R"), true);
  assert.equal(
    isContainerShortcut(keyEvent({ target: { tagName: "BUTTON" } }), "r"),
    false,
  );
});

test("shortcut rejects conflicts and modifiers", () => {
  assert.equal(isContainerShortcut(keyEvent(), "r", true), false);
  assert.equal(isContainerShortcut(keyEvent({ ctrlKey: true }), "r"), false);
  assert.equal(isContainerShortcut(keyEvent({ key: "t" }), "r"), false);
});

test("weekly report counts only stages completed this week", () => {
  const now = new Date(2026, 9, 21, 12).getTime();
  const thisWeek = new Date(2026, 9, 20, 10).getTime();
  const lastWeek = new Date(2026, 9, 12, 10).getTime();
  const report = weekSummary(
    [{ title: "交付说明", done: true, updatedAt: thisWeek, dueAt: thisWeek }],
    [
      {
        title: "版本发布",
        done: false,
        stages: [
          { title: "历史设计", done: true, completedAt: lastWeek, at: lastWeek },
          { title: "本周联调", done: true, completedAt: thisWeek, at: thisWeek },
          { title: "当前验收", done: false, completedAt: null, at: null },
        ],
      },
    ],
    now,
  );

  assert.equal(report.advanced, 1);
  assert.match(report.planText, /本周联调/);
  assert.match(report.planText, /当前验收/);
  assert.doesNotMatch(report.planText, /历史设计/);
});

test("weekly report template replaces every supported variable", () => {
  const text = renderWeekReport(DEFAULT_WEEK_REPORT_TEMPLATE, {
    overview: "概览内容",
    todoText: "待办内容",
    planText: "计划内容",
  });
  assert.match(text, /概览内容/);
  assert.match(text, /待办内容/);
  assert.match(text, /计划内容/);
});
