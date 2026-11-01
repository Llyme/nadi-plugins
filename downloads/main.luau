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

-- how many have finished since it began (a new one: it comes out of its edge for a moment)
local finished_count = 0

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
				finished_count += 1
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
			return a .. ", " .. b
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

-- how long until it is done, short: "12 s", "3 min", "1 h 5 min" (nil: not known)
local function left_of(received: number, total: number?, speed: number): string?
	if not total or total <= 0 or speed < 1024 then
		return nil
	end
	local s = math.max(0, total - received) / speed
	if s < 60 then
		return math.max(1, math.floor(s + 0.5)) .. " s"
	elseif s < 3600 then
		return math.floor(s / 60 + 0.5) .. " min"
	end
	local h = math.floor(s / 3600)
	local m = math.floor((s - h * 3600) / 60)
	return if m > 0 then h .. " h " .. m .. " min" else h .. " h"
end

local SOURCE_ICON = { browser = "globe", steam = "gamepad", torrent = "download" }

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
	local got, all, known = 0, 0, true
	for _, it in active do
		speed += it.speed
		if progress(it) then
			got += math.min(it.received, it.total)
			all += it.total
		else
			known = false
		end
	end
	local body: { any } = {}
	-- (on top: how fast all of it comes in, as a line)
	local top = 0
	for _, v in speeds do
		top = math.max(top, v)
	end
	if #active > 0 and top > 0 then
		table.insert(body, { kind = "linegraph", values = table.clone(speeds), max = math.max(top * 1.15, 100000), text = speed_text(speed), color = TINT })
	end
	for i, it in active do
		if i > MAX_ACTIVE then
			break
		end
		local appid = if it.kind == "steam" then string.match(it.id, "^steam:(%d+)$") else nil
		local part = progress(it)
		table.insert(body, {
			kind = "transfer",
			image = if appid then "steamart:" .. appid else nil,
			wide = appid ~= nil,
			icon = SOURCE_ICON[it.kind] or "download",
			title = it.name,
			sub = if speed_text(it.speed) ~= "" then speed_text(it.speed) else it.source,
			right = if part then left_of(it.received, it.total, it.speed) or math.floor(part * 100) .. "%" else nil,
			part = part,
			color = TINT,
		})
	end
	for i, it in done do
		if i > MAX_DONE then
			break
		end
		local buttons = {}
		if it.path then
			table.insert(buttons, { icon = "folder", action = "show:" .. it.id, tip = "Show in Folder" })
		end
		table.insert(buttons, { icon = "x", action = "clear:" .. it.id, tip = "Clear" })
		table.insert(body, { kind = "transfer", done = true, title = it.name, action = if it.path then "open:" .. it.id else nil, buttons = buttons })
	end
	-- at rest: everything coming in, in one ring, and the time until all of it is done; all done: a check
	local first = active[1]
	local part = if #active > 0 and known and all > 0 then got / all else (if first then progress(first) else nil)
	local slowest: string? = nil
	local worst = -1
	for _, it in active do
		if progress(it) and it.speed >= 1024 then
			local s = (it.total - it.received) / it.speed
			if s > worst then
				worst, slowest = s, left_of(it.received, it.total, it.speed)
			end
		end
	end
	local text = if #active == 0 then nil else slowest or (if speed_text(speed) ~= "" then speed_text(speed) else nil)
	local sub = if #active == 0 then #done .. " done" elseif #done == 0 then #active .. " downloading" else #active .. " downloading, " .. #done .. " done"
	return {
		widgets = {
			downloads = {
				peek = {
					icon = if #active == 0 then "check" else "download",
					ring = if #active == 0 then { part = 1, theme = true } elseif part then { part = part } else nil,
					color = if #active == 0 then nil else TINT,
					text = text,
					label = if first then cut(first.name, 40) .. ", " .. meta(first) else sub,
				},
				panel = { body = body },
				tip = if speed_text(speed) ~= "" then sub .. ", " .. speed_text(speed) else sub,
				bump = if finished_count > 0 then finished_count else nil,
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
		local verb, id = string.match(ev.action or "", "^(%a+):(.+)$")
		local i = if id then find(id, true) else nil
		if not i then
			return nil
		end
		local it = items[i]
		if verb == "open" and it.path then
			local out: any = draw()
			out.open_file = it.path
			return out
		elseif verb == "show" and it.path then
			local out: any = draw()
			out.reveal = it.path
			return out
		elseif verb == "clear" or verb == "done" then
			table.remove(items, i)
			return draw()
		end
	end
	return nil
end
