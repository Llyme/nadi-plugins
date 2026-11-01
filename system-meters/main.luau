--!strict
-- System Meters: how busy the CPU, the memory and the graphics card are. At rest, a small graph of the last minute of each and its
-- number now (the graphics card only while it works); calm for a while (all of them under their marks), it rests out of sight.
-- Opened: a graph of each over the last minute, ten minutes or hour, and the programs using the most. It reads the `system`
-- sensor (every 2 seconds; the busiest programs only while it is open) and sees nothing else.

local CPU, RAM, GPU = "#5ac8e6", "#b47ee6", "#8adb6e"
local WARM, HOT = "#e6a15a", "#e5675c"

-- calm: under these (the CPU's mark is a setting), for this long
local RAM_CALM, GPU_CALM = 85, 50
local CALM_FOR = 10000

local MINUTE, TEN, HOUR = 30, 60, 60 -- how many points each graph keeps: every 2 s, every 10 s, every minute

type Hist = { min: { number }, ten: { number }, hour: { number }, acc10: { number }, acc60: { number } }

local function hist(): Hist
	return { min = {}, ten = {}, hour = {}, acc10 = {}, acc60 = {} }
end

local h = { cpu = hist(), ram = hist(), gpu = hist() }
local range = "1m"
local calm_since: number? = nil
local last: any = nil

local function push(list: { number }, v: number, max: number)
	table.insert(list, v)
	while #list > max do
		table.remove(list, 1)
	end
end

local function avg(list: { number }): number
	local s = 0
	for _, v in list do
		s += v
	end
	return if #list > 0 then s / #list else 0
end

-- a reading into its graphs: each 2 s into the minute's; their average each 10 s into the ten minutes', each minute into the hour's
local function add(x: Hist, v: number)
	push(x.min, v, MINUTE)
	table.insert(x.acc10, v)
	if #x.acc10 >= 5 then
		local a = avg(x.acc10)
		x.acc10 = {}
		push(x.ten, a, TEN)
		table.insert(x.acc60, a)
		if #x.acc60 >= 6 then
			push(x.hour, avg(x.acc60), HOUR)
			x.acc60 = {}
		end
	end
end

local function values(x: Hist, which: string): { number }
	local src = if which == "10m" then x.ten elseif which == "1h" then x.hour else x.min
	local out = {}
	for _, v in src do
		table.insert(out, math.clamp(v / 100, 0, 1))
	end
	return out
end

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

local function draw(ev, s)
	local settings = ev.settings or {}
	local cpu_calm = tonumber(settings.calm or "50") or 50
	local gpu = s.gpu
	local items = {
		{ label = "CPU", values = values(h.cpu, "1m"), color = tone(s.cpu, CPU), text = pct(s.cpu) },
		{ label = "RAM", values = values(h.ram, "1m"), color = tone(s.ram, RAM), text = pct(s.ram) },
	}
	if gpu and gpu >= 5 then
		table.insert(items, { label = "GPU", values = values(h.gpu, "1m"), color = tone(gpu, GPU), text = pct(gpu) })
	end
	-- calm: under every mark, for a while; then it rests (out of sight until something is heavy again)
	local now = ev.now or 0
	local calm = s.cpu < cpu_calm and s.ram < RAM_CALM and (gpu == nil or gpu < GPU_CALM)
	if calm then
		calm_since = calm_since or now
	else
		calm_since = nil
	end
	local idle = settings.hide_calm ~= false and calm_since ~= nil and now - (calm_since :: number) >= CALM_FOR
	-- opened: a graph of each over the chosen while, and the busiest programs
	local seg = {}
	for _, r in { { "1m", "1 min" }, { "10m", "10 min" }, { "1h", "1 h" } } do
		table.insert(seg, { label = r[2], action = "range:" .. r[1], on = range == r[1] })
	end
	local body = {
		{ kind = "toolbar", items = { { kind = "grow" }, { kind = "seg", items = seg } } },
		{ kind = "chart", label = "CPU", text = pct(s.cpu), color = tone(s.cpu, CPU), values = values(h.cpu, range), height = 46 },
		{ kind = "chart", label = "RAM", text = pct(s.ram), color = tone(s.ram, RAM), values = values(h.ram, range), height = 30, sub = string.format("%.1f of %.1f GB", s.ram_used_gb or 0, s.ram_total_gb or 0) },
	}
	if gpu then
		table.insert(body, { kind = "chart", label = "GPU", text = pct(gpu), color = tone(gpu, GPU), values = values(h.gpu, range), height = 30 })
	end
	if type(s.top) == "table" and #s.top > 0 then
		local rows = {}
		for _, p in s.top do
			table.insert(rows, { image = "exe:" .. (p.exe or ""), name = p.name or "", value = math.clamp((p.cpu or 0) / 50, 0, 1), color = tone(p.cpu or 0, CPU), text = pct(p.cpu or 0) })
		end
		table.insert(body, { kind = "usage", rows = rows })
	end
	local tip = {}
	for _, it in items do
		table.insert(tip, it.label .. " " .. it.text)
	end
	return {
		widgets = {
			meters = {
				idle = idle,
				peek = { graphs = { items = items } },
				panel = { width = 400, body = body },
				tip = table.concat(tip, "\n"),
			},
		},
	}
end

function handle(ev)
	if ev.event == "action" and type(ev.action) == "string" and string.sub(ev.action, 1, 6) == "range:" then
		range = string.sub(ev.action, 7)
		return if last then draw(ev, last) else nil
	end
	if ev.event ~= "system" or not ev.system then
		return nil
	end
	local s = ev.system
	add(h.cpu, s.cpu or 0)
	add(h.ram, s.ram or 0)
	if s.gpu then
		add(h.gpu, s.gpu)
	end
	last = s
	return draw(ev, s)
end
