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
	eq(w.tip, "CPU 34% · RAM 61% · GPU 22%", "tip")
	eq(#w.panel.body[1].items, 3, "three rings")
	eq(w.panel.sub, "Busiest: RAM at 61%", "busiest")
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

local function busy(cpu: number, now: number)
	return handle({ event = "system", now = now, settings = { limit = 90 }, system = { cpu = cpu, ram = 20, ram_used_gb = 3, ram_total_gb = 16 } })
end

function tests.a_banner_comes_after_a_few_busy_readings_and_can_be_snoozed()
	check(busy(95, 2000).say == nil, "one")
	check(busy(95, 4000).say == nil, "two")
	local r = busy(95, 6000)
	eq(r.say[1].text, "The processor is busy", "three")
	check(busy(95, 8000).say == nil, "said once")
	eq(handle({ event = "action", action = "snooze_hour", now = 9000, settings = {} }).cancel, { "busy" }, "snoozed")
	for i = 1, 5 do
		check(busy(95, 10000 + i * 2000).say == nil, "for an hour")
	end
	check(busy(95, 9000 + 3600000).say ~= nil, "an hour later")
	eq(busy(20, 9000 + 3602000).cancel, { "busy" }, "calm again: taken back")
end
