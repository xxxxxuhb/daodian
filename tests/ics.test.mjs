import test from "node:test";
import assert from "node:assert/strict";
import { buildPlanCalendar, planCalendarFileName } from "../src/domain/ics.ts";

const plan = {
  id: "plan-1",
  title: "发布 / 1.0",
  note: "包含,逗号;分号",
  updatedAt: Date.UTC(2026, 9, 23, 8),
  stages: [
    { id: "stage-a", title: "提测", at: Date.UTC(2026, 9, 24, 2) },
    { id: "stage-b", title: "待定", at: null },
  ],
};

test("plan calendar exports only timed stages with stable UID", () => {
  const first = buildPlanCalendar(plan);
  const second = buildPlanCalendar({ ...plan, title: "改名后" });
  assert.match(first, /UID:plan-1\.stage-a@daodian/);
  assert.match(second, /UID:plan-1\.stage-a@daodian/);
  assert.equal((first.match(/BEGIN:VEVENT/g) ?? []).length, 1);
  assert.doesNotMatch(first, /待定/);
  assert.match(first, /DESCRIPTION:包含\\,逗号\\;分号/);
  assert.ok(first.endsWith("\r\n"));
});

test("plan calendar filename removes Windows reserved characters", () => {
  assert.equal(planCalendarFileName("发布 / 1.0"), "发布 _ 1.0.ics");
  assert.equal(planCalendarFileName(" ... "), "计划.ics");
});
