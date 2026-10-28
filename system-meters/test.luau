-- The tests of System Meters: `nadi-plugin test system-meters`.

local function reading(cpu: number, ram: number, gpu: number?)
	return { event = "system", now = 0, settings = {}, system = { cpu = cpu, ram = ram, ram_used_gb = 9.8, ram_total_gb = 16, gpu = gpu } }
end

tests = {}

function tests.a_reading_draws_bars_rings_and_the_tip()
	local w = handle(reading(34, 61, 22)).widgets.meters
	eq(#w.peek.bars, 3, "three bars")
	eq(w.peek.bars[1].color, "#5ac8e6", "cpu colour")
	eq(w.peek.bars[2].part, 0.61, "ram part")
	eq(w.tip, "CPU 34%\nRAM 61%\nGPU 22%", "tip")
	eq(#w.panel.body[1].items, 3, "three rings")
	eq(w.panel.title, nil, "no head")
	eq(w.panel.body[2].rows[1][2], "9.8 of 16.0 GB", "memory")
end

function tests.busy_changes_colour_and_no_gpu_means_two()
	local w = handle(reading(97, 85, nil)).widgets.meters
	eq(#w.peek.bars, 2, "two bars")
	eq(w.peek.bars[1].color, "#e5675c", "hot")
	eq(w.peek.bars[2].color, "#e6a15a", "warm")
end

function tests.another_event_is_left_alone()
	check(handle({ event = "tick", now = 0, settings = {} }) == nil, "nothing")
end
