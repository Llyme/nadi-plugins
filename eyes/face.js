// Eyes: the two eyes of the island. They blink, look at the cursor, squash and stretch as it moves, glance around when it
// rests, and show moods (happy, sleepy, wide awake, and, when they react to sound, the beat of the music). One frame runs this
// script, over the island: it draws the eyes of the collapsed island, the ripples of the bass in every view, and (in the hub)
// the eyes of the header, where the head's "head" widget is (that widget is a place only: the frame is a process, and one is enough).
//
// The island sends every frame: the time (T), the island's shape, the cursor, the sound; and tells of a beat of the bass,
// of the hub opening, of a click, of a boop. This script keeps the eyes' own state (blinks, moods, glances) and draws.

(function () {
	var canvas = document.createElement("canvas");
	canvas.style.cssText = "position:absolute;left:0;top:0;width:100%;height:100%";
	document.body.appendChild(canvas);
	var ctx = canvas.getContext("2d");
	var dpr = window.devicePixelRatio || 1;

	// what the eyes know of the sound: all of it while they react to it, none otherwise
	var SILENT_AUDIO = {
		target: 0, level: 0, mode: "silent", kind: "silent", voice: 0, voiceSm: 0, bang: 0, bands: new Array(16).fill(0),
		bpm: 100, conf: 0, energy: 0, chill: 0, pan: 0, dance: 0, talk: 0, talkEnv: 0, kick: { p: 0, v: 0 }, nod: { p: 0, v: 0 }, sway: 0, state: "silent", video: false,
	};
	var audio = SILENT_AUDIO;
	var director = { state: "silent", video: false };
	var T = 0; // the island's clock, ms
	var view = "idle";
	var shape = { w: 0, h: 0, x: 0, y: 0 };
	var self = { x: 0, y: 0, w: 0, h: 0 };
	var peers = [];
	var gazeOff = { x: 0, y: 0 };
	var started = false;

	function resize() {
		dpr = window.devicePixelRatio || 1;
		var w = Math.round(innerWidth * dpr);
		var h = Math.round(innerHeight * dpr);
		if (canvas.width !== w) canvas.width = w;
		if (canvas.height !== h) canvas.height = h;
	}
	nadi.on("resize", resize);
	resize();

	function rand(min, max) {
		return min + Math.random() * (max - min);
	}

	var eyeState = {
		blinkState: "open", // "open" | "closing" | "opening"
		blinkStart: 0,
		nextBlinkAt: 0,
		openness: 1.0,
		squish: 0.0,
		squishDir: [0, 0],
		smoothedCursor: { x: 0, y: 0 },
		doubleBlink: false,
		winkSide: 0, // -1 left eye only, +1 right eye only, 0 both
	};

	var mood = {
		happy: { p: 0, v: 0 },
		sleepy: { p: 0, v: 0 },
		wide: { p: 0, v: 0 },
		xd: { p: 0, v: 0 }, // > <  (excited, punchy music)
		bliss: { p: 0, v: 0 }, // u u  (chill)
		hover: false, // the cursor is over the island
		happyUntil: 0,
		wideUntil: 0,
		lastMoveAt: 0,
		// idle glances: after the cursor sits still a while the eyes look around
		glance: { x: 0, y: 0 },
		glanceTarget: { x: 0, y: 0 },
		glanceW: 0,
		nextGlanceAt: 0,
		lastFrame: 0,
	};

	function springTo(s, target, dt) {
		// underdamped on purpose: an expression change pops past its target and settles
		var a = 340 * (target - s.p) - 19 * s.v;
		s.v += a * dt;
		s.p += s.v * dt;
	}

	// Which expression the music calls for. Picked when the director's vibe changes and then re-rolled only every 5-9 s, so it
	// reads as a deliberate mood, never a flicker.
	var emote = { name: "none", vibe: "", until: 0 };
	function pickEmote(now) {
		if (director.state === emote.vibe && now < emote.until) return;
		emote.vibe = director.state;
		var options = { bang: ["xd", "arch", "xd", "arch", "open"], chill: ["bliss", "sleepy", "bliss", "open"], groove: ["open", "open", "arch"] }[emote.vibe] || ["none"];
		var next = options[Math.floor(Math.random() * options.length)];
		if (next === emote.name && options.length > 1) next = options[(options.indexOf(next) + 1) % options.length];
		emote.name = next;
		emote.until = now + rand(5000, 9000);
	}

	function updateMood(now) {
		var dt = Math.min(Math.max((now - mood.lastFrame) / 1000, 0), 0.05);
		mood.lastFrame = now;
		var idleFor = now - mood.lastMoveAt;
		var inIdle = view === "idle";

		var wide = now < mood.wideUntil ? 1 : 0;
		var happy = !wide && ((mood.hover && inIdle) || now < mood.happyUntil) ? 1 : 0;
		var sleepy = !wide && !happy && idleFor > 25000 && audio.mode === "silent" ? 1 : 0;
		springTo(mood.wide, wide, dt);
		// music-driven expressions (never over a hover / click reaction)
		pickEmote(now);
		var free = !happy && !wide;
		var e = free ? emote.name : "none";
		springTo(mood.happy, Math.max(happy, e === "arch" ? 1 : 0), dt);
		springTo(mood.xd, e === "xd" ? 1 : 0, dt);
		springTo(mood.bliss, e === "bliss" ? 1 : 0, dt);
		// relaxed, half-lidded eyes while the music is chill
		springTo(mood.sleepy, Math.max(sleepy, e === "sleepy" ? 0.75 : 0.3 * audio.chill * (e === "bliss" ? 0 : 1)), dt);

		// glances: cursor parked for 4s+ -> look around now and then
		var glancing = idleFor > 4000 && !happy && !wide;
		if (glancing && now >= mood.nextGlanceAt) {
			var spots = [[-1, 0.1], [1, 0.1], [0.7, -0.8], [-0.7, -0.8], [0, 0], [-0.9, 0.5], [0.9, 0.5]];
			var spot = spots[Math.floor(Math.random() * spots.length)];
			mood.glanceTarget = { x: spot[0], y: spot[1] };
			mood.nextGlanceAt = now + rand(1400, 3400);
		}
		var k = 1 - Math.exp(-dt * 14);
		mood.glance.x += (mood.glanceTarget.x - mood.glance.x) * k;
		mood.glance.y += (mood.glanceTarget.y - mood.glance.y) * k;
		mood.glanceW += ((glancing ? 1 : 0) - mood.glanceW) * (1 - Math.exp(-dt * 8));
	}

	function tickBlink(now) {
		// no blinking while the face is doing an expression: it only blinks in its normal open-eyed mode. The next blink is
		// pushed out so it does not fire the instant the expression ends.
		var expressive = Math.max(mood.happy.p, mood.xd.p, mood.bliss.p, mood.sleepy.p) > 0.35;
		if (expressive && eyeState.blinkState === "open") {
			eyeState.nextBlinkAt = Math.max(eyeState.nextBlinkAt, now + 1200);
			return;
		}
		var slow = 1 + Math.max(0, Math.min(mood.sleepy.p, 1)) * 1.8; // sleepy blinks are lazy
		if (eyeState.blinkState === "open" && now >= eyeState.nextBlinkAt) {
			eyeState.blinkState = "closing";
			eyeState.blinkStart = now;
			// now and then: a wink instead of a blink
			if (!eyeState.doubleBlink && mood.happy.p < 0.3 && mood.sleepy.p < 0.3 && Math.random() < 0.07) {
				eyeState.winkSide = Math.random() < 0.5 ? -1 : 1;
			} else if (!eyeState.doubleBlink) {
				eyeState.winkSide = 0;
			}
		} else if (eyeState.blinkState === "closing") {
			var t = (now - eyeState.blinkStart) / (80 * slow);
			if (t >= 1) {
				eyeState.blinkState = "opening";
				eyeState.blinkStart = now;
				eyeState.openness = 0;
			} else {
				eyeState.openness = 1 - t;
			}
		} else if (eyeState.blinkState === "opening") {
			var hold = eyeState.winkSide !== 0 ? 260 : 0; // a wink lingers
			var u = (now - eyeState.blinkStart - hold) / (120 * slow);
			if (u < 0) {
				eyeState.openness = 0;
			} else if (u >= 1) {
				eyeState.blinkState = "open";
				eyeState.openness = 1;
				if (!eyeState.doubleBlink && eyeState.winkSide === 0 && Math.random() < 0.22) {
					eyeState.doubleBlink = true; // quick second blink
					eyeState.nextBlinkAt = now + 110;
				} else {
					eyeState.doubleBlink = false;
					eyeState.winkSide = 0;
					eyeState.nextBlinkAt = now + (mood.sleepy.p > 0.5 ? rand(1800, 3400) : rand(2500, 5500));
				}
			} else {
				eyeState.openness = u;
			}
		}
	}

	// gaze vector (each axis -1..1) from one eye toward the cursor, blended with the idle-glance target when the cursor has been
	// still for a while
	function gazeFor(eyeX, eyeY) {
		var cx = Math.max(-1, Math.min(1, (eyeState.smoothedCursor.x - (eyeX + gazeOff.x)) / 60));
		var cy = Math.max(-1, Math.min(1, (eyeState.smoothedCursor.y - (eyeY + gazeOff.y)) / 40));
		var w = mood.glanceW;
		return [cx + (mood.glance.x - cx) * w, cy + (mood.glance.y - cy) * w];
	}

	// Each eye is a closed shape built from two curves over x in [-1, 1]:
	//   centerY(x) = c0 + k*x^2         (k > 0 bends the middle up: an arch)
	//   top/bottom = centerY -/+ amp * sqrt(1 - x^2)
	// A circle is c0=k=0 with top=bot=r; every mood is just a different set of numbers, so morphing is plain interpolation.
	var EYE_SHAPES = {
		neutral: { rx: 5.1, top: 5.8, bot: 5.8, c0: 0, k: 0, pw: 2 },
		happy: { rx: 6.0, top: 2.0, bot: 2.0, c0: -2.6, k: 5.2, pw: 2 }, // ^  ^ smiling arches
		sleepy: { rx: 5.6, top: 0.6, bot: 5.0, c0: -2.0, k: 0, pw: 2 }, // heavy lids
		wide: { rx: 4.6, top: 8.0, bot: 8.0, c0: 0, k: 0, pw: 2 }, // tall, startled ovals
		xd: { rx: 6.8, top: 1.5, bot: 1.5, c0: -3.5, k: 7.0, pw: 1.1 }, // >  < squeezed-shut chevrons
		bliss: { rx: 6.0, top: 2.0, bot: 2.0, c0: 2.6, k: -5.2, pw: 2 }, // u  u blissfully closed
	};
	var EYE_KEYS = ["rx", "top", "bot", "c0", "k", "pw"];

	function eyeParams(happy, sleepy, wide, xd, bliss) {
		var out = {};
		for (var i = 0; i < EYE_KEYS.length; i++) {
			var key = EYE_KEYS[i];
			var n = EYE_SHAPES.neutral[key];
			out[key] = n + happy * (EYE_SHAPES.happy[key] - n) + sleepy * (EYE_SHAPES.sleepy[key] - n) + wide * (EYE_SHAPES.wide[key] - n) + xd * (EYE_SHAPES.xd[key] - n) + bliss * (EYE_SHAPES.bliss[key] - n);
		}
		out.top = Math.max(out.top, 0.4); // springs overshoot; never invert a curve
		out.bot = Math.max(out.bot, 0.4);
		return out;
	}

	var EYE_STEPS = 32;

	function eyePath(c, p, blink) {
		c.beginPath();
		var i, x, s;
		for (i = 0; i <= EYE_STEPS; i++) {
			x = -1 + (2 * i) / EYE_STEPS;
			s = Math.sqrt(Math.max(0, 1 - x * x));
			var y = p.c0 + p.k * Math.pow(Math.abs(x), p.pw) - p.top * blink * s;
			if (i === 0) c.moveTo(x * p.rx, y);
			else c.lineTo(x * p.rx, y);
		}
		for (i = EYE_STEPS; i >= 0; i--) {
			x = -1 + (2 * i) / EYE_STEPS;
			s = Math.sqrt(Math.max(0, 1 - x * x));
			c.lineTo(x * p.rx, p.c0 + p.k * Math.pow(Math.abs(x), p.pw) + p.bot * blink * s);
		}
		c.closePath();
	}

	// Draws the eyes centered at (centerX, cy). `sc` enlarges them.
	function paintEyesAt(c, centerX, cy, sc) {
		var wide = Math.max(0, Math.min(mood.wide.p, 1.3));
		var happy = Math.max(0, Math.min(mood.happy.p, 1.25));
		var sleepy = Math.max(0, Math.min(mood.sleepy.p, 1.1));
		var xd = Math.max(0, Math.min(mood.xd.p, 1.15));
		var bliss = Math.max(0, Math.min(mood.bliss.p, 1.15));
		var dirx = eyeState.squishDir[0];
		var diry = eyeState.squishDir[1];
		// motion squash & stretch stays axis-aligned (no rotation): rotating the eye made blinks close sideways
		var ax = Math.abs(dirx);
		var ay = Math.abs(diry);
		var sq = eyeState.squish;
		var stretchX = Math.max(1 + sq * (0.25 * ax - 0.12 * ay), 0.6);
		var stretchY = Math.max(1 + sq * (0.25 * ay - 0.12 * ax), 0.6);

		// audio-driven motion for the whole face
		var kickP = Math.max(0, Math.min(audio.kick.p, 1.2));
		var chill = audio.chill;
		var xdW = Math.min(xd, 1); // how much the eyes are currently the > < chevrons
		var bang = audio.bang;
		var nodP = Math.max(-0.4, Math.min(audio.nod.p, 1.4));
		// while head-banging the head only moves on the kick
		var groupBob = -(audio.bands[0] * 1.5 * (1 - 0.8 * chill) + kickP * 1.7) * audio.dance * sc * (1 - bang) + nodP * 6.5 * bang * sc;
		// the > < chevrons rock gently side to side; not tied to the beat
		var tt = T / 1000;
		var shakeX = (Math.sin(tt * 8.2) + 0.3 * Math.sin(tt * 4.6 + 1.3)) * (1.3 + 0.5 * audio.level) * xdW * sc;
		var swayX = Math.sin(audio.sway) * 2.6 * (1 - 0.35 * chill) * (1 - 0.8 * bang) * audio.dance * sc + shakeX;
		var tilt = Math.sin(audio.sway) * 0.11 * (1 - 0.6 * chill) * audio.dance * (1 - xdW) * (1 - bang); // head-nod lean
		var kickSquash = 1 + kickP * 0.16 * audio.dance * (1 - bang) - nodP * 0.16 * bang;
		var talkAmt = Math.max(-0.32, Math.min(audio.talkEnv - 0.32, 0.68)) * audio.talk * 1.8;
		var talkY = 1 + talkAmt * 0.8;
		var talkX = 1 - talkAmt * 0.24;

		c.save();
		c.translate(centerX + swayX, cy + groupBob);
		c.rotate(tilt);
		c.translate(-centerX, -cy);

		var sides = [-1, 1];
		for (var si = 0; si < 2; si++) {
			var side = sides[si];
			var ex = centerX + side * 13.5 * sc;
			var gz = gazeFor(ex, cy);
			var gx = gz[0] + audio.pan * 0.6 * Math.min(1, audio.level * 2); // glance toward wherever the sound is panned
			var gy = gz[1];

			// a wink is just this one eye morphing into the happy arch and back
			var winking = eyeState.winkSide === side;
			var eyeHappy = winking ? Math.max(happy, 1 - eyeState.openness) : happy;
			var p = eyeParams(eyeHappy, sleepy, wide, xd, bliss);
			var blink = winking ? 1 : Math.max(eyeState.openness, 0.06);

			c.save();
			// without pupils the whole eye does the looking
			c.translate(ex + gx * 3.2 * sc, cy + gy * 1.7 * sc);
			c.scale(sc * talkX, sc * talkY * kickSquash);
			// eyelids always close vertically, whatever else the eye is doing
			c.scale(1, blink);
			// the ^ ^ arch / sleepy lids stay undistorted: squash fades out as they form
			var calm = 1 - Math.min(Math.max(eyeHappy, sleepy, xd, bliss), 1);
			c.scale(1 + (stretchX - 1) * calm, 1 + (stretchY - 1) * calm);

			// the > < chevrons are the arch turned toward the middle (left eye >, right eye <)
			if (xd > 0.01) c.rotate(-side * (Math.PI / 2) * Math.min(xd, 1));
			eyePath(c, p, 1);
			var g = c.createRadialGradient(-p.rx * 0.3, -p.top * 0.35, 0, -p.rx * 0.3, -p.top * 0.35, p.rx * 2);
			g.addColorStop(0, "rgb(252, 252, 254)");
			g.addColorStop(1, "rgb(222, 226, 236)");
			c.fillStyle = g;
			c.fill();
			c.restore();
		}
		c.restore();
	}

	// ---- ripples: very thin circular waves expanding from the middle between the eyes, one for each bass hit. Sound panned to a
	// side shows as a ring that is solid on that side and translucent on the other. ----
	var rings = [];
	var lastRingT = 0;

	// the colour drifts slowly, as the sound light's does
	function ringHue() {
		return ((T / 1000) * 7) % 360 + 20;
	}

	nadi.on("event", function (m) {
		if (m.name === "beat") {
			// bass only: one ring per kick-drum / strong low-end onset, no merging
			if (!nadi.settings.rings && nadi.settings.rings != null) return;
			rings.push({ t: T, s: m.data.strength, sv: m.data.strength, hue: ringHue(), pan: m.data.pan || 0 });
			if (rings.length > 12) rings.shift();
		} else if (m.name === "hub-open") {
			mood.happyUntil = T + 1600; // happy to see you
		} else if (m.name === "blip") {
			mood.happyUntil = T + 900; // a little happy blip
		} else if (m.name === "boop") {
			mood.wideUntil = T + 320; // boop!
		}
	});

	function paintRings(now) {
		var dt = Math.min(Math.max((now - lastRingT) / 1000, 0), 0.05);
		lastRingT = now;
		if (!rings.length) return;
		var w = innerWidth;
		var h = innerHeight;
		var size = Math.max(w, h);
		// where the eyes sit inside the island: the middle, or (in the hub) where their widget is, whichever row and place
		var cx = w / 2;
		var cy = h / 2;
		if (view === "hub") {
			for (var i0 = 0; i0 < peers.length; i0++) {
				if (peers[i0].face === "head") {
					cx = peers[i0].x + peers[i0].w / 2;
					cy = peers[i0].y + peers[i0].h / 2;
				}
			}
		}
		ctx.lineWidth = 0.7; // hairline, always: only the fade changes
		ctx.lineCap = "round";
		for (var i = rings.length - 1; i >= 0; i--) {
			var rg = rings[i];
			var age = Math.max(0, (now - rg.t) / 900);
			if (age >= 1) {
				rings.splice(i, 1);
				continue;
			}
			var ease = 1 - Math.pow(1 - age, 2.2);
			// merged hits raise the target strength; the drawn strength eases toward it
			rg.sv += (rg.s - rg.sv) * Math.min(1, dt * 9);
			var side = Math.max(-1, Math.min(1, rg.pan || 0));
			var dirness = Math.max(0, Math.min((Math.abs(side) - 0.12) / 0.6, 1)); // 0 centered .. 1 hard-panned
			// how far it travels follows how strong the bass hit was
			var reach = 0.12 + 0.62 * rg.sv;
			var radius = Math.max(0, ease * size * reach);
			var alpha = Math.min(1, (0.6 + 0.4 * rg.sv) * Math.pow(1 - age, 0.7)) * (view === "hub" ? 0.85 : 1);
			if (dirness > 0.02 && radius > 1) {
				// sound from one side: the ring is solid on the side it comes from and turns translucent across to the other
				var far = alpha * (1 - 0.85 * dirness);
				var g = ctx.createLinearGradient(cx - side * radius, cy, cx + side * radius, cy);
				g.addColorStop(0, "hsla(" + rg.hue + ", 95%, 76%, " + far + ")");
				g.addColorStop(0.5, "hsla(" + rg.hue + ", 95%, 76%, " + (far + alpha) / 2 + ")");
				g.addColorStop(1, "hsla(" + rg.hue + ", 95%, 76%, " + alpha + ")");
				ctx.strokeStyle = g;
			} else {
				ctx.strokeStyle = "hsla(" + rg.hue + ", 95%, 76%, " + alpha + ")";
			}
			ctx.beginPath();
			ctx.arc(cx, cy, radius, 0, Math.PI * 2);
			ctx.stroke();
		}
	}

	// the cursor moved (the island's poll of it): how far it went, and how it moves the eyes: squash, stretch, gaze
	var lastSeq = -1;
	function cursorMoved(c) {
		if (c.speed > 2) {
			if (T - mood.lastMoveAt > 25000) mood.wideUntil = T + 500; // startled awake
			mood.lastMoveAt = T;
		}
		mood.hover = !!c.hover;
		var targetSquish = Math.min(c.speed / 140.0, 1.0);
		if (targetSquish > eyeState.squish) {
			eyeState.squish = targetSquish;
			if (c.speed > 0.5) eyeState.squishDir = [c.dx / c.speed, c.dy / c.speed];
		} else {
			eyeState.squish *= 0.78;
		}
		eyeState.smoothedCursor.x += (c.x - eyeState.smoothedCursor.x) * 0.35;
		eyeState.smoothedCursor.y += (c.y - eyeState.smoothedCursor.y) * 0.35;
	}

	nadi.on("frame", function (m) {
		T = m.now;
		view = m.view;
		shape = m.island;
		self = m.rect || self;
		peers = m.peers || [];
		if (!started) {
			started = true;
			eyeState.nextBlinkAt = T + rand(2500, 5500);
			mood.lastMoveAt = T;
			mood.lastFrame = T;
			lastRingT = T;
		}
		var react = nadi.settings.react;
		var reacts = react == null ? true : !!react;
		audio = reacts && m.audio ? m.audio : SILENT_AUDIO;
		director = reacts && m.audio ? { state: m.audio.state, video: !!m.audio.video } : { state: "silent", video: false };
		if (m.cursor && m.cursor.seq !== lastSeq) {
			lastSeq = m.cursor.seq;
			cursorMoved(m.cursor);
		}

		var w = innerWidth;
		var h = innerHeight;
		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		ctx.clearRect(0, 0, w, h);
		paintRings(T);
		if (view === "idle") {
			updateMood(T);
			tickBlink(T);
			gazeOff = { x: 0, y: 0 };
			// breathing: the eyes drift up and down a hair, on a slow cycle
			paintEyesAt(ctx, w / 2, h / 2 + Math.sin((T / 3600) * Math.PI * 2) * 0.55, 1);
		} else if (view === "hub") {
			// the eyes of the header: where their widget is, whichever row and place it was put in
			var head = null;
			for (var i = 0; i < peers.length; i++) if (peers[i].face === "head") head = peers[i];
			if (head) {
				updateMood(T);
				tickBlink(T);
				// the cursor is told in window coordinates: where this frame is in the window
				gazeOff = { x: self.x, y: self.y };
				paintEyesAt(ctx, head.x + head.w / 2, head.y + head.h / 2, (1.3 * head.h) / 76);
			}
		}
	});
})();
