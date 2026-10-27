--!strict
-- Downloads: what your browsers, Steam and qBittorrent are downloading right now.
--
-- NADI watches what those apps leave on disk and tells the plugin what is downloading (`observed`) and what stopped showing up
-- (`ended`: done, or given up). The plugin keeps the list: it smooths the speed, drops a download that stands still (it comes back
-- when it moves), and keeps a finished one until you press it.

local STALL_MS = 5000 -- nothing came in for this long: stuck or paused, so it goes
local HISTORY = 30 -- samples of the combined speed (one each look, about every 1.5 s)
local MAX_ACTIVE, MAX_DONE = 8, 4
local TINT = "#4f8ff0"

local items = {}
local stalled = {} -- id -> the bytes it had when it was dropped for standing still
local stalled_n = 0
local speeds = {}

local function bytes(n: number): string
	if n < 1024 then
		return math.floor(n + 0.5) .. " B"
	end
	local units = { "KB", "MB", "GB", "TB" }
	local v, u = n / 1024, 1
	while v >= 1024 and u < #units do
		v /= 1024
		u += 1
	end
	if v >= 100 then
		return math.floor(v + 0.5) .. " " .. units[u]
	end
	return string.format("%.1f %s", v, units[u])
end

local function speed_text(speed: number): string
	return if speed > 1024 then bytes(speed) .. "/s" else ""
end

local function find(id: string, done: boolean?): number?
	for i, it in items do
		if it.id == id and (done == nil or it.done == done) then
			return i
		end
	end
	return nil
end

local function merge(now: number, observed, ended)
	for _, o in observed or {} do
		local moving = (o.speed or 0) > 0
		local i = find(o.id, false)
		if i then
			local it = items[i]
			if o.received > it.received or moving then
				it.moved = now
			end
			local dt = (now - it.last_at) / 1000
			if dt >= 0.5 then
				local inst = math.max(0, o.received - it.last_bytes) / dt
				it.speed = if it.speed == 0 then inst else it.speed * 0.6 + inst * 0.4
				it.last_bytes = o.received
				it.last_at = now
			end
			if o.speed then
				it.speed = o.speed
			end
			it.received, it.total, it.name = o.received, o.total, o.name
		elseif not find(o.id) then
			local b = stalled[o.id]
			if b == nil or o.received > b or moving then
				if b ~= nil then
					stalled[o.id] = nil
					stalled_n -= 1
				end
				table.insert(items, {
					id = o.id,
					source = o.source,
					kind = o.kind,
					name = o.name,
					received = o.received,
					total = o.total,
					speed = 0,
					done = false,
					last_bytes = o.received,
					last_at = now,
					started = now,
					moved = now,
				})
			end
		end
	end
	for _, e in ended or {} do
		local i = find(e.id, false)
		if i then
			if e.finished then
				local it = items[i]
				it.done, it.speed, it.path = true, 0, e.path
				if it.total then
					it.received = it.total
				end
			else
				table.remove(items, i)
			end
		end
	end
	if stalled_n > 64 then
		stalled, stalled_n = {}, 0
	end
	local kept = {}
	for _, it in items do
		if not it.done and now - it.moved > STALL_MS then
			stalled[it.id] = it.received
			stalled_n += 1
		else
			table.insert(kept, it)
		end
	end
	items = kept
	local total = 0
	for _, it in items do
		if not it.done then
			total += it.speed
		end
	end
	table.insert(speeds, total)
	if #speeds > HISTORY then
		table.remove(speeds, 1)
	end
end

local function progress(it): number?
	-- (Steam writes no progress anywhere)
	if it.kind == "steam" or not it.total or it.total == 0 then
		return nil
	end
	return math.clamp(it.received / it.total, 0, 1)
end

-- "42% · 3.1 MB/s", or what Steam gives: its size and the speed
local function meta(it): string
	local speed = speed_text(it.speed)
	local function join(a: string, b: string): string
		if a ~= "" and b ~= "" then
			return a .. " · " .. b
		end
		return if a ~= "" then a else b
	end
	if it.kind == "steam" then
		local words = join(if it.total then bytes(it.total) else "", speed)
		return if words == "" then "Downloading" else words
	end
	if it.total and it.total > 0 then
		return join(math.min(100, math.floor(it.received / it.total * 100)) .. "%", speed)
	end
	return join(bytes(it.received), speed)
end

local function cut(name: string, n: number): string
	if (utf8.len(name) or #name) <= n then
		return name
	end
	local stop = utf8.offset(name, n) or (#name + 1)
	return (string.gsub(string.sub(name, 1, stop - 1), "%s+$", "")) .. "…"
end

local function draw()
	if #items == 0 then
		-- (nothing coming in: idle, so NADI can hide it)
		return { widgets = { downloads = { idle = true, peek = { icon = "download", text = "No downloads" }, tip = "No downloads" } } }
	end
	local active, done = {}, {}
	for _, it in items do
		table.insert(if it.done then done else active, it)
	end
	table.sort(active, function(a, b)
		return a.started < b.started
	end)
	local speed = 0
	for _, it in active do
		speed += it.speed
	end
	local sub = if #active == 0 then #done .. " done" elseif #done == 0 then #active .. " downloading" else #active .. " downloading · " .. #done .. " done"
	local body: { any } = {}
	local top = 0
	for _, v in speeds do
		top = math.max(top, v)
	end
	if top > 0 and #active > 0 then
		table.insert(body, { kind = "graph", values = table.clone(speeds), max = math.max(top * 1.15, 100000) })
	end
	if #active > 0 then
		local rows = {}
		for i, it in active do
			if i > MAX_ACTIVE then
				break
			end
			table.insert(rows, { title = it.source .. " · " .. it.name, sub = meta(it), progress = progress(it) })
		end
		table.insert(body, { kind = "list", rows = rows })
	end
	if #done > 0 then
		local buttons = {}
		for i, it in done do
			if i > MAX_DONE then
				break
			end
			local verb = if it.kind == "browser" and it.path then "Show" else "Clear"
			table.insert(buttons, { label = verb .. " " .. cut(it.name, 28), action = "done:" .. it.id })
		end
		table.insert(body, { kind = "buttons", items = buttons })
	end
	local first = active[1]
	local part = if first then progress(first) else nil
	local text = if #active == 0 then #done .. " done" elseif speed_text(speed) ~= "" then speed_text(speed) else #active .. " downloading"
	return {
		widgets = {
			downloads = {
				peek = {
					icon = "download",
					ring = if part then { part = part } else nil,
					color = TINT,
					text = text,
					label = if first then cut(first.name, 40) .. " · " .. meta(first) else sub,
					count = if #active > 1 then #active else nil,
				},
				panel = { body = body },
				tip = sub,
			},
		},
	}
end

function handle(ev)
	if ev.event == "start" then
		items, stalled, stalled_n, speeds = {}, {}, 0, {}
		return nil
	elseif ev.event == "downloads" then
		merge(ev.now, ev.observed, ev.ended)
		return draw()
	elseif ev.event == "action" then
		local id = string.match(ev.action or "", "^done:(.+)$")
		local i = if id then find(id, true) else nil
		if not i then
			return nil
		end
		local it = table.remove(items, i)
		local out: any = draw()
		if it and it.kind == "browser" and it.path then
			out.reveal = it.path
		end
		return out
	end
	return nil
end
