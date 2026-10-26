local function at(now: number, exes: { string })
	local windows = {}
	for _, e in exes do
		table.insert(windows, { exe = e })
	end
	return handle({ event = "processes", now = now, settings = { games = "hl2.exe, x.exe", remind_min = 60 }, windows = windows })
end

tests = {}

function tests.a_game_is_timed_and_said_once_an_hour()
	eq(at(0, { "code.exe", "hl2.exe" }).widgets.clock.peek.text, "0m", "starts")
	local r = at(3600000, { "hl2.exe" })
	eq(r.widgets.clock.panel.title, "Hl2", "name")
	eq(r.say[1].text, "You have been playing Hl2 for 1h 0m", "said")
	check(at(3700000, { "hl2.exe" }).say == nil, "once")
	eq(at(3800000, {}).widgets.clock, {}, "gone")
end
