--!strict
-- Battery: how full the battery is, as a liquid in a vessel. One widget (a vessel with the number at rest; open, the charge of the last three
-- hours, how it stands, how worn it is) and a banner when it runs low or critical, is plugged in or out, is full, battery saver comes on,
-- or it is wearing out. The island tells the plugin the battery (`battery`, with the `battery` permission) and draws the liquid; the
-- plugin only decides what to say. A PC with no battery is told so once, and shows nothing.

local GREEN, AMBER, RED, GREY = "#5ac88c", "#e8b04a", "#e87a7a", "#8e8e96"
-- the charge of this many hours is kept for the graph, with at most this many points, a sample at least this far apart
local HOURS_MS = 3 * 3600000
local POINTS = 54
local SAMPLE_MS = 120000

local seen = nil -- the battery as it was last told: what a change is a change from
local samples = {} -- { at, percent } over the last three hours
local health_said = nil -- the lowest health a banner has said
local told_none = false

local function round(x: number): number
	return math.floor(x + 0.5)
end

-- the colour of the liquid: green, amber at the low mark, red at the critical one, and green whatever the number while it charges
local function color_of(percent: number, charging: boolean, low: number, critical: number): string
	if charging then
		return GREEN
	elseif percent <= critical then
		return RED
	elseif percent <= low then
		return AMBER
	end
	return GREEN
end

-- what is left or what is to come, in a few words
local function left_of(b): string
	if b.charging then
		return if b.minutes_to_full then nadi.duration(b.minutes_to_full) .. " to full" else "Charging"
	elseif b.plugged then
		return if (b.percent or 0) >= 99.5 then "Fully charged" else "Plugged in"
	elseif b.minutes_left then
		return nadi.duration(b.minutes_left) .. " left"
	end
	return ""
end

local function sub_of(b): string
	local left = left_of(b)
	local head = if b.charging then "Charging" elseif b.plugged then "Plugged in" else "On battery"
	if left == "" or left == head then
		return head
	elseif b.plugged and not b.charging then
		return left
	end
	return head .. ", " .. left
end

local function about(b): string
	local p = round(b.percent or 0) .. "%"
	return if b.minutes_left then p .. ", about " .. nadi.duration(b.minutes_left) .. " left" else p
end

local function graph_of(): { number }
	local out = {}
	local n = #samples
	if n <= POINTS then
		for _, s in samples do
			table.insert(out, s[2])
		end
		return out
	end
	for i = 0, POINTS - 1 do
		table.insert(out, samples[math.floor(i * (n - 1) / (POINTS - 1)) + 1][2])
	end
	return out
end

-- the vessel of a battery, as a tile: its number in it, filled to the charge
local function liquid(b, color: string)
	local p = b.percent or 0
	return { text = tostring(round(p)), color = color, ring = p / 100, ring_color = color, vessel = true, bolt = b.charging or nil }
end

-- a banner: the circle is the liquid, filled to the charge
local function banner(title: string, sub: string, tone: string, color: string, b, ms: number, priority: number)
	return {
		text = title,
		priority = priority,
		ambient = true,
		pill = { icon = "battery", right = "", key = "battery", tone = tone, sub = sub, ms = ms, tile = liquid(b, color) },
	}
end

local function marks(settings): (number, number)
	local low = settings.low or 20
	-- (the critical mark is under the low one)
	local critical = math.max(1, math.min(settings.critical or 10, low - 1))
	return low, critical
end

