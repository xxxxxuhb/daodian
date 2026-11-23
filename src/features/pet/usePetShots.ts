import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
	computed,
	onBeforeUnmount,
	onMounted,
	shallowRef,
	watch,
	type Ref,
} from "vue";

const RAIN_X_MIN = 100;
const RAIN_X_SPAN = 196;
const RAIN_Y_MIN = 18;
const RAIN_Y_SPAN = 92;
const TIP_RIGHT = { x: 252, y: 229 };
const TIP_LEFT = { x: 48, y: 229 };
const EYE_RIGHT = { x: 252, y: 254 };
const EYE_LEFT = { x: 48, y: 254 };
const LOOK_LIMIT = 3;
const MAX_GLYPHS = 14;
const MAX_QUEUE = 128;
const SHOT_DELAY = 320;
const SHOT_GAP = 150;
const BEAM_GROW = 200;
const BEAM_LIFE = 380;
const MAX_BEAMS = 2;
const BURST_LIFE = 420;
const IDLE_HOME = 7_000;

const BURST_ANGLES = [20, 80, 140, 200, 260, 320];
const CROWD = 8;
const BOOM_WIND = 280;
const BOOM_SPAN = 640;
const BOOM_TAIL = 180;
const SLACK_LIFE = 1400;
const STILL_WIND = 180;
const STILL_SPAN = 560;
const STILL_BRAKE = 220;
const SPECIAL_GAP = 3200;

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

type Mood = "boom" | "slack" | "still";

interface Wave {
	r: number;
	opacity: number;
}

interface PendingKey {
	id: number;
	label: string | null;
}

export function lookToward(x: number, y: number, origin = EYE_RIGHT) {
	const dx = x - origin.x;
	const dy = y - origin.y;
	const span = Math.max(Math.abs(dx), Math.abs(dy));
	if (span < 1) return { x: 0, y: 0 };
	const scale = LOOK_LIMIT / span;
	return { x: dx * scale, y: dy * scale };
}

