--!strict
-- Calendar: the events of a calendar link (.ics: Google Calendar, Outlook, iCloud). It reads the link itself (`net`: the host of the
-- address in its setting, nothing else) every few minutes, and NADI turns the feed into events (`nadi.ics`). At rest: the date and the
-- time (NADI keeps the clock ticking), and what is next in its tip. Open: a month, a day picked on it, the events of that day. A banner
-- shortly before an event starts. And, if asked, a small banner now and then that says the time (what the Time plugin was): it lines
-- up with the clock (every 30 minutes means :00 and :30), once each.

local LISTED = 6
local LOOK_MS = 15000 -- how often it looks at the clock
local DAY_MS = 86400000
local MONTHS = { "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December" }
local WEEKDAYS = { "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday" }
local COLUMNS = { "S", "M", "T", "W", "T", "F", "S" } -- Sunday first
local TINT = "#4f8ff0"

local AMBER = "#e8b04a"
local HOUR_MS = 3600000

local events = {}
local view = "day" -- what the panel shows: the day (its week, what is next, its events) or the month
local told = {}
local month = 0 -- the month the panel shows: year * 12 + (month - 1)
local sel = 0 -- the day picked, as days since 1970 (local)
local last_fetch: number? = nil
local fetching = false
local problem: string? = nil
local url_seen = ""
local chimed: string? = nil -- the day and minute of the last time said: once each

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

-- the events of a day, all-day ones first
local function day_events(n: number, tz: number)
	local list = {}
	for _, e in events do
		if on_day(e, n, tz) then
			table.insert(list, e)
		end
	end
	table.sort(list, function(a, b)
		if a.all_day ~= b.all_day then
			return a.all_day
		end
		return a.start_ms < b.start_ms
	end)
	return list
end

-- a dot of its calendar's colour for each of a day's events (three at most)
local function dots_of(n: number, tz: number)
	local out = {}
	for _, e in events do
		if #out >= 3 then
			break
		end
		if on_day(e, n, tz) then
			table.insert(out, TINT)
		end
	end
	return out
end

-- the next event that starts at a time (not all day), or is going on, within a day
local function next_event(now: number)
	for _, e in events do
		if not e.all_day and e.end_ms > now and e.start_ms - now <= 24 * HOUR_MS then
			return e
		end
	end
	return nil
end

-- "12m", "1h 5m", "Now"
local function soon_text(ms: number): string
	if ms <= 0 then
		return "Now"
	end
	local m = math.ceil(ms / 60000)
	if m < 60 then
		return m .. "m"
	end
	local h = m // 60
	return if m % 60 > 0 then h .. "h " .. (m % 60) .. "m" else h .. "h"
end

