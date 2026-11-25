import test from "node:test";
import assert from "node:assert/strict";
import {
  RHYTHM,
  chooseSpecial,
  formatRecap,
  poseFor,
  rhythmFor,
} from "../src/features/pet/petRhythm.ts";

const gate = RHYTHM.normal;

function snap(over) {
  return { burst: 0, steady: null, linger: 0, stock: 0, keys: 0, rapidFor: 0, ...over };
}

test("手停下来、场上还有块，就收工", () => {
  const kind = chooseSpecial(snap({ linger: 6_200, stock: 3, keys: 24 }), 100_000, 0, {}, gate);
  assert.equal(kind, "absorb");
});

test("还在敲的时候不收工", () => {
  const kind = chooseSpecial(snap({ linger: 400, stock: 3, keys: 24 }), 100_000, 0, {}, gate);
  assert.notEqual(kind, "absorb");
});

test("敲得太少、或者场上已经空了，都不收工", () => {
  assert.notEqual(
    chooseSpecial(snap({ linger: 9_000, stock: 3, keys: 3 }), 100_000, 0, {}, gate),
    "absorb",
  );
  assert.notEqual(
    chooseSpecial(snap({ linger: 9_000, stock: 0, keys: 40 }), 100_000, 0, {}, gate),
    "absorb",
  );
});

test("爆发优先于收工", () => {
  const kind = chooseSpecial(snap({ burst: 14, linger: 6_500, stock: 4, keys: 30 }), 100_000, 0, {}, gate);
  assert.equal(kind, "boom");
});

test("收工文案：不足一分钟报秒，超过报分钟", () => {
  assert.equal(formatRecap(18_000, 42), "本次 18 秒 · 42 下");
  assert.equal(formatRecap(12 * 60_000, 640), "本次 12 分钟 · 640 下");
  assert.equal(formatRecap(400, 3), "本次 1 秒 · 3 下");
});

test("键位人格仍然按物理键码映射", () => {
  assert.equal(poseFor(0x08), "eat");
  assert.equal(poseFor(0x0d), "stamp");
  assert.equal(poseFor(0x1b), "shrink");
  assert.equal(poseFor(0x09), "hop");
  assert.equal(poseFor(0x26), "look");
  assert.equal(poseFor(0x41), null);
});

test("三档互动的门槛单调", () => {
  assert.ok(rhythmFor("calm").minKeys > rhythmFor("normal").minKeys);
  assert.ok(rhythmFor("normal").lingerMs > rhythmFor("playful").lingerMs);
});
