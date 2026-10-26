--!strict
-- System Meters: how busy the CPU, the memory and the graphics card are. One widget: three thin bars at rest (the numbers in its tip),
-- and open, a ring for each with the memory in use. When the processor stays above a limit for a few readings, a banner says so, with
-- buttons that snooze it. It reads the `system` sensor (every 2 seconds) and sees nothing else.

local CPU, RAM, GPU = "#5ac8e6", "#b47ee6", "#8adb6e"
local WARM, HOT = "#e6a15a", "#e5675c"
local READINGS = 3 -- readings in a row above the limit before the banner comes

local over = 0
local snoozed_until = 0
local said = false

-- the colour of a meter: its own, until the load asks for attention
local function tone(pct: number, own: string): string
	if pct >= 95 then
		return HOT
	elseif pct >= 80 then
		return WARM
	end
	return own
end

local function pct(x: number): string
	return math.floor(x + 0.5) .. "%"
end

function handle(ev)
	if ev.event == "action" then
		local minutes = if ev.action == "snooze" then 10 elseif ev.action == "snooze_hour" then 60 else nil
		if minutes then
			snoozed_until = ev.now + minutes * 60000
			over, said = 0, false
			return { cancel = { "busy" } }
		end
		return nil
	elseif ev.event ~= "system" or not ev.system then
		return nil
	end
	local s = ev.system
	local meters = { { label = "CPU", pct = s.cpu, own = CPU }, { label = "RAM", pct = s.ram, own = RAM } }
	if s.gpu then
		table.insert(meters, { label = "GPU", pct = s.gpu, own = GPU })
	end
	local bars, rings, tip = {}, {}, {}
	local busiest = meters[1]
	for _, m in meters do
		local color = tone(m.pct, m.own)
		table.insert(bars, { part = m.pct / 100, color = color })
		table.insert(rings, { label = m.label, part = m.pct / 100, color = color, text = pct(m.pct) })
		table.insert(tip, m.label .. " " .. pct(m.pct))
		if m.pct > busiest.pct then
			busiest = m
		end
	end
	local memory = string.format("%.1f of %.1f GB", s.ram_used_gb or 0, s.ram_total_gb or 0)
	local out: any = {
		widgets = {
			meters = {
				peek = { bars = bars, label = table.concat(tip, " · ") },
				panel = {
					title = "System",
					sub = "Busiest: " .. busiest.label .. " at " .. pct(busiest.pct),
					tile = { icon = "sliders", color = CPU },
					body = { { kind = "rings", items = rings }, { kind = "facts", rows = { { "Memory", memory } } } },
				},
				tip = table.concat(tip, " · "),
			},
		},
	}
	-- the banner: the processor stayed busy
	local limit = ev.settings.limit or 90
	over = if s.cpu > limit then over + 1 else 0
	if ev.settings.alert ~= false and over >= READINGS and ev.now >= snoozed_until then
		if not said then
			said = true
			out.say = {
				{
					text = "The processor is busy",
					priority = 50,
					pill = {
						icon = "timer",
						key = "busy",
						tone = "low",
						sub = pct(s.cpu) .. " for " .. (over * 2) .. " s",
						ms = 8000,
						buttons = { { label = "Snooze 10 minutes", action = "snooze" }, { label = "Snooze 1 hour", action = "snooze_hour" } },
					},
				},
			}
		end
	elseif over == 0 and said then
		said = false
		out.cancel = { "busy" }
	end
	return out
end
