import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, shallowRef, watch, type Ref } from "vue";
import {
	REST_PULSE,
	SLEEP_PULSE,
	blendPulse,
	burstOf,
	chooseSpecial,
	isModifier,
	lookVector,
	formatRecap,
	poseFor,
	pulseAt,
	rateOf,
	rhythmFor,
	steadyOf,
	type KeyPose,
	type Pulse,
	type Special,
} from "./petRhythm";

const RAIN_X_MIN = 100;
const RAIN_X_SPAN = 196;
const RAIN_Y_MIN = 18;
const RAIN_Y_SPAN = 92;
const TIP_RIGHT = { x: 252, y: 229 };
const TIP_LEFT = { x: 48, y: 229 };
/** 珠体底边。锚点贴在 300 窗底，边长 96，图 92 居中，身体圆心 52、半径 16。 */
const PET_BOTTOM = 300 - 96 + (96 - 92) / 2 + ((52 + 16) * 92) / 96;
const GLYPH_HALF = 11;
const FORCED_SPECIALS = ["boom", "still", "absorb"] as const;
const EYE_RIGHT = { x: 252, y: 254 };
const EYE_LEFT = { x: 48, y: 254 };
const LOOK_LIMIT = 3;
const MAX_GLYPHS = 14;
const MAX_QUEUE = 128;
const SHOT_DELAY = 320;
const BEAM_GROW = 200;
const BEAM_LIFE = 380;
const MAX_BEAMS = 2;
const BURST_LIFE = 420;
const IDLE_HOME = 7_000;
const POSE_MERGE = 160;
const BURST_ANGLES = [20, 80, 140, 200, 260, 320];
const BOOM_WIND = 280;
const BOOM_SPAN = 640;
const BOOM_TAIL = 180;
const STILL_WIND = 180;
const STILL_SPAN = 560;
const STILL_BRAKE = 220;

interface Ink {
	ink: string;
	paper: string;
	glow: string;
	core: string;
	head: string;
}

const INKS: Ink[] = [
	{ ink: "#E23B3B", paper: "#FDECEC", glow: "#E23B3B", core: "#FFD4D4", head: "#FFF6F6" },
	{ ink: "#E07A2F", paper: "#FFF0E6", glow: "#E07A2F", core: "#FFD8BC", head: "#FFF7F1" },
	{ ink: "#E6A317", paper: "#FFF6E0", glow: "#E6A317", core: "#FFE0B0", head: "#FFF8EC" },
	{ ink: "#2F9E6B", paper: "#E5F6EE", glow: "#2F9E6B", core: "#C8F3DE", head: "#F3FFF8" },
	{ ink: "#0E8A8A", paper: "#E5F6F6", glow: "#0E8A8A", core: "#C8F0F0", head: "#F3FCFC" },
	{ ink: "#2F6FED", paper: "#E7F0FE", glow: "#2F6FED", core: "#D4E4FF", head: "#F5F8FF" },
	{ ink: "#7A4DD6", paper: "#F1EBFC", glow: "#7A4DD6", core: "#E2D6FF", head: "#F8F5FF" },
	{ ink: "#D4537E", paper: "#FDECF3", glow: "#D4537E", core: "#FFD4E4", head: "#FFF5F8" },
];

function inkFor(id: number): Ink {
	return INKS[Math.abs(id) % INKS.length];
}

interface KeyEvent {
	id: number;
	vk?: number;
	edge?: string;
	label?: string;
}

interface Glyph {
	id: number;
	label: string | null;
	wide: boolean;
	x: number;
	y: number;
	rot: number;
	vx: number;
	vy: number;
	spin: number;
	born: number;
	targeted: boolean;
	loose: boolean;
	struck: boolean;
	frozen: boolean;
	brake: number;
	brakeVx: number;
	brakeVy: number;
	brakeSpin: number;
	opacity: number;
	scale: number;
	jolt: number;
	held: boolean;
	pushed: boolean;
	dragged: boolean;
	fadeAt: number;
	job: "" | "eat" | "absorb";
	jobAt: number;
	jobDelay: number;
	jobMs: number;
	fromX: number;
	fromY: number;
	ink: Ink;
}

