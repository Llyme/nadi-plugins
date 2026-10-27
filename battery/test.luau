-- The tests of the Battery plugin: `nadi-plugin test battery`. Each test runs against a plugin of its own.

local T0 = 1800000000000

local function battery(percent: number, more: { [string]: any }?)
	local b = { present = true, percent = percent, plugged = false, charging = false, saver = false, minutes_left = 200, health = 91, full_wh = 48, design_wh = 52.7 }
	for k, v in more or {} do
		b[k] = v
	end
	return b
end

local function at(now: number, b, settings: { [string]: any }?)
	return handle({ event = "battery", now = now, battery = b, settings = settings or {} })
end

local function said(r)
	return (r and r.say) or {}
end

tests = {}

function tests.the_widget_is_the_vessel_and_says_how_it_stands()
	local r = at(T0, battery(64))
	local w = r.widgets.battery
	eq(w.peek.text, "64%", "peek text")
	eq(w.peek.label, "64% · 3h 20m left", "peek label")
	eq(w.peek.vessel, { level = 0.64, charging = false }, "vessel")
	eq(w.panel.title, "On battery · 3h 20m left", "how it stands")
	eq(w.panel.body[1].kind, "chips", "chips first")
	eq(w.panel.body[2].kind, "stats", "no graph yet")
	-- later, with the last hours, it has a graph
	r = at(T0 + 600000, battery(60))
	eq(r.widgets.battery.panel.body[2].kind, "graph", "graph")
end

function tests.amber_when_low_red_when_critical_green_when_charging()
	eq(at(T0, battery(64)).widgets.battery.peek.color, "#5ac88c", "green")
	eq(at(T0, battery(20)).widgets.battery.peek.color, "#e8b04a", "amber")
	eq(at(T0, battery(10)).widgets.battery.peek.color, "#e87a7a", "red")
	local r = at(T0, battery(5, { plugged = true, charging = true, minutes_to_full = 42, minutes_left = nadi.null }))
	eq(r.widgets.battery.peek.color, "#5ac88c", "charging is green")
	eq(r.widgets.battery.panel.title, "Charging · 42 min to full", "charging")
end

function tests.it_says_when_it_runs_low_and_when_it_runs_out()
	at(T0, battery(25))
	local r = at(T0 + 60000, battery(20))
	local s = said(r)[1]
	eq(s.text, "Battery low", "low")
	eq(s.priority, 60, "low priority")
	eq(s.pill.tone, "low", "low tone")
	eq(s.pill.sub, "20% · about 3h 20m left", "low sub")
	eq(s.pill.key, "battery", "key")
	eq(#said(at(T0 + 120000, battery(19))), 0, "told once")
	s = said(at(T0 + 180000, battery(10)))[1]
	eq(s.text, "Battery critical", "critical")
	eq(s.priority, 90, "critical rings through do not disturb")
	eq(s.pill.sub, "10% · about 3h 20m left · plug in", "critical sub")
	eq(said(at(T0 + 240000, battery(5)))[1].text, "Battery critical", "again at 5")
end

function tests.the_marks_are_the_settings_and_each_banner_can_be_off()
	at(T0, battery(40))
	eq(said(at(T0 + 60000, battery(29), { low = 30 }))[1].text, "Battery low", "low mark 30")
end

function tests.a_banner_that_is_off_is_not_said()
	at(T0, battery(25))
	eq(#said(at(T0 + 60000, battery(20), { b_low = false })), 0, "b_low off")
end

function tests.all_banners_off()
	at(T0, battery(25))
	eq(#said(at(T0 + 60000, battery(20), { alerts = false })), 0, "alerts off")
end

function tests.the_charger_going_in_says_charging()
	at(T0, battery(42))
	local s = said(at(T0 + 10000, battery(42, { plugged = true, charging = true, minutes_to_full = 42, rate_w = 18 })))[1]
	eq(s.text, "Charging", "charging")
	eq(s.pill.sub, "42% · about 42 min to full · 18 W", "charging sub")
	eq(s.pill.tile.bolt, true, "bolt")
	-- unplugged: off until asked for
	eq(#said(at(T0 + 20000, battery(42))), 0, "unplugged is off")
end

function tests.a_full_battery_says_so()
	at(T0, battery(98, { plugged = true, charging = true }))
	local s = said(at(T0 + 60000, battery(100, { plugged = true })))[1]
	eq(s.text, "Fully charged", "full")
	eq(s.pill.sub, "100%", "full sub")
end

function tests.saver_and_wear_are_off_until_asked_for()
	at(T0, battery(50))
	eq(#said(at(T0 + 10000, battery(50, { saver = true }))), 0, "saver off")
end

function tests.saver_and_wear_when_asked_for()
	local s = { b_saver = true, b_health = true }
	at(T0, battery(50), s)
	eq(said(at(T0 + 10000, battery(50, { saver = true }), s))[1].text, "Battery saver on", "saver")
	local worn = battery(50, { saver = true, health = 62, full_wh = 30, design_wh = 48 })
	eq(said(at(T0 + 20000, worn, s))[1].pill.sub, "Holds 62% of what it did new · 30 Wh of 48", "worn")
	eq(#said(at(T0 + 30000, worn, s)), 0, "worn once")
end

function tests.a_pc_with_no_battery_says_so_once()
	local r = at(T0, { present = false })
	eq(r.note, "No battery found: this PC runs on the wall.", "note")
	eq(r.widgets.battery, {}, "no widget")
	check(at(T0 + 1000, { present = false }).note == nil, "said once")
end
