-- The tests of Time: `nadi-plugin test time`.

local AT = 1791559845000 - 45000 -- Fri 2026-10-09 15:30:00 UTC

local function ev(kind: string, now: number, tz: number, every: number)
	return { event = kind, now = now, tz_offset = tz, settings = { every = every, h24 = false } }
end

local function said(r)
	return (r and r.say) or {}
end

tests = {}

function tests.says_the_time_at_a_mark_once()
	eq(#said(handle(ev("tick", AT + 60000, 0, 60))), 0, "15:31 is not a mark")
	local s = said(handle(ev("tick", AT, 0, 30)))
	eq(s[1].text, "3:30 PM", "text")
	eq(s[1].pill.right, "Fri, Oct 9", "date")
	eq(s[1].pill.look, "big", "big")
	eq(s[1].ambient, true, "ambient")
	eq(#said(handle(ev("tick", AT + 5000, 0, 30))), 0, "once")
	eq(handle(ev("tick", AT + 61000, 0, 30)).wake_ms, 59020, "woken at the next minute")
end

function tests.a_button_is_answered_and_not_ambient()
	local r = handle({ event = "action", action = "preview", now = AT, tz_offset = 0, settings = { h24 = true } })
	eq(said(r)[1].text, "15:30", "24-hour")
	eq(said(r)[1].ambient, false, "not ambient")
end

function tests.the_hour_is_the_hour_of_the_pc()
	eq(said(handle(ev("tick", AT, 540, 30)))[1].text, "12:30 AM", "Tokyo")
end

function tests.midnight_and_noon()
	eq(said(handle(ev("tick", AT - 15 * 3600000 - 30 * 60000, 0, 60)))[1].text, "12:00 AM", "midnight")
	eq(said(handle(ev("tick", AT - 3 * 3600000, 0, 30)))[1].text, "12:30 PM", "noon")
end
