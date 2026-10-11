// Sound light: an aura that lives in the island itself instead of bars, and spills a little outside it. 16 soft gradient
// lights (one per frequency band, about 60 Hz to 12 kHz) drift through the island; the call of what is playing (talk, chill,
// groove, bang, calm) picks how they dance. Bass-only hits make the lights slam, a voice gets a warm halo that pulses with the
// syllables. Painted behind everything the island shows ("inside"), and a copy of it over the window's margin, stretched past
// the island's edges and masked so it reads as light spilling out ("outside").
//
// The island sends this face the sound (`audio`, already understood) and the island's shape for every frame; it draws.

(function () {
	var VIZ_BANDS = 16;
	var VIZ_SCALE = 0.7; // render below device resolution: it is all soft glow anyway
	var VIZ_FADE_MS = 700;
	var outside = nadi.role === "outside";

	// the aura (inside: what the frame shows; outside: kept off the page and copied into the margin)
	var vizCanvas = document.createElement("canvas");
	var vizCtx = vizCanvas.getContext("2d");
	var bleedCanvas = null;
	var bleedCtx = null;
	if (outside) {
		bleedCanvas = document.createElement("canvas");
		bleedCtx = bleedCanvas.getContext("2d");
		document.body.appendChild(bleedCanvas);
		bleedCanvas.style.cssText = "position:absolute;left:0;top:0;width:100%;height:100%";
	} else {
		document.body.appendChild(vizCanvas);
		vizCanvas.style.cssText = "position:absolute;left:0;top:0;width:100%;height:100%";
	}
	var viz = { hue: 150, on: false, fadeFrom: null, lightCur: [] };

	// what the island says now
	var audio = null;
	var shown = { view: "idle", w: 0, h: 0, x: 0, y: 0, r: 0, pad: 0 };

	function brightness() {
		var b = nadi.settings.brightness;
		return Math.max(0, Math.min(100, b == null ? 100 : b)) / 100;
	}
	function bleedLevel() {
		var a = nadi.settings.ambient;
		return Math.max(0, Math.min(100, a == null ? 60 : a)) / 100;
	}

	function sizeViz(win) {
		var w = Math.max(1, Math.round(shown.w * VIZ_SCALE));
		var h = Math.max(1, Math.round(shown.h * VIZ_SCALE));
		if (vizCanvas.width !== w) vizCanvas.width = w;
		if (vizCanvas.height !== h) vizCanvas.height = h;
		if (bleedCanvas) {
			var bw = Math.max(1, Math.round(win.w * VIZ_SCALE));
			var bh = Math.max(1, Math.round(win.h * VIZ_SCALE));
			if (bleedCanvas.width !== bw) bleedCanvas.width = bw;
			if (bleedCanvas.height !== bh) bleedCanvas.height = bh;
		}
	}

	// The light outside the island is the aura drawn once over the island's shape and kept where a soft mask lets it
	// through: bright at the island's edge, gone a few pixels away. The mask is only rebuilt when the island's shape changes.
	var maskCanvas = document.createElement("canvas");
	var maskCtx = maskCanvas.getContext("2d");
	var maskKey = "";
	function paintBleed() {
		var S = VIZ_SCALE;
		var level = bleedLevel();
		var reach = Math.min(shown.pad * 0.5, 24) * level;
		var grow = reach * 0.3;
		var x = (shown.x - grow) * S;
		var y = (shown.y - grow) * S;
		var w = (shown.w + 2 * grow) * S;
		var h = (shown.h + 2 * grow) * S;
		var radius = Math.min((shown.r + grow) * S, w / 2, h / 2);
		var blur = Math.max(0.5, reach * 0.5 * S);
		var key = [x, y, w, h, radius, blur, bleedCanvas.width, bleedCanvas.height].map(function (v) { return Math.round(v * 10); }).join();
		if (key !== maskKey) {
			maskKey = key;
			maskCanvas.width = bleedCanvas.width;
			maskCanvas.height = bleedCanvas.height;
			maskCtx.filter = "blur(" + blur + "px)";
			maskCtx.fillStyle = "#fff";
			maskCtx.beginPath();
			maskCtx.roundRect(x, y, w, h, radius);
			maskCtx.fill();
			maskCtx.filter = "none";
		}
		bleedCtx.setTransform(1, 0, 0, 1, 0, 0);
		bleedCtx.globalCompositeOperation = "source-over";
		bleedCtx.clearRect(0, 0, bleedCanvas.width, bleedCanvas.height);
		// the light has to exist as far out as the mask lets it reach: the aura is stretched over the island grown by the full reach
		var ext = Math.max(grow, Math.min(shown.pad - 2, reach * 2.2));
		bleedCtx.drawImage(vizCanvas, (shown.x - ext) * S, (shown.y - ext) * S, (shown.w + 2 * ext) * S, (shown.h + 2 * ext) * S);
		bleedCtx.globalCompositeOperation = "destination-in";
		bleedCtx.drawImage(maskCanvas, 0, 0);
		bleedCtx.globalCompositeOperation = "source-over";
	}

	function clearBleed() {
		if (bleedCtx) bleedCtx.clearRect(0, 0, bleedCanvas.width, bleedCanvas.height);
	}

	// where the eyes sit inside the island, in CSS px (the voice's halo is drawn around them)
	function eyeCenter() {
		return [shown.w / 2, shown.view === "hub" ? 38 : shown.h / 2];
	}

	// -- genre choreography: where each of the 16 lights sits, per vibe --
	// talk = huddled murmur, chill = slow drift, groove/pop = orbit, bang/rock = slam, calm/silent = barely there.
	// deterministic per-light 0..1 variation, so no two lights ever share a path
	function lightVar(i, salt) {
		var x = Math.sin(i * 127.1 + salt * 311.7) * 43758.5453;
		return x - Math.floor(x);
	}

	function lightPose(i, n, t, vibe, w, h, kickP, env) {
		var frac = i / n;
		var TAU = Math.PI * 2;
		var v1 = lightVar(i, 1);
		var v2 = lightVar(i, 2);
		var v3 = lightVar(i, 3);
		var v4 = lightVar(i, 4);
		var ax, fx, ay, fy;
		switch (vibe) {
			case "talk":
				ax = 0.34 * (0.55 + 0.45 * v1);
				fx = 0.9 * (0.7 + 0.6 * v2);
				ay = 0.28 * (0.55 + 0.45 * v4);
				fy = 1.1 * (0.7 + 0.6 * v1);
				return [w * (0.5 + ax * Math.sin(t * fx + i * 0.55 + v3 * TAU) + audio.pan * 0.1), h * (0.5 + ay * Math.sin(t * fy + i * 0.9 + v2 * TAU))];
			case "chill":
				ax = 0.42 * (0.55 + 0.45 * v1);
				fx = 0.25 * (0.6 + 0.9 * v2);
				ay = 0.36 * (0.55 + 0.45 * v3);
				fy = 0.3 * (0.6 + 0.9 * v4);
				return [w * (0.5 + ax * Math.sin(t * fx + i * 2.4 + v3 * TAU)), h * (0.5 + ay * Math.sin(t * fy + i * 1.8 + 1 + v4 * TAU))];
			case "bang": {
				var spd = 2.2 * (0.7 + 0.6 * v1);
				var ang = t * spd + frac * TAU + v2 * TAU;
				var slam = kickP * 0.16 * (0.6 + 0.8 * v4);
				var rx = 0.44 * (0.6 + 0.4 * v3);
				var ry = 0.38 * (0.6 + 0.4 * v4);
				return [
					w * (0.5 + rx * Math.cos(ang + Math.sin(t * (5 + 3 * v1) + i + v2 * 6) * 0.35) + audio.pan * 0.14),
					h * (0.5 + ry * Math.sin(ang * (1.1 + 0.5 * v2) + i + v3 * 6) - slam),
				];
			}
			case "calm":
				ax = 0.26 * (0.55 + 0.45 * v1);
				fx = 0.3 * (0.6 + 0.9 * v2);
				ay = 0.2 * (0.55 + 0.45 * v3);
				fy = 0.35 * (0.6 + 0.9 * v4);
				return [w * (0.5 + ax * Math.sin(t * fx + i * 1.1 + v3 * TAU)), h * (0.5 + ay * Math.sin(t * fy + i * 1.7 + v4 * TAU))];
			default: {
				// groove: pop song and friends, a loose orbit with no formation
				var sp = (0.5 + Math.PI * (audio.bpm / 60) * 0.04) * (0.7 + 0.6 * v1);
				var an = t * sp + frac * TAU + v2 * TAU + Math.sin(audio.sway + v3 * 6) * 0.35;
				var bounce = (env || 0) * 0.1 + kickP * 0.05;
				var gx = 0.38 * (0.6 + 0.4 * v3);
				var gy = (0.3 + bounce) * (0.6 + 0.4 * v4);
				return [
					w * (0.5 + gx * Math.cos(an) + 0.04 * Math.sin(t * (1.3 + v1) + i * 2.1) + audio.pan * 0.12),
					h * (0.5 + gy * Math.sin(an) + 0.04 * Math.sin(t * (1.7 + v2) + i * 1.3)),
				];
			}
		}
	}

	function lightHue(i, n, t, vibe, baseHue) {
		var frac = i / n;
		switch (vibe) {
			case "talk":
				return 28 + frac * 40 + Math.sin(t * 0.7 + i) * 5; // warm voices, still distinct
			case "chill":
				return 195 + Math.sin(t * 0.2 + i * 0.5) * 25 + frac * 30; // teal-blue wash
			case "bang":
				return (8 + frac * 45 + t * 40 + Math.sin(t * 5 + i) * 12 + 360) % 360; // hot, fast cycle
			case "calm":
				return (baseHue + frac * 40) % 360;
			default:
				return (baseHue + frac * 300) % 360; // pop rainbow: full circle across the 16 lights
		}
	}

	// paints the aura; false when there is nothing to show (the sound stopped and the light has faded out)
	function paintViz(now, dt) {
		var c = vizCtx;
		var w = vizCanvas.width;
		var h = vizCanvas.height;
		var S = VIZ_SCALE;
		var bright = brightness();
		var level = bleedLevel();
		var gain0;
		// stay live while any lingering band is still visible, so the slow fall plays out instead of being cut by the fade-out
		var peakBand = 0;
		for (var b = 0; b < VIZ_BANDS; b++) if (audio.bands[b] > peakBand) peakBand = audio.bands[b];
		var live = audio.level > 0.03 || peakBand > 0.04;
		if (!live) {
			if (viz.on) {
				// the sound stopped: the light fades out over a moment instead of vanishing
				if (viz.fadeFrom === null) viz.fadeFrom = now;
				var f = 1 - (now - viz.fadeFrom) / VIZ_FADE_MS;
				if (f > 0) {
					vizCanvas.style.opacity = String(f);
					if (bleedCanvas) bleedCanvas.style.opacity = String(Math.min(1, level * 1.1) * f);
					return true;
				}
				c.clearRect(0, 0, w, h);
				viz.on = false;
				viz.fadeFrom = null;
				vizCanvas.style.opacity = "";
				if (bleedCanvas) bleedCanvas.style.opacity = "";
				clearBleed();
			}
			return false;
		}
		viz.fadeFrom = null;
		vizCanvas.style.opacity = "";
		viz.on = true;
		c.setTransform(1, 0, 0, 1, 0, 0);
		c.clearRect(0, 0, w, h);
		// source-over, not additive: stacked lights converge toward the cap instead of summing past it
		c.globalCompositeOperation = "source-over";

		viz.hue = (viz.hue + dt * 7 * (1 - 0.75 * audio.chill)) % 360;
		var vibe = audio.state; // silent | talk | chill | groove | bang | calm: picks the dance
		var t = (now / 1000) * (vibe === "chill" ? 0.45 : 1);
		var baseHue = viz.hue;
		// the hub is mostly text: keep the aura subtle there
		var gain = shown.view === "hub" ? 0.5 : shown.view === "idle" ? 1 : 0.7;
		var musicish = 0.35 + 0.65 * (1 - audio.voiceSm);
		var size = Math.max(w, h);
		// a narrow island packs the same lights into less room, and they add up to a glare
		var crowd = Math.min(1, Math.max(0.5, w / (260 * S)));
		var kickP = Math.max(0, Math.min(audio.kick.p, 1.2));

		// the expanded island is large, so lights roam it fully; the small views make their lights shrink to match
		var expanded = shown.view === "hub";
		var compact = shown.view === "idle" || shown.view === "notification" || shown.view === "brief" || shown.view === "usage_peek";
		var rMul = expanded ? 1 : compact ? 0.65 : 0.8;

		// 16 lights expanded, 8 elsewhere: the small views condense adjacent band pairs. Genre switches glide: each light rides
		// a critically damped spring toward its choreography target.
		var activeN = expanded ? VIZ_BANDS : 8;
		var STIFF = 20;
		var DAMP = 9;
		for (var i = 0; i < activeN; i++) {
			var env = expanded ? audio.bands[i] || 0 : ((audio.bands[2 * i] || 0) + (audio.bands[2 * i + 1] || 0)) / 2;
			var pose = lightPose(i, activeN, t, vibe, w, h, kickP, env);
			// never let a center leave the island (an off-canvas light reads as a light that disappeared)
			var tnx = Math.min(0.98, Math.max(0.02, pose[0] / Math.max(1, w)));
			var tny = Math.min(0.98, Math.max(0.02, pose[1] / Math.max(1, h)));
			var cur = viz.lightCur[i];
			if (!cur) {
				cur = viz.lightCur[i] = { x: tnx, y: tny, vx: 0, vy: 0 };
			} else {
				cur.vx += ((tnx - cur.x) * STIFF - cur.vx * DAMP) * dt;
				cur.vy += ((tny - cur.y) * STIFF - cur.vy * DAMP) * dt;
				cur.x = Math.min(1.05, Math.max(-0.05, cur.x + cur.vx * dt));
				cur.y = Math.min(1.05, Math.max(-0.05, cur.y + cur.vy * dt));
			}
			var px = cur.x * w;
			var py = cur.y * h;
			var hue = lightHue(i, activeN, now / 1000, vibe, baseHue);
			// talk stays warm; bang runs hot; chill stays soft
			var sizeMul = vibe === "talk" ? 0.16 : vibe === "chill" ? 0.3 : vibe === "bang" ? 0.2 : vibe === "calm" ? 0.16 : 0.17;
			var r = size * sizeMul * (0.35 + 2.1 * env) * crowd * rMul;
			// (brightness is band-driven only: the genre picks the dance and the hues, never the intensity)
			var a = (0.03 + 0.32 * env) * audio.level * gain * musicish * 1.4 * crowd * crowd * bright;
			if (a < 0.004) continue;
			var g = c.createRadialGradient(px, py, 0, px, py, Math.max(1, r));
			g.addColorStop(0, "hsla(" + hue + ", 90%, 62%, " + Math.min(0.5, a) + ")");
			g.addColorStop(1, "hsla(" + hue + ", 90%, 62%, 0)");
			c.fillStyle = g;
			// (only the square the light reaches: past its radius it is clear, and a whole canvas for each light is the cost)
			var rr = Math.max(1, r);
			c.fillRect(px - rr, py - rr, rr * 2, rr * 2);
		}

		// voice: a warm halo breathing with the syllables (rings are bass-only, and the eyes' own)
		if (audio.talk > 0.03) {
			var ec = eyeCenter();
			var ex = ec[0] * S;
			var ey = ec[1] * S;
			var te = audio.talkEnv;
			var tr = size * (0.16 + 0.3 * te) * rMul;
			var tg = c.createRadialGradient(ex, ey, 0, ex, ey, tr);
			tg.addColorStop(0, "hsla(38, 95%, 70%, " + Math.min(0.5, 0.3 * audio.talk * gain * (0.4 + te) * bright) + ")");
			tg.addColorStop(1, "hsla(38, 95%, 70%, 0)");
			c.fillStyle = tg;
			c.fillRect(ex - tr, ey - tr, tr * 2, tr * 2);
		}
		c.globalCompositeOperation = "source-over";
		return true;
	}

	nadi.on("frame", function (m) {
		audio = m.audio;
		shown = { view: m.view, w: m.island.w, h: m.island.h, x: m.island.x, y: m.island.y, r: m.island.r, pad: m.island.pad };
		if (!audio || shown.w < 1) return;
		var bright = brightness();
		var level = bleedLevel();
		if (outside) {
			// (only while the light spills: with no margin, or a setting of 0, there is nothing to paint here)
			if (level <= 0 || shown.pad <= 0 || bright <= 0) {
				if (viz.on) {
					viz.on = false;
					clearBleed();
				}
				return;
			}
			sizeViz(m.win);
			if (paintViz(m.now, m.dt)) {
				bleedCanvas.style.opacity = vizCanvas.style.opacity ? String(Math.min(1, level * 1.1) * Number(vizCanvas.style.opacity)) : String(Math.min(1, level * 1.1));
				paintBleed();
			}
			return;
		}
		sizeViz(m.win);
		if (bright > 0) paintViz(m.now, m.dt);
		else if (viz.on) {
			vizCtx.clearRect(0, 0, vizCanvas.width, vizCanvas.height);
			viz.on = false;
		}
	});
})();
