export type Interaction = "calm" | "normal" | "playful";
export type Special = "boom" | "still" | "absorb";
export type KeyPose = "eat" | "stamp" | "shrink" | "hop" | "look";

export interface RhythmThresholds {
	burst: number;
	flowMs: number;
	steadyMax: number;
	lingerMs: number;
	minKeys: number;
	rapidMs: number;
}

export interface Pulse {
	dur: number;
	amp: number;
	glow: number;
	gap: number;
}

export interface RhythmSnapshot {
	burst: number;
	steady: number | null;
	linger: number;
	stock: number;
	keys: number;
	rapidFor: number;
}

export const REST_PULSE: Pulse = { dur: 3.6, amp: 1.02, glow: 4, gap: 150 };
export const SLEEP_PULSE: Pulse = { dur: 3.6, amp: 1.02, glow: 0, gap: 150 };

/** 平常档的心流用验收的 20 秒；安静档保留规则表里的 40 秒。 */
export const RHYTHM: Record<Interaction, RhythmThresholds> = {
	calm: { burst: 16, flowMs: 40_000, steadyMax: 80, lingerMs: 8_000, minKeys: 30, rapidMs: 16_000 },
	normal: { burst: 12, flowMs: 20_000, steadyMax: 140, lingerMs: 6_000, minKeys: 20, rapidMs: 12_000 },
	playful: { burst: 8, flowMs: 12_000, steadyMax: 180, lingerMs: 4_000, minKeys: 12, rapidMs: 6_000 },
};

const COOLDOWN: Record<Exclude<Special, "absorb">, number> = {
	boom: 90_000,
	still: 120_000,
};

const SPECIAL_GAP = 20_000;

const STOPS: Pulse[] = [
	{ dur: 3.6, amp: 1.02, glow: 4, gap: 150 },
	{ dur: 2.7, amp: 1.03, glow: 6, gap: 140 },
	{ dur: 2.0, amp: 1.04, glow: 8, gap: 125 },
	{ dur: 1.5, amp: 1.05, glow: 9, gap: 110 },
];
const STOP_RATES = [0, 4, 7, 10];

export function rhythmFor(value: string): RhythmThresholds {
	if (value === "calm" || value === "playful" || value === "normal") return RHYTHM[value];
	return RHYTHM.normal;
}

export function isModifier(vk: number) {
	return vk === 0x10 || vk === 0x11 || vk === 0x12 || vk === 0x5b || vk === 0x5c || vk === 0xa4 || vk === 0xa5;
}

export function poseFor(vk: number): KeyPose | null {
	switch (vk) {
		case 0x08:
		case 0x2e:
			return "eat";
		case 0x0d:
			return "stamp";
		case 0x1b:
			return "shrink";
		case 0x09:
			return "hop";
		case 0x25:
		case 0x26:
		case 0x27:
		case 0x28:
			return "look";
		default:
			return null;
	}
}

export function lookVector(vk: number) {
	switch (vk) {
		case 0x25:
			return { x: -3, y: 0 };
		case 0x27:
			return { x: 3, y: 0 };
		case 0x26:
			return { x: 0, y: -3 };
		case 0x28:
			return { x: 0, y: 3 };
		default:
			return null;
	}
}

function countSince(times: number[], now: number, windowMs: number) {
	const cut = now - windowMs;
	let count = 0;
	for (let index = times.length - 1; index >= 0; index -= 1) {
		if (times[index] < cut) break;
		count += 1;
	}
	return count;
}

export function rateOf(times: number[], now: number) {
	return countSince(times, now, 3_000) / 3;
}

export function burstOf(times: number[], now: number) {
	return countSince(times, now, 1_500);
}

export function steadyOf(times: number[], now: number) {
	const cut = now - 20_000;
	let start = 0;
	while (start < times.length && times[start] < cut) start += 1;
	if (times.length - start < 8) return null;
	const gaps: number[] = [];
	for (let index = start + 1; index < times.length; index += 1) {
		gaps.push(times[index] - times[index - 1]);
	}
	const mean = gaps.reduce((sum, gap) => sum + gap, 0) / gaps.length;
	const variance = gaps.reduce((sum, gap) => sum + (gap - mean) ** 2, 0) / gaps.length;
	return Math.sqrt(variance);
}

export function pulseAt(rate: number): Pulse {
	const value = Math.max(0, rate);
	if (value >= 10) return { ...STOPS[3] };
	for (let index = 0; index < STOP_RATES.length - 1; index += 1) {
		const left = STOP_RATES[index];
		const right = STOP_RATES[index + 1];
		if (value > right) continue;
		const t = (value - left) / (right - left);
		const from = STOPS[index];
		const to = STOPS[index + 1];
		return {
			dur: from.dur + (to.dur - from.dur) * t,
			amp: from.amp + (to.amp - from.amp) * t,
			glow: from.glow + (to.glow - from.glow) * t,
			gap: from.gap + (to.gap - from.gap) * t,
		};
	}
	return { ...STOPS[3] };
}

export function blendPulse(from: Pulse, to: Pulse, t: number): Pulse {
	const mix = Math.min(1, Math.max(0, t));
	return {
		dur: from.dur + (to.dur - from.dur) * mix,
		amp: from.amp + (to.amp - from.amp) * mix,
		glow: from.glow + (to.glow - from.glow) * mix,
		gap: from.gap + (to.gap - from.gap) * mix,
	};
}

export function chooseSpecial(
	snap: RhythmSnapshot,
	now: number,
	lastAt: number,
	lastOf: Partial<Record<Special, number>>,
	gate: RhythmThresholds,
): Special | null {
	const cooled = (kind: Exclude<Special, "absorb">) => {
		const previous = lastOf[kind];
		if (previous != null && now - previous < COOLDOWN[kind]) return false;
		if (!lastAt) return true;
		return now - lastAt >= SPECIAL_GAP;
	};
	if (snap.burst >= gate.burst && cooled("boom")) return "boom";
	if (
		snap.rapidFor >= gate.flowMs &&
		snap.steady != null &&
		snap.steady <= gate.steadyMax &&
		cooled("still")
	) {
		return "still";
	}
	// 收工不是"大招"：手停了、场上还有块，就是一次会话的句号，没有冷却。
	if (snap.stock > 0 && snap.keys >= gate.minKeys && snap.linger >= gate.lingerMs) {
		return "absorb";
	}
	return null;
}

export function formatRecap(ms: number, count: number) {
	if (ms < 60_000) return `本次 ${Math.max(1, Math.round(ms / 1000))} 秒 · ${count} 下`;
	return `本次 ${Math.max(1, Math.round(ms / 60_000))} 分钟 · ${count} 下`;
}
