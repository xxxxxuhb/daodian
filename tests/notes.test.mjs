import test from "node:test";
import assert from "node:assert/strict";
import { describeDoc, filterNotes, matchScore } from "../src/features/notes/logic.ts";

const now = new Date(2026, 10, 2, 9).getTime();

function note(patch) {
  return {
    id: "n1",
    title: "提测清单",
    summary: "回归登录",
    text: "回归登录、订单和支付",
    tags: ["版本"],
    dueAt: null,
    pinned: false,
    archived: false,
    reminded: false,
    attachments: [],
    createdAt: now - 86_400_000,
    updatedAt: now - 3_600_000,
    ...patch,
  };
}

test("search ranks title above body", () => {
  const titled = note({ id: "a", title: "支付超时", summary: "别的", text: "别的" });
  const body = note({ id: "b", title: "周报", summary: "支付超时", text: "支付超时" });
  assert.equal(matchScore(titled, "支付"), 3);
  assert.equal(matchScore(body, "支付"), 1);
  const found = filterNotes([body, titled], "all", "支付", now);
  assert.deepEqual(found.map((item) => item.id), ["a", "b"]);
});

test("filters hide archived notes unless asked", () => {
  const live = note({ id: "live" });
  const archived = note({ id: "old", archived: true, title: "旧笔记" });
  const due = note({ id: "due", dueAt: now + 60_000, title: "到点" });
  assert.deepEqual(filterNotes([live, archived, due], "all", "", now).map((item) => item.id), ["live", "due"]);
  assert.deepEqual(filterNotes([live, archived, due], "archived", "", now).map((item) => item.id), ["old"]);
  assert.deepEqual(filterNotes([live, archived, due], "due", "", now).map((item) => item.id), ["due"]);
});

test("describeDoc keeps the first image and the plain text", () => {
  const described = describeDoc({
    type: "doc",
    content: [
      { type: "paragraph", content: [{ type: "text", text: "先看登录" }] },
      { type: "image", attrs: { hash: "a".repeat(64) } },
      { type: "image", attrs: { hash: "b".repeat(64) } },
    ],
  });
  assert.equal(described.summary.includes("先看登录"), true);
  assert.equal(described.thumb, "a".repeat(64));
  assert.deepEqual(described.attachments, ["a".repeat(64), "b".repeat(64)]);
});
