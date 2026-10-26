--!strict
-- Calendar: the events of a calendar link (.ics: Google Calendar, Outlook, iCloud). It reads the link itself (`net`: the host of the
-- address in its setting, nothing else) every few minutes, and NADI turns the feed into events (`nadi.ics`). At rest: the date and the
-- time (NADI keeps the clock ticking), and what is next in its tip. Open: a month, a day picked on it, the events of that day. A banner
-- shortly before an event starts.

local LISTED = 6
local LOOK_MS = 15000 -- how often it looks at the clock
local DAY_MS = 86400000
local MONTHS = { "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December" }
local WEEKDAYS = { "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday" }
local COLUMNS = { "S", "M", "T", "W", "T", "F", "S" } -- Sunday first
local TINT = "#4f8ff0"

local events = {}
local told = {}
local month = 0 -- the month the panel shows: year * 12 + (month - 1)
local sel = 0 -- the day picked, as days since 1970 (local)
local last_fetch: number? = nil
local fetching = false
local problem: string? = nil
local url_seen = ""

local function of_day(n: number, tz: number)
	return nadi.time((n * 86400 + 43200) * 1000 - tz * 60000, tz)
end

local function first_of(m: number): number
	return nadi.moment(m // 12, m % 12 + 1, 1) // DAY_MS
end

local function day_start(n: number, tz: number): number
	return n * DAY_MS - tz * 60000
end

-- does the event touch the day (the end is exclusive: a 00:00-00:00 all-day event covers exactly one day)
local function on_day(e, n: number, tz: number): boolean
	local from, to = day_start(n, tz), day_start(n + 1, tz)
	local last = if e.end_ms > e.start_ms then e.end_ms - 1 else e.start_ms
	return e.start_ms < to and last >= from
end

local function clock(t, h24: boolean): string
	if h24 then
		return string.format("%02d:%02d", t.hour, t.minute)
	end
	local h = t.hour % 12
	return string.format("%d:%02d %s", if h == 0 then 12 else h, t.minute, if t.hour < 12 then "AM" else "PM")
end

-- "3:00 PM – 4:00 PM", "All day", "3:00 PM →", "→ 4:00 PM", as the event sits on the day
local function span_text(e, n: number, tz: number, h24: boolean): string
	if e.all_day then
		return "All day"
	end
	local function at(ms: number): string
		return clock(nadi.time(ms, tz), h24)
	end
	if e.end_ms <= e.start_ms then
		return at(e.start_ms)
	end
	local from, to = day_start(n, tz), day_start(n + 1, tz)
	local starts, ends = e.start_ms >= from, e.end_ms <= to
	if starts and ends then
		return at(e.start_ms) .. " – " .. at(e.end_ms)
	elseif starts then
		return at(e.start_ms) .. " →"
	elseif ends then
		return "→ " .. at(e.end_ms)
	end
	return "All day"
end

-- the events that have not finished (an hour of grace after the end), first ones first
local function upcoming(now: number)
	local out = {}
	for _, e in events do
		if e.end_ms >= now - 3600000 then
			table.insert(out, e)
			if #out >= LISTED then
				break
			end
		end
	end
	return out
end

local function today_of(now: number, tz: number): number
	return (now + tz * 60000) // DAY_MS
end

local function go_to(n: number, tz: number)
	sel = n
	local t = of_day(n, tz)
	month = t.year * 12 + t.month - 1
end

local function panel(ev)
	local tz = ev.tz_offset or 0
	local h24 = ev.settings.h24 == true
	local today = today_of(ev.now, tz)
	local week_start = if ev.settings.week_start == 1 then 1 else 0
	local first = first_of(month)
	local sunday_based = (of_day(first, tz).weekday + 1) % 7 -- (weekday 0 is Monday)
	local start = first - (sunday_based + 7 - week_start) % 7
	local labels = {}
	for i = 0, 6 do
		table.insert(labels, COLUMNS[(i + week_start) % 7 + 1])
	end
	local cells = {}
	for i = 0, 41 do
		local n = start + i
		local t = of_day(n, tz)
		local dots = 0
		for _, e in events do
			if dots >= 3 then
				break
			end
			if on_day(e, n, tz) then
				dots += 1
			end
		end
		table.insert(cells, {
			t = tostring(t.day),
			id = "d:" .. n,
			state = if n == sel then "sel" elseif n == today then "today" elseif t.month ~= month % 12 + 1 then "other" else "",
			dots = dots,
		})
	end
	local body: { any } = {
		{ kind = "head", title = MONTHS[month % 12 + 1] .. " " .. (month // 12), prev = "prev", next = "next", press = "today" },
		{ kind = "grid", labels = labels, cells = cells },
	}
	local url = ev.settings.url or ""
	if string.match(url, "^%s*$") then
		table.insert(body, { kind = "text", tone = "muted", text = "No calendar link yet. Paste one in this plugin's settings." })
		return body
	end
	if problem then
		table.insert(body, { kind = "text", tone = "muted", text = problem })
	end
	local list = {}
	for _, e in events do
		if on_day(e, sel, tz) then
			table.insert(list, e)
		end
	end
	table.sort(list, function(a, b)
		if a.all_day ~= b.all_day then
			return a.all_day
		end
		return a.start_ms < b.start_ms
	end)
	if #list > 0 then
		local t = of_day(sel, tz)
		table.insert(body, { kind = "text", tone = "muted", text = WEEKDAYS[t.weekday + 1] .. ", " .. t.mon .. " " .. t.day .. " · " .. #list .. (if #list == 1 then " event" else " events") })
		local rows = {}
		for _, e in list do
			table.insert(rows, { title = e.summary, time = span_text(e, sel, tz, h24), past = e.end_ms < ev.now and not e.all_day })
		end
		table.insert(body, { kind = "rows", rows = rows })
	end
	return body
end

local function widget(ev)
	local tz = ev.tz_offset or 0
	local h24 = ev.settings.h24 == true
	local next_up = upcoming(ev.now)[1]
	local tip = "Nothing coming up"
	if next_up then
		local t = nadi.time(next_up.start_ms, tz)
		tip = next_up.summary .. " · " .. t.wday .. ", " .. t.mon .. " " .. t.day .. " · " .. (if next_up.all_day then "all day" else clock(t, h24))
	end
	local now_t = nadi.time(ev.now, tz)
	return {
		peek = { clock = { h24 = h24, date = true }, icon = "calendar", color = TINT, text = clock(now_t, h24), label = tip },
		panel = { title = "Calendar", sub = now_t.wday .. ", " .. now_t.mon .. " " .. now_t.day, tile = { icon = "calendar", color = TINT }, body = panel(ev) },
		tip = tip,
	}
end

local function ask(ev, out)
	local url = string.match(ev.settings.url or "", "^%s*(.-)%s*$") or ""
	if url == "" then
		return
	end
	url = string.gsub(url, "^webcal://", "https://")
	fetching = true
	last_fetch = ev.now
	out.fetch = { { id = "ics", method = "GET", url = url, max_kb = 8000 } }
end

function handle(ev)
	local tz = ev.tz_offset or 0
	local now = ev.now
	local out: any = { wake_ms = LOOK_MS }
	local e = ev.event
	if e == "start" then
		told, fetching, last_fetch, problem = {}, false, nil, nil
		go_to(today_of(now, tz), tz)
		url_seen = ev.settings.url or ""
		ask(ev, out)
	elseif e == "fetched" then
		fetching = false
		if ev.status == 200 then
			events = nadi.ics(ev.body or "", now)
			problem = nil
			out.note = #events .. " events read."
		else
			problem = "The calendar could not be read: " .. (if (ev.error or "") ~= "" then ev.error else "HTTP " .. tostring(ev.status))
			out.note = problem
		end
	elseif e == "action" then
		local a = ev.action or ""
		if a == "prev" or a == "next" then
			month += if a == "prev" then -1 else 1
			-- (the same day of the month where it has one)
			local d = of_day(sel, tz).day
			local last = first_of(month + 1) - first_of(month)
			sel = first_of(month) + math.min(d - 1, last - 1)
		elseif a == "today" then
			go_to(today_of(now, tz), tz)
		elseif string.match(a, "^d:") then
			local n = tonumber(string.sub(a, 3))
			if n then
				go_to(n, tz)
			end
		elseif a == "open" then
			go_to(today_of(now, tz), tz)
		end
	elseif e == "tick" then
		local url = ev.settings.url or ""
		local every = math.max(1, ev.settings.poll_min or 10) * 60000
		if url ~= url_seen then
			url_seen = url
			events, problem = {}, nil
			ask(ev, out)
		elseif not fetching and (last_fetch == nil or now - last_fetch >= every) then
			ask(ev, out)
		end
	else
		return nil
	end
	if ev.settings.remind ~= false then
		local lead = math.max(1, ev.settings.lead_min or 15) * 60000
		for _, x in events do
			local delta = x.start_ms - now
			if delta > 0 and delta <= lead and not told[x.uid .. ":" .. x.start_ms] then
				told[x.uid .. ":" .. x.start_ms] = true
				out.say = out.say or {}
				table.insert(out.say, { text = x.summary, priority = 40, pill = { icon = "calendar", right = "in " .. math.max(1, delta // 60000) .. " min", key = "cal:" .. x.uid, ms = 6000 } })
			end
		end
	end
	out.widgets = { calendar = widget(ev) }
	return out
end