interface Beam {
	id: number;
	fromX: number;
	fromY: number;
	x: number;
	y: number;
	headX: number;
	headY: number;
	opacity: number;
	born: number;
	hit: boolean;
	ink: Ink;
}

interface BurstDot {
	x: number;
	y: number;
	r: number;
}

interface Burst {
	id: number;
	opacity: number;
	dots: BurstDot[];
	born: number;
	x: number;
	y: number;
	power: number;
	ink: string;
}

type Mood = "boom" | "still";

interface Wave {
	r: number;
	opacity: number;
}

interface PendingKey {
	id: number;
	label: string | null;
}

export interface ShotGate {
	hovered: boolean;
	blocked: boolean;
	night: boolean;
}

export interface PetCue {
	id: number;
	name: "stamp" | "shrink" | "nibble" | "flinch" | "fed" | "hop" | "puff" | "burp";
	ms: number;
}

export function lookToward(x: number, y: number, origin = EYE_RIGHT) {
	const dx = x - origin.x;
	const dy = y - origin.y;
	const span = Math.max(Math.abs(dx), Math.abs(dy));
	if (span < 1) return { x: 0, y: 0 };
	const scale = LOOK_LIMIT / span;
	return { x: dx * scale, y: dy * scale };
}

function quad(a: number, b: number, c: number, t: number) {
	const u = 1 - t;
	return u * u * a + 2 * u * t * b + t * t * c;
}

