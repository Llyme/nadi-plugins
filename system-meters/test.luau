-- The tests of System Meters: `nadi-plugin test system-meters`.

local function reading(cpu: number, ram: number, gpu: number?, now: number?, settings: any?)
	return { event = "system", now = now or 0, settings = settings or {}, system = { cpu = cpu, ram = ram, ram_used_gb = 9.8, ram_total_gb = 16, gpu = gpu } }
end

tests = {}

function tests.a_reading_draws_graphs_and_the_tip()
	local w = handle(reading(34, 61, 22)).widgets.meters
	eq(#w.peek.graphs.items, 3, "three graphs, the GPU's while it works")
	eq(w.peek.graphs.items[1].color, "#5ac8e6", "cpu colour")
	eq(w.peek.graphs.items[2].text, "61%", "ram now")
	eq(w.tip, "CPU 34%\nRAM 61%\nGPU 22%", "tip")
	eq(w.panel.body[2].kind, "chart", "opened: a chart each")
	eq(w.panel.body[3].sub, "9.8 of 16.0 GB", "memory")
end

function tests.busy_changes_colour_and_an_idle_gpu_is_left_out()
	local w = handle(reading(97, 85, 2)).widgets.meters
	eq(#w.peek.graphs.items, 2, "two graphs")
	eq(w.peek.graphs.items[1].color, "#e5675c", "hot")
	eq(w.peek.graphs.items[2].color, "#e6a15a", "warm")
end

function tests.calm_for_a_while_it_rests_and_heavy_it_comes_back()
	handle(reading(10, 40, nil, 0))
	local a = handle(reading(10, 40, nil, 4000)).widgets.meters
	eq(a.idle, false, "not yet")
	local b = handle(reading(10, 40, nil, 12000)).widgets.meters
	eq(b.idle, true, "calm a while: resting")
	local c = handle(reading(80, 40, nil, 14000)).widgets.meters
	eq(c.idle, false, "heavy: back at once")
	local d = handle(reading(10, 40, nil, 30000, { hide_calm = false })).widgets.meters
	eq(d.idle, false, "switched off: never resting")
end

function tests.the_busiest_programs_and_the_range()
	local ev = reading(30, 50, nil, 0)
	ev.system.top = { { name = "NTE", exe = "C:\\g\\nte.exe", cpu = 14, ram_gb = 3 } }
	local w = handle(ev).widgets.meters
	local last = w.panel.body[#w.panel.body]
	eq(last.kind, "usage", "the busiest")
	eq(last.rows[1].image, "exe:C:\\g\\nte.exe", "its picture")
	local r = handle({ event = "action", now = 1, settings = {}, action = "range:10m" })
	eq(r.widgets.meters.panel.body[1].items[2].items[2].on, true, "ten minutes chosen")
end

function tests.another_event_is_left_alone()
	check(handle({ event = "tick", now = 0, settings = {} }) == nil, "nothing")
end