export function usePetShots(
	follow: Readonly<Ref<boolean>>,
	stageRight: Readonly<Ref<boolean>>,
) {
	const glyphs = shallowRef<Glyph[]>([]);
	const beams = shallowRef<Beam[]>([]);
	const bursts = shallowRef<Burst[]>([]);
	const eye = shallowRef({ x: 0, y: 0 });
	const engaged = shallowRef(false);
	const mood = shallowRef<Mood | null>(null);
	const wave = shallowRef<Wave | null>(null);
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

	function aim(x: number, y: number) {
		return lookToward(x, y, stageRight.value ? EYE_LEFT : EYE_RIGHT);
	}
	let lastInput = 0;
	let lastShot = 0;
	let raf = 0;
	let lastFrame = 0;
	let homeTimer = 0;
	let moodName: Mood | null = null;
	let moodStarted = 0;
	let nextRoll = 0;
	let boomReleased = false;
	let unlistenKey: UnlistenFn | null = null;

	function hasScene() {
		return (
			live.length + liveBeams.length + liveBursts.length + queue.length > 0 ||
			moodName != null
		);
	}

	function publish() {
		const oldest = live[0]?.id;
		for (const glyph of live) glyph.targeted = glyph.id === oldest;
		glyphs.value = live.slice();
		beams.value = liveBeams.slice();
		bursts.value = liveBursts.slice();
	}

	function setEye(next: { x: number; y: number }) {
		if (
			Math.abs(next.x - eye.value.x) < 0.05 &&
			Math.abs(next.y - eye.value.y) < 0.05
		)
			return;
		eye.value = next;
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

	function finishMood(now: number) {
		resetMood();
		nextRoll = now + SPECIAL_GAP;
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

	function release(glyph: Glyph) {
		glyph.loose = true;
		glyph.targeted = false;
		glyph.spin = (Math.random() < 0.5 ? -1 : 1) * (160 + Math.random() * 120);
		glyph.vx = (Math.random() - 0.5) * 70;
		glyph.vy = 36 + Math.random() * 28;
	}

	function beginMood(kind: Mood, now: number) {
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
		engaged.value = true;
		if (kind === "slack") {
			queue = [];
			for (const glyph of live) release(glyph);
		}
	}

	function maybeSpecial(now: number) {
		if (moodName || now < nextRoll) return;
		if (live.length + queue.length < CROWD) return;
		nextRoll = now + 900 + Math.random() * 800;
		if (Math.random() > 0.34) return;
		const roll = Math.random();
		const kind: Mood = roll < 1 / 3 ? "boom" : roll < 2 / 3 ? "slack" : "still";
		beginMood(kind, now);
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
			const origin = stageRight.value ? TIP_LEFT : TIP_RIGHT;
			wave.value = { r: radius, opacity: 1 - t * 0.9 };
			const keep: Glyph[] = [];
			for (const glyph of live) {
				const dx = glyph.x - origin.x;
				const dy = glyph.y - origin.y;
				if (dx * dx + dy * dy <= radius * radius)
					liveBursts.push(burstAt(glyph, now, 2.1));
				else keep.push(glyph);
			}
			live = keep;
			if (age >= BOOM_WIND + BOOM_SPAN + BOOM_TAIL) finishMood(now);
			return;
		}
		if (moodName === "still") {
			const age = now - moodStarted;
			if (age >= STILL_WIND) {
				const t = Math.min(1, (age - STILL_WIND) / STILL_SPAN);
				const eased = 1 - (1 - t) ** 2.2;
				const radius = 18 + eased * 330;
				const origin = stageRight.value ? TIP_LEFT : TIP_RIGHT;
				wave.value = {
					r: radius,
					opacity: t > 0.82 ? 1 - (t - 0.82) / 0.18 : 0.92,
				};
				for (const glyph of live) {
					const dx = glyph.x - origin.x;
					const dy = glyph.y - origin.y;
					if (dx * dx + dy * dy <= radius * radius) freeze(glyph, now);
				}
			}
			const waveDone = age >= STILL_WIND + STILL_SPAN;
			if (waveDone) {
				for (const glyph of live) freeze(glyph, now);
				if (live.length === 0 && queue.length === 0) finishMood(now);
			}
			return;
		}
		if (moodName === "slack" && now - moodStarted >= SLACK_LIFE)
			finishMood(now);
	}

	function step(now: number) {
		const dt = Math.min(0.05, (now - lastFrame) / 1000 || 0.016);
		lastFrame = now;
		maybeSpecial(now);
		runMood(now);
		for (const glyph of live) {
			if (glyph.frozen) {
				if (glyph.brake) {
					const t = Math.min(1, (now - glyph.brake) / STILL_BRAKE);
					const scale = (1 - t) ** 2;
					glyph.x += glyph.brakeVx * scale * dt;
					glyph.y += glyph.brakeVy * scale * dt;
					glyph.rot += glyph.brakeSpin * scale * dt;
					if (t >= 1) glyph.brake = 0;
				}
				continue;
			}
			if (glyph.loose) glyph.vx += Math.sin(now / 160 + glyph.id) * 90 * dt;
			glyph.x += glyph.vx * dt;
			glyph.y += glyph.vy * dt;
			glyph.rot += glyph.spin * dt;
			if (glyph.loose)
				glyph.opacity = Math.max(
					0,
					Math.min(glyph.opacity, 1 - (glyph.y - 210) / 110),
				);
		}
		live = live.filter(
			(glyph) => glyph.frozen || (glyph.y < 318 && glyph.opacity > 0.04),
		);
		while (live.length < MAX_GLYPHS && queue.length) {
			const next = queue.shift();
			if (!next) break;
			const glyph = spawn(next, now);
			if (moodName === "still") freeze(glyph, now);
			live.push(glyph);
		}
		liveBeams = liveBeams.filter((beam) => now - beam.born < BEAM_LIFE);
		for (const beam of liveBeams) {
			const age = now - beam.born;
			const grown = Math.min(1, age / BEAM_GROW);
			const reach = grown * grown * (3 - 2 * grown);
			beam.headX = beam.fromX + (beam.x - beam.fromX) * reach;
			beam.headY = beam.fromY + (beam.y - beam.fromY) * reach;
			beam.opacity =
				age < BEAM_GROW
					? 1
					: 1 - (age - BEAM_GROW) / (BEAM_LIFE - BEAM_GROW);
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
				? live.find((glyph) => glyph.frozen && !glyph.brake && !glyph.struck)
				: moodName
					? undefined
					: live.find(
							(glyph) =>
								!glyph.loose &&
								!glyph.struck &&
								now - glyph.born >= SHOT_DELAY,
						);
		if (ready && now - lastShot >= SHOT_GAP && liveBeams.length < MAX_BEAMS) {
			lastShot = now;
			ready.struck = true;
			ready.vx = 0;
			ready.vy = 0;
			ready.spin = 0;
			const origin = stageRight.value ? TIP_LEFT : TIP_RIGHT;
			liveBeams.push({
				id: ready.id,
				fromX: origin.x,
				fromY: origin.y,
				x: ready.x,
				y: ready.y,
				headX: origin.x,
				headY: origin.y,
				opacity: 1,
				born: now,
				hit: false,
				ink: ready.ink,
			});
		}
		const target = live.find((glyph) => !glyph.loose);
		if (moodName === "slack") setEye({ x: 0, y: 2.6 });
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

	function enqueue(key: PendingKey) {
		if (!follow.value || document.hidden || moodName === "still") return;
		const now = performance.now();
		lastInput = now;
		engaged.value = true;
		armHome();
		queue.push(key);
		if (queue.length > MAX_QUEUE) queue.splice(0, queue.length - MAX_QUEUE);
		ensure(now);
	}

	function clearScene() {
		queue = [];
		live = [];
		liveBeams = [];
		liveBursts = [];
		resetMood();
		if (raf) cancelAnimationFrame(raf);
		raf = 0;
		window.clearTimeout(homeTimer);
		publish();
		engaged.value = false;
		setEye({ x: 0, y: 0 });
	}

	watch(follow, (value) => {
		if (!value) clearScene();
	});
	watch(stageRight, () => {
		queue = [];
		live = [];
		liveBeams = [];
		liveBursts = [];
		resetMood();
		if (raf) cancelAnimationFrame(raf);
		raf = 0;
		publish();
		setEye({ x: 0, y: 0 });
	});

	onMounted(() => {
		void listen<KeyEvent>("pet-key", ({ payload }) => {
			if (!payload || typeof payload.id !== "number") return;
			const label =
				typeof payload.label === "string" && payload.label
					? payload.label
					: null;
			enqueue({ id: payload.id, label });
		})
			.then((unlisten) => {
				unlistenKey = unlisten;
			})
			.catch(() => {});
	});

	onBeforeUnmount(() => {
		unlistenKey?.();
		if (raf) cancelAnimationFrame(raf);
		window.clearTimeout(homeTimer);
	});

	return { glyphs, beams, bursts, eye, engaged, shooting, tip, mood, wave };
}
