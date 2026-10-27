tests = {}

function tests.a_toast_is_a_banner_that_opens_its_app()
	local r = handle({ event = "notifications", now = 0, settings = {}, notifications = { { app = "Discord", title = "ann", body = "hi there" }, { app = "Mail", title = "", body = "Inbox" } } })
	eq(r.say[1], { text = "hi there", title = "ann", priority = 0, open = "discord://", group = "Discord" }, "discord")
	eq(r.say[2].title, "Mail", "app as title")
	eq(r.say[2].text, "Inbox", "body")
	eq(r.widgets.inbox.peek.dot, true, "the bell shows they came")
	eq(#r.widgets.inbox.panel.body[1].rows, 2, "listed")
	eq(r.widgets.inbox.panel.body[1].rows[1].title, "Mail", "newest first")
	local o = handle({ event = "open", now = 0, settings = {}, widget = "inbox", open = true })
	eq(o.widgets.inbox.peek.dot, nil, "seen once opened")
end

function tests.the_bell_is_there_from_the_start()
	local r = handle({ event = "start", now = 0, settings = {} })
	eq(r.widgets.inbox.peek.icon, "bell", "bell")
	eq(r.widgets.inbox.panel.body[1].text, "Nothing lately", "empty")
end

function tests.vibers_popup_is_noticed_once_it_comes()
	local function look(ids: { number })
		local windows = {}
		for _, id in ids do
			table.insert(windows, { exe = "viber.exe", title = "ViberPC", w = 300, id = id })
		end
		return handle({ event = "processes", now = 0, settings = {}, windows = windows })
	end
	handle({ event = "start", now = 0, settings = {} })
	check(look({ 1 }) == nil, "there at the start: not news")
	eq(look({ 1, 2 }).say[1].title, "Viber", "a new one")
	check(look({ 2 }) == nil, "the same")
end