-- "in 12 min", "in 1 h 5 min", "now"
local function soon_words(ms: number): string
	if ms <= 0 then
		return "now"
	end
	local m = math.ceil(ms / 60000)
	if m < 60 then
		return "in " .. m .. " min"
	end
	return "in " .. (m // 60) .. " h" .. (if m % 60 > 0 then " " .. (m % 60) .. " min" else "")
end

local function month_panel(ev)
	local tz = ev.tz_offset or 0
	local today = today_of(ev.now, tz)
	local week_start = if ev.settings.week_start == 1 then 1 else 0
	local first = first_of(month)
	local last = first_of(month + 1) - 1
	local sunday_based = (of_day(first, tz).weekday + 1) % 7 -- (weekday 0 is Monday)
	local labels = {}
	for i = 0, 6 do
		table.insert(labels, COLUMNS[(i + week_start) % 7 + 1])
	end
	local days = {}
	for n = first, last do
		local t = of_day(n, tz)
		table.insert(days, { num = tostring(t.day), today = n == today, sel = n == sel and n ~= today, weekend = t.weekday >= 5, dots = dots_of(n, tz), action = "d:" .. n })
	end
	return {
		{ kind = "monthgrid", title = MONTHS[month % 12 + 1] .. " " .. (month // 12), prev = "prev", next = "next", press = "view:day", labels = labels, lead = (sunday_based + 7 - week_start) % 7, days = days },
	}
end

local function panel(ev)
	if view == "month" then
		return month_panel(ev)
	end
	local tz = ev.tz_offset or 0
	local h24 = ev.settings.h24 == true
	local now = ev.now
	local today = today_of(now, tz)
	local week_start = if ev.settings.week_start == 1 then 1 else 0
	local now_t = nadi.time(now, tz)
	local sel_t = of_day(sel, tz)
	-- (the time, big; the day and the date picked on its right)
	local time = clock(now_t, h24)
	local ampm = ""
	if not h24 then
		ampm = string.sub(time, -2)
		time = string.sub(time, 1, -4)
	end
	local body: { any } = {
		{ kind = "dayhead", time = time, ampm = ampm, day = WEEKDAYS[sel_t.weekday + 1], date = MONTHS[sel_t.month] .. " " .. sel_t.day, toggle = "view:month", toggle_icon = "calendar", toggle_tip = "Month" },
	}
	-- (the week the day picked is in)
	local sunday_based = (sel_t.weekday + 1) % 7
	local start = sel - (sunday_based + 7 - week_start) % 7
	local days = {}
	for i = 0, 6 do
		local n = start + i
		local t = of_day(n, tz)
		table.insert(days, { label = COLUMNS[(i + week_start) % 7 + 1], num = tostring(t.day), on = n == sel, today = n == today, dots = dots_of(n, tz), action = "d:" .. n })
	end
	table.insert(body, { kind = "weekstrip", prev = "wprev", next = "wnext", days = days })
	local url = ev.settings.url or ""
	if string.match(url, "^%s*$") then
		table.insert(body, { kind = "text", tone = "muted", text = "No calendar link" })
		return body
	end
	if problem then
		table.insert(body, { kind = "text", tone = "muted", text = problem })
	end
	-- (what is next: a ring running down to it over its last hour, a button to join it)
	local nx = next_event(now)
	if nx then
		local left = nx.start_ms - now
		local soon = left <= 10 * 60000
		local nt = nadi.time(nx.start_ms, tz)
		local nday = today_of(nx.start_ms, tz)
		local when = span_text(nx, nday, tz, h24)
		if nday ~= today then
			when = (if nday == today + 1 then "Tomorrow" else nt.wday) .. ", " .. when
		end
		table.insert(body, {
			kind = "nextup",
			title = nx.summary,
			sub = if (nx.location or "") ~= "" then when .. ", " .. nx.location else when,
			part = math.clamp(left / HOUR_MS, 0, 1),
			text = soon_text(left),
			color = if soon then AMBER else TINT,
			join = if (nx.link or "") ~= "" then "join:" .. nx.uid else nil,
		})
	end
	-- (the day's events in time; a line where now is)
	local list = day_events(sel, tz)
	if #list > 0 then
		local rows = {}
		local now_at = -1
		for i, e in list do
			if sel == today and now_at < 0 and not e.all_day and e.start_ms > now then
				now_at = i - 1
			end
			local t = nadi.time(e.start_ms, tz)
			local at = if e.all_day then "All day" else clock(t, h24)
			if not h24 and not e.all_day then
				at = string.sub(at, 1, -4)
			end
			local span = span_text(e, sel, tz, h24)
			table.insert(rows, {
				time = at,
				title = e.summary,
				sub = if (e.location or "") ~= "" then span .. ", " .. e.location else span,
				color = TINT,
				past = sel < today or (sel == today and e.end_ms < now and not e.all_day),
				action = if (e.link or "") ~= "" then "join:" .. e.uid else nil,
			})
		end
		if sel == today and now_at < 0 then
			now_at = #rows
		end
		table.insert(body, { kind = "timeline", rows = rows, now = now_at })
	end
	return body
end

local function widget(ev)
	local tz = ev.tz_offset or 0
	local h24 = ev.settings.h24 == true
	local now = ev.now
	local next_up = upcoming(now)[1]
	local tip = "Nothing coming up"
	if next_up then
		local t = nadi.time(next_up.start_ms, tz)
		tip = next_up.summary .. "\n" .. t.wday .. ", " .. t.mon .. " " .. t.day .. ", " .. (if next_up.all_day then "all day" else clock(t, h24))
	end
	local now_t = nadi.time(now, tz)
	-- (an event within the hour: a dot after the date in its colour (amber and breathing in its last ten minutes); large, it beside the
	-- clock; its icon alone, the page's top in its colour, draining as it comes)
	local nx = next_event(now)
	local left = if nx then nx.start_ms - now else math.huge
	local within = nx ~= nil and left > 0 and left <= HOUR_MS
	local last10 = within and left <= 10 * 60000
	local c = if last10 then AMBER else TINT
	if within and nx then
		tip = nx.summary .. "\n" .. soon_words(left)
	end
	-- (an event about to start, within five minutes: it comes out of its edge for a moment)
	local soon = nx and left <= 5 * 60 * 1000 and left > 0
	return {
		peek = {
			clock = { h24 = h24, date = true, next = if within and nx then { title = nx.summary, when = soon_words(left), color = c } else nil },
			dot = within,
			dot_pulse = last10,
			color = if within then c else TINT,
			page = { mon = string.upper(now_t.mon), day = tostring(now_t.day), part = if within then left / HOUR_MS else nil, color = if within then c else nil },
			text = clock(now_t, h24),
			label = tip,
		},
		panel = { body = panel(ev) },
		tip = tip,
		bump = if soon and nx then nx.summary .. "|" .. tostring(nx.start_ms) else nil,
	}
end

-- the time, said in a small banner
local function chime(ev, ambient: boolean)
	local t = nadi.time(ev.now, ev.tz_offset or 0)
	return { text = clock(t, ev.settings.h24 == true), priority = 10, ambient = ambient, pill = { icon = "clock", right = t.wday .. ", " .. t.mon .. " " .. t.day, look = "big", ms = 4000 } }
end

-- The calendar links: several, one a line (or between spaces or commas); a webcal one read over https.
local function links(ev): { string }
	local out = {}
	for u in string.gmatch(ev.settings.url or "", "[^%s,;]+") do
		local l = string.gsub(u, "^webcal://", "https://")
		if not table.find(out, l) then
			table.insert(out, l)
		end
	end
	return out
end

-- Each calendar's events and problem, by its place among the links; merged as one (an event in two of them once).
local by_cal: { [number]: { any } } = {}
local problems: { [number]: string } = {}
local waiting = 0

local function merge()
	local seen, all = {}, {}
	for i = 1, 64 do
		for _, x in by_cal[i] or {} do
			local key = tostring(x.uid) .. "|" .. tostring(x.start_ms)
			if not seen[key] then
				seen[key] = true
				table.insert(all, x)
			end
		end
	end
	table.sort(all, function(a, b)
		return (a.start_ms or 0) < (b.start_ms or 0)
	end)
	events = all
	local p = {}
	for i = 1, 64 do
		if problems[i] then
			table.insert(p, problems[i])
		end
	end
	problem = if #p > 0 then table.concat(p, "\n") else nil
end

local function ask(ev, out)
	local ls = links(ev)
	if #ls == 0 then
		return
	end
	fetching = true
	last_fetch = ev.now
	waiting = #ls
	out.fetch = {}
	for i, url in ls do
		table.insert(out.fetch, { id = "ics:" .. i, method = "GET", url = url, max_kb = 8000 })
	end
end

function handle(ev)
	local tz = ev.tz_offset or 0
	local now = ev.now
	local out: any = { wake_ms = LOOK_MS }
	local e = ev.event
	if e == "start" then
		told, fetching, last_fetch, problem = {}, false, nil, nil
		by_cal, problems, waiting = {}, {}, 0
		go_to(today_of(now, tz), tz)
		url_seen = ev.settings.url or ""
		ask(ev, out)
	elseif e == "open" then
		-- (its panel opened: today, its day)
		local opened = if type(ev.open) == "table" then ev.open.calendar == true else ev.open == true
		if opened then
			go_to(today_of(now, tz), tz)
			view = "day"
		end
	elseif e == "fetched" then
		-- (one of the calendars: its events kept with its place; all in, the note says how many)
		local i = tonumber(string.match(ev.id or "", "^ics:(%d+)$")) or 1
		if ev.status == 200 then
			by_cal[i] = nadi.ics(ev.body or "", now)
			problems[i] = nil
		else
			problems[i] = (if #links(ev) > 1 then "Calendar " .. i .. " could not be read: " else "The calendar could not be read: ") .. (if (ev.error or "") ~= "" then ev.error else "HTTP " .. tostring(ev.status))
		end
		merge()
		waiting = math.max(0, waiting - 1)
		if waiting == 0 then
			fetching = false
		end
		local n = #links(ev)
		out.note = if problem then problem elseif n > 1 then #events .. " events read from " .. n .. " calendars." else #events .. " events read."
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
		elseif a == "wprev" or a == "wnext" then
			go_to(sel + (if a == "wprev" then -7 else 7), tz)
		elseif a == "view:month" or a == "view:day" then
			view = string.sub(a, 6)
		elseif string.match(a, "^join:") then
			local uid = string.sub(a, 6)
			for _, x in events do
				if x.uid == uid and (x.link or "") ~= "" then
					out.open = x.link
					break
				end
			end
		elseif string.match(a, "^d:") then
			local n = tonumber(string.sub(a, 3))
			if n then
				go_to(n, tz)
				view = "day"
			end
		elseif a == "open" then
			go_to(today_of(now, tz), tz)
			view = "day"
		elseif a == "say_time" then
			out.say = { chime(ev, false) }
		end
	elseif e == "tick" then
		local url = ev.settings.url or ""
		local every = math.max(1, ev.settings.poll_min or 10) * 60000
		if url ~= url_seen then
			url_seen = url
			events, problem, by_cal, problems = {}, nil, {}, {}
			ask(ev, out)
		elseif not fetching and (last_fetch == nil or now - last_fetch >= every) then
			ask(ev, out)
		end
	else
		return nil
	end
	-- the time, at the marks of the clock it was asked to say it at
	local every = ev.settings.chime or 0
	if every > 0 and e ~= "action" then
		local t = nadi.time(now, tz)
		local minute = t.hour * 60 + t.minute
		local mark = t.days .. ":" .. minute
		if minute % every == 0 and chimed ~= mark then
			chimed = mark
			out.say = out.say or {}
			table.insert(out.say, chime(ev, true))
		end
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
