import test from "node:test";
import assert from "node:assert/strict";
import { describeDoc, filterNotes, matchScore, noteTagEntries } from "../src/features/notes/logic.ts";

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

test("sidebar tags follow notes and keep new ones at the end", () => {
  const first = note({ id: "a", createdAt: now - 2 * 86_400_000, tags: ["版本"] });
  const second = note({ id: "b", createdAt: now - 86_400_000, tags: ["会议", "版本"] });
  const archived = note({ id: "c", archived: true, tags: ["旧"] });
  assert.deepEqual(noteTagEntries([], [second, first, archived]), [
    { tag: "版本", count: 2 },
    { tag: "会议", count: 1 },
  ]);
  assert.deepEqual(noteTagEntries(["会议", "版本"], [first, second, note({ id: "d", tags: ["发布"] })]).map((item) => item.tag), ["会议", "版本", "发布"]);
  assert.deepEqual(filterNotes([first, second], "all", "", now, "会议").map((item) => item.id), ["b"]);
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
