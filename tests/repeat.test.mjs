import test from "node:test";
import assert from "node:assert/strict";
import { nextOccurrence } from "../src/domain/repeat.ts";

function daily(skipWeekends = false) {
  return {
    rule: "daily",
    weekday: null,
    intervalDays: null,
    until: null,
    skipWeekends,
  };
}

test("repeat preview skips configured holidays", () => {
  const due = new Date(2026, 9, 22, 9).getTime();
  const next = nextOccurrence(daily(), due, {
    holidays: ["2026-10-23"],
    workdays: [],
  });
  assert.equal(new Date(next).getDate(), 24);
});

test("adjusted workday overrides weekend and holiday", () => {
  const due = new Date(2026, 9, 23, 9).getTime();
  const next = nextOccurrence(daily(true), due, {
    holidays: ["2026-10-24"],
    workdays: ["2026-10-24"],
  });
  assert.equal(new Date(next).getDate(), 24);
});

test("month end moves backward across configured holidays", () => {
  const due = new Date(2026, 0, 31, 9).getTime();
  const repeat = { ...daily(true), rule: "monthEnd" };
  const next = nextOccurrence(repeat, due, {
    holidays: ["2026-02-27"],
    workdays: [],
  });
  assert.equal(new Date(next).toLocaleDateString("sv-SE"), "2026-02-26");
});