-- What changed since the battery was last told, as one banner (the one that matters most).
local function banner_for(now, s)
	local prev = seen
	if not prev or s.alerts == false then
		return nil
	end
	local p, q = now.percent, prev.percent
	if not p or not q then
		return nil
	end
	local low, critical = marks(s)
	local function on(k: string, d: boolean): boolean
		local v = s[k]
		if v == nil then
			return d
		end
		return v == true
	end
	local color = color_of(p, now.charging, low, critical)
	local said = {}
	local draining = not now.plugged and not now.charging
	if draining and on("b_critical", true) and q > critical and p <= critical then
		table.insert(said, banner("Battery critical", about(now) .. ", plug in", "critical", RED, now, 8000, 90))
	elseif draining and on("b_critical", true) and critical > 5 and q > 5 and p <= 5 then
		-- (and again, at the last few percent)
		table.insert(said, banner("Battery critical", about(now) .. ", plug in", "critical", RED, now, 8000, 90))
	elseif draining and on("b_low", true) and q > low and p <= low then
		table.insert(said, banner("Battery low", about(now), "low", AMBER, now, 5000, 60))
	end
	if not prev.plugged and now.plugged and on("b_plugged", true) then
		local sub = round(p) .. "%"
		if now.minutes_to_full then
			sub ..= ", about " .. nadi.duration(now.minutes_to_full) .. " to full"
		end
		if now.rate_w and now.rate_w > 0.5 then
			sub ..= ", " .. round(now.rate_w) .. " W"
		end
		table.insert(said, banner("Charging", sub, "charging", GREEN, now, 3000, 40))
	end
	if prev.plugged and not now.plugged and on("b_unplugged", false) then
		local tone = if color == AMBER then "low" elseif color == RED then "critical" else "charging"
		table.insert(said, banner("On battery", about(now), tone, color, now, 3000, 40))
	end
	if now.plugged and p >= 99.5 and q < 99.5 and on("b_full", true) then
		table.insert(said, banner("Fully charged", "100%", "full", GREEN, now, 4000, 40))
	end
	if not prev.saver and now.saver and on("b_saver", false) then
		table.insert(said, banner("Battery saver on", round(p) .. "%", "saver", AMBER, now, 4000, 40))
	end
	if now.health and now.health < 80 and (health_said == nil or now.health <= health_said - 10) then
		health_said = now.health
		if on("b_health", false) then
			local wh = if now.full_wh and now.design_wh then ", " .. round(now.full_wh) .. " Wh of " .. round(now.design_wh) else ""
			table.insert(said, banner("Battery is wearing out", "Holds " .. round(now.health) .. "% of what it did new" .. wh, "health", GREY, now, 6000, 40))
		end
	end
	local best = nil
	for _, x in said do
		if not best or x.priority > best.priority then
			best = x
		end
	end
	return best
end

function handle(ev)
	if ev.event ~= "battery" then
		return nil
	end
	local b = ev.battery
	-- a PC with no battery: nothing is shown, and it is said once why
	if not b.present or not b.percent then
		seen = nil
		local out = { widgets = { battery = {} } }
		if not told_none then
			out.note = "No battery found: this PC runs on the wall."
		end
		told_none = true
		return out
	end
	told_none = false
	local percent = b.percent
	-- the charge of the last three hours
	local kept = {}
	for _, s in samples do
		if ev.now - s[1] <= HOURS_MS then
			table.insert(kept, s)
		end
	end
	samples = kept
	local last = samples[#samples]
	if not last or math.abs(last[2] - percent) >= 1 or ev.now - last[1] >= SAMPLE_MS then
		table.insert(samples, { ev.now, percent })
	end
	-- the banner (the first look is not news; what is already wrong is known)
	local said = banner_for(b, ev.settings)
	if seen == nil and b.health and b.health < 80 then
		health_said = b.health
	end
	seen = b

	local low, critical = marks(ev.settings)
	local color = color_of(percent, b.charging, low, critical)
	local big = round(percent) .. "%"
	local left = left_of(b)

	-- open: the chips, the last hours, four numbers
	local chips = {}
	if b.plugged then
		table.insert(chips, { icon = "plug", text = "Plugged in", tone = "ok" })
	end
	table.insert(chips, { icon = "leaf", text = if b.saver then "Battery saver on" else "Battery saver off", tone = if b.saver then "hot" else "" })
	if b.health then
		table.insert(chips, { icon = "heart", text = "Health " .. round(b.health) .. "%", tone = "" })
	end
	local body: { any } = { { kind = "chips", items = chips } }
	local points = graph_of()
	if #points >= 2 then
		table.insert(body, { kind = "graph", values = points, max = 100 })
	end
	local dash = "–"
	table.insert(body, {
		kind = "stats",
		items = {
			{ value = if b.rate_w then math.abs(round(b.rate_w)) .. " W" else dash, label = if b.charging then "Charging" else "Drawing" },
			{
				value = if b.charging then (if b.minutes_to_full then nadi.duration(b.minutes_to_full) else dash) else (if b.minutes_left then nadi.duration(b.minutes_left) else dash),
				label = if b.charging then "To full" else "Left",
			},
			{ value = if b.health then round(b.health) .. "%" else dash, label = "Health" },
			{ value = if b.full_wh then round(b.full_wh) .. " Wh" else dash, label = "Full charge" },
		},
	})

	local out: any = {
		widgets = {
			battery = {
				peek = { vessel = { level = percent / 100, charging = b.charging }, color = color, text = big, label = if left ~= "" then big .. ", " .. left else big },
				glow = if not b.charging and (color == AMBER or color == RED) then { color = color } else nil,
				panel = { title = sub_of(b), body = body },
				tip = { title = big, sub = sub_of(b), tile = { vessel = true, ring = percent / 100, bolt = b.charging, color = color } },
			},
		},
	}
	if said then
		out.say = { said }
	end
	return out
end
