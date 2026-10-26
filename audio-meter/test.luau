tests = {}

function tests.the_bars_rise_at_once_and_fall_slowly()
	local function at(x: number, bpm: number)
		return handle({ event = "audio", now = 0, settings = {}, audio = { level = x, spectrum = { x, x }, kind = "music", bpm = bpm } }).widgets.meter
	end
	local w = at(1, 120)
	eq(w.panel.body[1].values[1], 1, "rises")
	eq(w.panel.sub, "Music · 120 bpm", "tempo")
	w = at(0, 0)
	eq(w.panel.body[1].values[1], 0.82, "falls")
	eq(w.peek.text, "0%", "level")
end