export function usePetShots(
	follow: Readonly<Ref<boolean>>,
	stageRight: Readonly<Ref<boolean>>,
	gate: Readonly<Ref<ShotGate>>,
	interaction: Readonly<Ref<string>>,
) {
	const glyphs = shallowRef<Glyph[]>([]);
	const beams = shallowRef<Beam[]>([]);
	const bursts = shallowRef<Burst[]>([]);
	const eye = shallowRef({ x: 0, y: 0 });
	const engaged = shallowRef(false);
	const mood = shallowRef<Mood | null>(null);
	const wave = shallowRef<Wave | null>(null);
	const pulse = shallowRef<Pulse>({ ...REST_PULSE });
	const recap = shallowRef("");
	const cue = shallowRef<PetCue | null>(null);
	const tip = computed(() => (stageRight.value ? TIP_LEFT : TIP_RIGHT));
	const shooting = computed(
		() =>
			glyphs.value.length + beams.value.length + bursts.value.length > 0 ||
			mood.value != null,
	);

	let queue: PendingKey[] = [];
	let live: Glyph[] = [];
	let liveBeams: Beam[] = [];
	let liveBursts: Burst[] = [];
	let taps: number[] = [];
	let sessionStart = 0;
	let sessionCount = 0;
	let rapidSince = 0;
	let lastSpecial = 0;
	let lastOf: Partial<Record<Special, number>> = {};
	let lastInput = 0;
	let lastShot = 0;
	let shotGap = REST_PULSE.gap;
	let raf = 0;
	let lastFrame = 0;
	let homeTimer = 0;
	let recapTimer = 0;
	let moodName: Mood | null = null;
	let moodStarted = 0;
	let boomReleased = false;
	let settling = false;
	let absorbUntil = 0;
	let pendingRecap = "";
	let shakeUntil = 0;
	let lookUntil = 0;
	let lookAt = { x: 0, y: 0 };
	let lastPose: KeyPose | "" = "";
	let lastPoseAt = 0;
	let feedCount = 0;
	let cueId = 0;
	let pulseAnchor: Pulse = { ...REST_PULSE };
	let pulseBusy = false;
	let motionOk = true;
	let motionAt = 0;
	let unlistenKey: UnlistenFn | null = null;

	function aim(x: number, y: number) {
		return lookToward(x, y, stageRight.value ? EYE_LEFT : EYE_RIGHT);
	}

	function origin() {
		return stageRight.value ? TIP_LEFT : TIP_RIGHT;
	}

	function hasScene() {
		return (
			live.length + liveBeams.length + liveBursts.length + queue.length > 0 ||
			moodName != null ||
			settling ||
			pulseBusy
		);
	}

	function publish() {
		const oldest = live.find((glyph) => !glyph.job && !glyph.pushed && !glyph.loose)?.id;
		for (const glyph of live) glyph.targeted = glyph.id === oldest;
		glyphs.value = live.slice();
		beams.value = liveBeams.slice();
		bursts.value = liveBursts.slice();
	}

	function setEye(next: { x: number; y: number }) {
		if (Math.abs(next.x - eye.value.x) < 0.05 && Math.abs(next.y - eye.value.y) < 0.05) return;
		eye.value = next;
	}

	function publishPulse(next: Pulse) {
		const prev = pulse.value;
		if (
			Math.abs(prev.dur - next.dur) < 0.03 &&
			Math.abs(prev.amp - next.amp) < 0.002 &&
			Math.abs(prev.glow - next.glow) < 0.2 &&
			Math.abs(prev.gap - next.gap) < 1
		) {
			return;
		}
		pulse.value = next;
	}

	function signal(name: PetCue["name"], ms: number) {
		cueId += 1;
		cue.value = { id: cueId, name, ms };
	}

	function publishRecap(text: string) {
		pendingRecap = "";
		recap.value = text;
		window.clearTimeout(recapTimer);
		recapTimer = window.setTimeout(() => {
			recap.value = "";
		}, 4_000);
	}

	function showRecap() {
		if (!pendingRecap) return;
		publishRecap(pendingRecap);
	}

	function armHome() {
		window.clearTimeout(homeTimer);
		homeTimer = window.setTimeout(() => {
			if (hasScene() || performance.now() - lastInput < IDLE_HOME) {
				armHome();
				return;
			}
			engaged.value = false;
			setEye({ x: 0, y: 0 });
		}, IDLE_HOME);
	}

	function spawn(key: PendingKey, now: number): Glyph {
		const label = key.label;
		return {
			id: key.id,
			label,
			wide: !!label && label.length > 1,
			x: RAIN_X_MIN + Math.random() * RAIN_X_SPAN,
			y: RAIN_Y_MIN + Math.random() * RAIN_Y_SPAN,
			rot: (Math.random() - 0.5) * 14,
			vx: (Math.random() - 0.5) * 36,
			vy: 78 + Math.random() * 34,
			spin: (Math.random() - 0.5) * 46,
			born: now,
			targeted: false,
			loose: false,
			struck: false,
			frozen: false,
			brake: 0,
			brakeVx: 0,
			brakeVy: 0,
			brakeSpin: 0,
			opacity: 1,
			scale: 1,
			jolt: 0,
			held: false,
			pushed: false,
			dragged: false,
			fadeAt: 0,
			job: "",
			jobAt: 0,
			jobDelay: 0,
			jobMs: 320,
			fromX: 0,
			fromY: 0,
			ink: inkFor(key.id),
		};
	}

	function burstAt(glyph: Glyph, now: number, power = 1): Burst {
		const dots = BURST_ANGLES.map((deg, index) => {
			const angle = (deg * Math.PI) / 180;
			const distance = (7 + (index % 3) * 2) * power;
			return {
				x: glyph.x + Math.cos(angle) * distance,
				y: glyph.y + Math.sin(angle) * distance,
				r: (1.1 + (index % 3) * 0.25) * Math.min(power, 1.6),
			};
		});
		return {
			id: glyph.id,
			opacity: 1,
			dots,
			born: now,
			x: glyph.x,
			y: glyph.y,
			power,
			ink: glyph.ink.ink,
		};
	}

	function resetMood() {
		moodName = null;
		mood.value = null;
		wave.value = null;
		boomReleased = false;
	}

	function markSpecial(kind: Special, now: number) {
		lastSpecial = now;
		lastOf = { ...lastOf, [kind]: now };
	}

	function finishMood() {
		resetMood();
	}

	function freeze(glyph: Glyph, now: number) {
		if (glyph.frozen) return;
		glyph.frozen = true;
		glyph.loose = false;
		glyph.opacity = 1;
		glyph.brake = now;
		glyph.brakeVx = glyph.vx;
		glyph.brakeVy = glyph.vy;
		glyph.brakeSpin = glyph.spin;
	}

	function beginMood(kind: Mood, now: number) {
		markSpecial(kind, now);
		moodName = kind;
		moodStarted = now;
		boomReleased = false;
		mood.value = kind;
		wave.value = null;
		for (const glyph of live) {
			if (!glyph.struck) continue;
			glyph.struck = false;
			if (glyph.vy < 40) glyph.vy = 78;
		}
		liveBeams = [];
		if (kind === "still") queue = [];
		engaged.value = true;
	}

	function startJob(glyph: Glyph, now: number, delay: number, ms: number, kind: "eat" | "absorb") {
		glyph.job = kind;
		glyph.jobAt = now;
		glyph.jobDelay = delay;
		glyph.jobMs = ms;
		glyph.fromX = glyph.x;
		glyph.fromY = glyph.y;
		glyph.vx = 0;
		glyph.vy = 0;
		glyph.spin = 0;
		glyph.struck = true;
		glyph.loose = false;
		glyph.held = false;
		glyph.dragged = false;
		glyph.pushed = false;
	}

	function runJob(glyph: Glyph, now: number) {
		const elapsed = now - glyph.jobAt - glyph.jobDelay;
		if (elapsed < 0) return true;
		const t = elapsed / glyph.jobMs;
		if (t >= 1) return false;
		const tip = origin();
		const cx = (glyph.fromX + tip.x) / 2;
		const cy = Math.min(glyph.fromY, tip.y) - 36;
		glyph.x = quad(glyph.fromX, cx, tip.x, t);
		glyph.y = quad(glyph.fromY, cy, tip.y, t);
		glyph.scale = 0.9 + (0.2 - 0.9) * t;
		glyph.opacity = 1;
		return true;
	}

	function beginAbsorb(now: number) {
		markSpecial("absorb", now);
		settling = true;
		// 这一次敲了多少、敲了多久：只在内存里留一句，悬停时才报。
		if (sessionStart && sessionCount >= 8) {
			pendingRecap = formatRecap(now - sessionStart, sessionCount);
		}
		sessionStart = 0;
		sessionCount = 0;
		while (queue.length) {
			const next = queue.shift();
			if (next) live.push(spawn(next, now));
		}
		const count = live.length;
		live.forEach((glyph, index) => {
			if (!glyph.job) startJob(glyph, now, index * 60, 500, "absorb");
		});
		absorbUntil = now + 500 + Math.max(0, count - 1) * 60 + 120;
		signal("puff", absorbUntil - now);
	}

	function allowsSpecial() {
		return (
			!document.hidden &&
			!gate.value.hovered &&
			!gate.value.blocked &&
			!(gate.value.night && !engaged.value) &&
			motionOk
		);
	}

	function maybeSpecial(now: number) {
		const rate = rateOf(taps, now);
		if (rate >= 4) {
			if (!rapidSince) rapidSince = now;
		} else rapidSince = 0;
		if (moodName || settling || !allowsSpecial()) return;
		const linger = lastInput ? now - lastInput : 0;
		const kind = chooseSpecial(
			{
				burst: burstOf(taps, now),
				steady: steadyOf(taps, now),
				linger,
				stock: live.length + queue.length,
				keys: sessionCount,
				rapidFor: rapidSince ? now - rapidSince : 0,
			},
			now,
			lastSpecial,
			lastOf,
			rhythmFor(interaction.value),
		);
		if (kind === "absorb") {
			beginAbsorb(now);
			return;
		}
		if (kind === "boom" || kind === "still") beginMood(kind, now);
	}

	function pastPetBottom(glyph: Glyph) {
		return glyph.y + GLYPH_HALF >= PET_BOTTOM;
	}

	function forceRandom(now: number) {
		if (moodName || settling || live.some((glyph) => glyph.job === "absorb")) return;
		const overdue = live.some((glyph) => !glyph.job && !glyph.dragged && glyph.opacity > 0.15 && pastPetBottom(glyph));
		if (!overdue) return;
		const kind = FORCED_SPECIALS[Math.floor(Math.random() * FORCED_SPECIALS.length)];
		if (kind === "absorb") beginAbsorb(now);
		else beginMood(kind, now);
	}

	function runMood(now: number) {
		if (moodName === "boom") {
			const age = now - moodStarted;
			if (age < BOOM_WIND) {
				wave.value = null;
				return;
			}
			if (!boomReleased) {
				boomReleased = true;
				queue = [];
			}
			const t = Math.min(1, (age - BOOM_WIND) / BOOM_SPAN);
			const eased = 1 - (1 - t) ** 2.2;
			const radius = 18 + eased * 330;
			const from = origin();
			wave.value = { r: radius, opacity: 1 - t * 0.9 };
			const keep: Glyph[] = [];
			for (const glyph of live) {
				const dx = glyph.x - from.x;
				const dy = glyph.y - from.y;
				if (dx * dx + dy * dy <= radius * radius) liveBursts.push(burstAt(glyph, now, 2.1));
				else keep.push(glyph);
			}
			live = keep;
			if (age >= BOOM_WIND + BOOM_SPAN + BOOM_TAIL) finishMood();
			return;
		}
		if (moodName === "still") {
			const age = now - moodStarted;
			if (age >= STILL_WIND) {
				const t = Math.min(1, (age - STILL_WIND) / STILL_SPAN);
				const eased = 1 - (1 - t) ** 2.2;
				const radius = 18 + eased * 330;
				const from = origin();
				wave.value = {
					r: radius,
					opacity: t > 0.82 ? 1 - (t - 0.82) / 0.18 : 0.92,
				};
				for (const glyph of live) {
					const dx = glyph.x - from.x;
					const dy = glyph.y - from.y;
					if (dx * dx + dy * dy <= radius * radius) freeze(glyph, now);
				}
			}
			const waveDone = age >= STILL_WIND + STILL_SPAN;
			if (waveDone) {
				for (const glyph of live) freeze(glyph, now);
				if (live.length === 0 && queue.length === 0) finishMood();
			}
			return;
		}
	}

	function updatePulse(now: number) {
		const sleeping = gate.value.night && !engaged.value;
		if (sleeping) {
			pulseBusy = false;
			shotGap = SLEEP_PULSE.gap;
			publishPulse(SLEEP_PULSE);
			return;
		}
		const rate = rateOf(taps, now);
		const linger = lastInput ? now - lastInput : 10_000;
		let next: Pulse;
		if (!lastInput || linger < 80) {
			next = pulseAt(rate);
			pulseAnchor = next;
			pulseBusy = false;
		} else {
			const mix = Math.min(1, linger / 4_000);
			next = blendPulse(pulseAnchor, REST_PULSE, mix);
			pulseBusy = mix < 1;
		}
		let gap = next.gap;
		const thresholds = rhythmFor(interaction.value);
		if (rapidSince && now - rapidSince >= thresholds.rapidMs) gap = Math.min(gap, 110);
		shotGap = gap;
		publishPulse(next);
	}

	function touchMotion(now: number) {
		if (now - motionAt < 800) return;
		motionAt = now;
		void invoke<boolean>("pet_can_idle")
			.then((ok) => {
				motionOk = ok;
				if (!ok) clearScene();
			})
			.catch(() => {});
	}

	function step(now: number) {
		const dt = Math.min(0.05, (now - lastFrame) / 1000 || 0.016);
		lastFrame = now;
		touchMotion(now);
		updatePulse(now);
		maybeSpecial(now);
		forceRandom(now);
		if (settling && now >= absorbUntil) settling = false;
		runMood(now);
		const jolting = now < shakeUntil;
		const keep: Glyph[] = [];
		for (const glyph of live) {
			glyph.jolt = jolting ? Math.sin(now / 45 + glyph.id) * 2 : 0;
			if (glyph.job) {
				if (runJob(glyph, now)) keep.push(glyph);
				continue;
			}
			if (glyph.dragged) {
				keep.push(glyph);
				continue;
			}
			if (glyph.frozen) {
				if (glyph.brake) {
					const t = Math.min(1, (now - glyph.brake) / STILL_BRAKE);
					const scale = (1 - t) ** 2;
					glyph.x += glyph.brakeVx * scale * dt;
					glyph.y += glyph.brakeVy * scale * dt;
					glyph.rot += glyph.brakeSpin * scale * dt;
					if (t >= 1) glyph.brake = 0;
				}
				keep.push(glyph);
				continue;
			}
			if (glyph.loose) glyph.vx += Math.sin(now / 160 + glyph.id) * 90 * dt;
			if (glyph.pushed && glyph.fadeAt) glyph.opacity = Math.max(0, 1 - (now - glyph.fadeAt) / 600);
			glyph.x += glyph.vx * dt;
			glyph.y += glyph.vy * dt;
			glyph.rot += glyph.spin * dt;
			if (glyph.loose) glyph.opacity = Math.max(0, Math.min(glyph.opacity, 1 - (glyph.y - 210) / 110));
			if (glyph.frozen || (glyph.y < 318 && glyph.opacity > 0.04)) keep.push(glyph);
		}
		live = keep;
		while (live.length < MAX_GLYPHS && queue.length && !settling && moodName !== "still") {
			const next = queue.shift();
			if (!next) break;
			live.push(spawn(next, now));
		}
		liveBeams = liveBeams.filter((beam) => now - beam.born < BEAM_LIFE);
		for (const beam of liveBeams) {
			const age = now - beam.born;
			const grown = Math.min(1, age / BEAM_GROW);
			const reach = grown * grown * (3 - 2 * grown);
			beam.headX = beam.fromX + (beam.x - beam.fromX) * reach;
			beam.headY = beam.fromY + (beam.y - beam.fromY) * reach;
			beam.opacity = age < BEAM_GROW ? 1 : 1 - (age - BEAM_GROW) / (BEAM_LIFE - BEAM_GROW);
			if (beam.hit || age < BEAM_GROW) continue;
			beam.hit = true;
			const glyph = live.find((item) => item.id === beam.id);
			if (!glyph) continue;
			liveBursts.push(burstAt(glyph, now));
			live = live.filter((item) => item.id !== beam.id);
		}
		liveBursts = liveBursts.filter((burst) => now - burst.born < BURST_LIFE);
		for (const burst of liveBursts) {
			const age = (now - burst.born) / BURST_LIFE;
			burst.opacity = 1 - age;
			burst.dots = BURST_ANGLES.map((deg, index) => {
				const angle = (deg * Math.PI) / 180;
				const distance = 4 + age * 16 * burst.power + (index % 3);
				return {
					x: burst.x + Math.cos(angle) * distance,
					y: burst.y + Math.sin(angle) * distance,
					r: 1.1 + (index % 3) * 0.25,
				};
			});
		}
		const ready =
			moodName === "still"
				? live.find((glyph) => glyph.frozen && !glyph.brake && !glyph.struck && !glyph.job)
				: moodName
					? undefined
					: live.find(
							(glyph) =>
								!glyph.loose &&
								!glyph.struck &&
								!glyph.job &&
								!glyph.pushed &&
								!glyph.dragged &&
								!glyph.held &&
								now - glyph.born >= SHOT_DELAY,
						);
		if (ready && now - lastShot >= shotGap && liveBeams.length < MAX_BEAMS) {
			lastShot = now;
			ready.struck = true;
			ready.vx = 0;
			ready.vy = 0;
			ready.spin = 0;
			const from = origin();
			liveBeams.push({
				id: ready.id,
				fromX: from.x,
				fromY: from.y,
				x: ready.x,
				y: ready.y,
				headX: from.x,
				headY: from.y,
				opacity: 1,
				born: now,
				hit: false,
				ink: ready.ink,
			});
		}
		const target = live.find((glyph) => !glyph.loose && !glyph.job && !glyph.pushed);
		if (now < lookUntil) setEye(lookAt);
		else if (moodName === "boom") setEye({ x: 0, y: -2.2 });
		else if (moodName === "still" && !target) setEye({ x: 0, y: -1.2 });
		else if (target) setEye(aim(target.x, target.y));
		else setEye({ x: 0, y: 0 });
		publish();
		if (hasScene()) raf = requestAnimationFrame(step);
		else raf = 0;
	}

	function ensure(now: number) {
		if (raf) return;
		lastFrame = now;
		raf = requestAnimationFrame(step);
	}

	function noteTap(now: number) {
		lastInput = now;
		taps.push(now);
		const cut = now - 20_000;
		while (taps.length && taps[0] < cut) taps.shift();
		if (!sessionStart) sessionStart = now;
		sessionCount += 1;
		engaged.value = true;
		armHome();
	}

	function enqueue(key: PendingKey, now: number) {
		if (!follow.value || document.hidden || moodName === "still" || settling) return;
		queue.push(key);
		if (queue.length > MAX_QUEUE) queue.splice(0, queue.length - MAX_QUEUE);
		ensure(now);
	}

	function eatNearest(now: number) {
		const eligible = live.filter((glyph) => !glyph.job && !glyph.pushed && !glyph.loose);
		if (!eligible.length) return false;
		const glyph = eligible.reduce((latest, item) => (item.born > latest.born ? item : latest));
		if (now - glyph.born < 140) return false;
		startJob(glyph, now, 0, 320, "eat");
		return true;
	}

	function applyPose(pose: KeyPose, vk: number, now: number) {
		if (pose === "eat") {
			if (!eatNearest(now)) return false;
			signal("nibble", 320);
			return true;
		}
		if (pose === "stamp") {
			shakeUntil = now + 400;
			signal("stamp", 400);
			return true;
		}
		if (pose === "shrink") {
			signal("shrink", 280);
			return true;
		}
		if (pose === "hop") {
			const oldest = live.find((glyph) => !glyph.job && !glyph.pushed);
			if (oldest) oldest.x += 12;
			signal("hop", 500);
			return true;
		}
		const vector = lookVector(vk);
		if (vector) {
			lookAt = vector;
			lookUntil = now + 400;
		}
		return true;
	}

	function onKey(payload: KeyEvent) {
		if (!follow.value || document.hidden || !payload) return;
		const now = performance.now();
		const vk = payload.vk ?? 0;
		if (payload.edge === "up") return;
		if (settling || moodName === "still") return;
		if (isModifier(vk)) return;
		noteTap(now);
		const pose = poseFor(vk);
		if (pose) {
			const stacks = pose === "stamp" || pose === "shrink" || pose === "hop";
			if (stacks && pose === lastPose && now - lastPoseAt < POSE_MERGE) {
				ensure(now);
				return;
			}
			if (applyPose(pose, vk, now)) {
				if (stacks) {
					lastPose = pose;
					lastPoseAt = now;
				}
				ensure(now);
				return;
			}
		}
		const label = typeof payload.label === "string" && payload.label ? payload.label : null;
		enqueue({ id: payload.id, label }, now);
	}

	function holdGlyph(id: number) {
		const glyph = live.find((item) => item.id === id);
		if (!glyph || glyph.job) return;
		glyph.dragged = true;
		glyph.held = true;
		glyph.vx = 0;
		glyph.vy = 0;
		glyph.spin = 0;
		ensure(performance.now());
	}

	function moveGlyph(id: number, x: number, y: number) {
		const glyph = live.find((item) => item.id === id && item.dragged);
		if (!glyph) return;
		glyph.x = x;
		glyph.y = y;
		publish();
	}

	function dropGlyph(id: number) {
		const glyph = live.find((item) => item.id === id);
		if (!glyph) return;
		glyph.dragged = false;
		glyph.held = false;
		glyph.vy = 78;
		ensure(performance.now());
	}

	function popGlyph(id: number) {
		const now = performance.now();
		const glyph = live.find((item) => item.id === id);
		if (!glyph || glyph.job) return;
		liveBursts.push(burstAt(glyph, now));
		live = live.filter((item) => item.id !== id);
		lookAt = aim(glyph.x, glyph.y);
		lookUntil = now + 400;
		signal("flinch", 60);
		publish();
		ensure(now);
	}

	function feedGlyph(id: number) {
		const now = performance.now();
		const glyph = live.find((item) => item.id === id);
		if (!glyph || glyph.job) return;
		startJob(glyph, now, 0, 320, "eat");
		feedCount += 1;
		signal("fed", 280);
		if (feedCount >= 5) {
			feedCount = 0;
			signal("burp", 900);
		}
		ensure(now);
	}

	function clearBoard() {
		queue = [];
		live = [];
		liveBeams = [];
		liveBursts = [];
		resetMood();
		settling = false;
		if (raf) cancelAnimationFrame(raf);
		raf = 0;
		window.clearTimeout(homeTimer);
		publish();
		engaged.value = false;
		setEye({ x: 0, y: 0 });
	}

	function clearScene() {
		clearBoard();
		taps = [];
		sessionStart = 0;
		sessionCount = 0;
		rapidSince = 0;
		lastSpecial = 0;
		lastOf = {};
		feedCount = 0;
		lastInput = 0;
		pendingRecap = "";
		recap.value = "";
		window.clearTimeout(recapTimer);
		pulseAnchor = { ...REST_PULSE };
		pulseBusy = false;
		shotGap = REST_PULSE.gap;
		publishPulse(REST_PULSE);
	}

	function onHidden() {
		if (document.hidden) clearScene();
	}

	watch(follow, (value) => {
		if (!value) clearScene();
	});
	watch(stageRight, () => {
		clearBoard();
	});

	onMounted(() => {
		document.addEventListener("visibilitychange", onHidden);
		void listen<KeyEvent>("pet-key", ({ payload }) => {
			if (!payload || typeof payload.id !== "number") return;
			onKey(payload);
		})
			.then((unlisten) => {
				unlistenKey = unlisten;
			})
			.catch(() => {});
	});

	onBeforeUnmount(() => {
		document.removeEventListener("visibilitychange", onHidden);
		unlistenKey?.();
		if (raf) cancelAnimationFrame(raf);
		window.clearTimeout(homeTimer);
		window.clearTimeout(recapTimer);
	});

	return {
		glyphs,
		beams,
		bursts,
		eye,
		engaged,
		shooting,
		tip,
		mood,
		wave,
		pulse,
		recap,
		cue,
		showRecap,
		holdGlyph,
		moveGlyph,
		dropGlyph,
		popGlyph,
		feedGlyph,
	};
}
