local T = 1791559845000 - 45000 -- Fri 2026-10-09 15:30:00 UTC
local DAY = 86400000

local function ev(kind: string, now: number, more: any?)
	local e: any = { event = kind, now = now, tz_offset = 0, settings = { url = "webcal://example.com/cal.ics", h24 = false, remind = true, lead_min = 15, poll_min = 10 } }
	for k, v in more or {} do
		e[k] = v
	end
	return e
end

local FEED = table.concat({
	"BEGIN:VCALENDAR",
	"BEGIN:VEVENT",
	"UID:a",
	"SUMMARY:Standup",
	"DTSTART:20261009T154000Z",
	"DTEND:20261009T160000Z",
	"END:VEVENT",
	"BEGIN:VEVENT",
	"UID:b",
	"SUMMARY:Holiday",
	"DTSTART;VALUE=DATE:20261012",
	"DTEND;VALUE=DATE:20261013",
	"END:VEVENT",
	"END:VCALENDAR",
}, "\r\n")

tests = {}

function tests.it_reads_its_link_and_reminds_once()
	local r = handle(ev("start", T))
	eq(r.fetch[1].url, "https://example.com/cal.ics", "webcal is https")
	eq(r.fetch[1].max_kb, 8000, "a big feed")
	r = handle(ev("fetched", T, { id = "ics", status = 200, body = FEED }))
	eq(r.note, "2 events read.", "read")
	eq(r.say[1].text, "Standup", "reminded")
	eq(r.say[1].pill.right, "in 10 min", "when")
	check(handle(ev("tick", T + 1000)).say == nil, "once")
	local w = r.widgets.calendar
	eq(w.tip, "Standup\nFri, Oct 9, 3:40 PM", "what is next")
end

function tests.the_month_and_a_day()
	handle(ev("start", T))
	handle(ev("fetched", T, { id = "ics", status = 200, body = FEED }))
	local p = handle(ev("action", T, { action = "today" })).widgets.calendar.panel
	eq(p.body[1].title, "October 2026", "month")
	local grid = p.body[2]
	eq(grid.labels[1], "S", "Sunday first")
	-- (October 1st 2026 is a Thursday: four days of September first)
	eq(grid.cells[5].t, "1", "the 1st")
	eq(grid.cells[1].state, "other", "September")
	eq(grid.cells[13].state, "sel", "today, picked")
	eq(p.body[4].rows[1], { title = "Standup", time = "3:40 PM – 4:00 PM", past = false }, "the day's event")
	local day = (T // DAY) + 3
	p = handle(ev("action", T, { action = "d:" .. day })).widgets.calendar.panel
	eq(p.body[4].rows[1].time, "All day", "holiday")
	p = handle(ev("action", T, { action = "next" })).widgets.calendar.panel
	eq(p.body[1].title, "November 2026", "next month")
end

function tests.without_a_link_it_says_where_to_put_one()
	local e = ev("start", T)
	e.settings.url = ""
	local r = handle(e)
	check(r.fetch == nil, "nothing to read")
	check(string.find(r.widgets.calendar.panel.body[3].text, "No calendar link", 1, true) ~= nil, "says so")
end

function tests.it_says_the_time_at_the_marks_when_asked()
	handle(ev("start", T))
	local function at(now: number, every: number)
		local e = ev("tick", now)
		e.settings.chime = every
		local r = handle(e)
		return (r and r.say) or {}
	end
	eq(#at(T, 0), 0, "never, by default")
	local s = at(T, 30)
	eq(s[1].text, "3:30 PM", "at :30")
	eq(s[1].pill.right, "Fri, Oct 9", "the date")
	eq(s[1].ambient, true, "it can wait")
	eq(#at(T + 5000, 30), 0, "once")
	eq(#at(T + 60000, 30), 0, "3:31 is no mark")
	local r = handle(ev("action", T, { action = "say_time" }))
	eq(r.say[1].text, "3:30 PM", "now, when pressed")
	eq(r.say[1].ambient, false, "pressed: at once")
end
