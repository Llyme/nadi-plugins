--!strict
-- System Meters: how busy the CPU, the memory and the graphics card are. One widget: three thin bars at rest (the numbers in its tip),
-- and open, a ring for each with the memory in use. It reads the `system` sensor (every 2 seconds) and sees nothing else.

local CPU, RAM, GPU = "#5ac8e6", "#b47ee6", "#8adb6e"
local WARM, HOT = "#e6a15a", "#e5675c"

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
	if ev.event ~= "system" or not ev.system then
		return nil
	end
	local s = ev.system
	local meters = { { label = "CPU", pct = s.cpu, own = CPU }, { label = "RAM", pct = s.ram, own = RAM } }
	if s.gpu then
		table.insert(meters, { label = "GPU", pct = s.gpu, own = GPU })
	end
	local bars, rings, tip = {}, {}, {}
	for _, m in meters do
		local color = tone(m.pct, m.own)
		table.insert(bars, { part = m.pct / 100, color = color })
		table.insert(rings, { label = m.label, part = m.pct / 100, color = color, text = pct(m.pct) })
		table.insert(tip, m.label .. " " .. pct(m.pct))
	end
	local memory = string.format("%.1f of %.1f GB", s.ram_used_gb or 0, s.ram_total_gb or 0)
	return {
		widgets = {
			meters = {
				peek = { bars = bars, label = table.concat(tip, " · ") },
				panel = {
					body = { { kind = "rings", items = rings }, { kind = "facts", rows = { { "Memory", memory } } } },
				},
				tip = table.concat(tip, " · "),
			},
		},
	}
end
