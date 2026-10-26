--!strict
-- Time: every so often a small banner says what time it is. It lines up with the clock (every 30 minutes means :00 and :30), so
-- it never counts: it asks to be woken at the next minute, and speaks when that minute is one of the marks. It sees nothing but
-- the time. "Show it now" in its settings says it at once.

local last: string? = nil -- the day and minute of the last mark said: one each

local function clock(t, h24: boolean): string
	if h24 then
		return string.format("%02d:%02d", t.hour, t.minute)
	end
	local h = t.hour % 12
	if h == 0 then
		h = 12
	end
	return string.format("%d:%02d %s", h, t.minute, if t.hour < 12 then "AM" else "PM")
end

local function date(t): string
	return t.wday .. ", " .. t.mon .. " " .. t.day
end

local function say(t, h24: boolean, ambient: boolean)
	return { text = clock(t, h24), priority = 10, ambient = ambient, pill = { icon = "clock", right = date(t), look = "big", ms = 4000 } }
end

function handle(ev)
	local t = nadi.time(ev.now, ev.tz_offset or 0)
	local h24 = ev.settings.h24 == true
	if ev.event == "action" then
		return if ev.action == "preview" then { say = { say(t, h24, false) } } else nil
	end
	local every = math.max(1, ev.settings.every or 60)
	local minute = t.hour * 60 + t.minute
	local out: any = { wake_ms = 60000 - ev.now % 60000 + 20 }
	local mark = t.days .. ":" .. minute
	if minute % every == 0 and last ~= mark then
		last = mark
		out.say = { say(t, h24, true) }
	end
	return out
end
