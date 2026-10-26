local function win(path: string, title: string, front: boolean)
	return { exe = string.lower(string.match(path, "([^\\]+)$") or ""), path = path, name = "", title = title, front = front, tool = false }
end

local function look(now: number, windows, idle: number, open: boolean?)
	return handle({ event = "processes", now = now, tz_offset = 0, settings = {}, windows = windows, idle_s = idle, open = { work = open or false } })
end

local function code()
	return { win("C:\\x\\Code.exe", "main.rs - proj - Visual Studio Code", true) }
end

tests = {}

function tests.a_session_is_confirmed_after_the_dwell_and_left_after_the_grace()
	handle({ event = "start", now = 0, settings = {} })
	local w = look(10000, code(), 0).widgets.work
	eq(w.panel.title, "Work", "not yet a session")
	look(14000, code(), 0)
	w = look(19000, code(), 0).widgets.work
	eq(w.panel.title, "Coding", "a session")
	eq(w.panel.body[1].title, "proj", "the project")
	eq(w.panel.body[1].body[1].text, "Rust · main.rs", "the file")
	eq(w.peek.image, "exe:C:\\x\\Code.exe", "its icon, by name")
	eq(look(60000, code(), 200).widgets.work.panel.title, "Coding", "a glance away does not end it")
	look(100000, code(), 200)
	eq(look(110000, code(), 200).widgets.work.panel.title, "Work", "ended")
end

function tests.a_browser_is_no_session_and_the_time_is_kept()
	handle({ event = "start", now = 0, settings = {} })
	local w = function()
		return { win("C:\\x\\chrome.exe", "Some page", true) }
	end
	look(1000, w(), 0)
	look(3000, w(), 0)
	eq(look(40000, w(), 0).widgets.work, {}, "browsing is no session")
	local r = look(80000, w(), 0)
	local found = false
	for _, d in r.store.days do
		if (d.categories.browsing or 0) > 10 then
			found = true
		end
	end
	check(found, "kept")
end

function tests.titles_are_read()
	handle({ event = "start", now = 0, settings = {} })
	look(1000, { win("C:\\x\\Code.exe", "● lib.rs - nadi - Visual Studio Code", true) }, 0)
	local w = look(10000, { win("C:\\x\\Code.exe", "Settings - nadi - Visual Studio Code", true) }, 0).widgets.work
	eq(w.panel.body[1].body[1].text, "Rust · lib.rs ●", "a tab that is no file keeps the last file")
end

function tests.git_is_asked_while_the_panel_is_open()
	handle({ event = "start", now = 0, settings = {} })
	handle({ event = "file", now = 0, settings = {}, path = "storage.json", text = nadi.json.encode({ windowsState = { lastActiveWindow = { folder = "file:///c%3A/Workspace/proj" } } }) })
	check(look(1000, code(), 0).run == nil, "shut: not asked")
	local r = look(7000, code(), 0, true)
	eq(#r.run, 3, "three")
	eq(r.run[1].line[3], "c:\\Workspace\\proj", "the folder")
	check(look(9000, code(), 0, true).run == nil, "not again at once")
	local function cmd(id: string, c: number, out: string)
		return handle({ event = "command", now = 9500, tz_offset = 0, settings = {}, id = id, code = c, stdout = out })
	end
	cmd("branch", 0, "main\n")
	cmd("diff", 0, "12\t3\tsrc/a.rs\n-\t-\timg/x.png")
	local w = cmd("untracked", 0, "notes.md").widgets.work
	local s = w.panel.body[1]
	eq(s.body[2].items[1].text, "main", "branch")
	eq(s.body[2].items[2].text, "+12 −3 · 3 files", "changes")
	eq(s.body[3].rows[1], { title = "a.rs", sub = "+12 −3", progress = 1 }, "the biggest first")
	eq(s.body[3].rows[3].sub, "new", "new file")
end

function tests.what_is_kept_comes_back()
	handle({ event = "start", now = 0, settings = {}, store = { days = { ["2026-10-10"] = { categories = { coding = 3600 } } } } })
	local r = look(1791676800000 - 86400000 + 1000, code(), 0)
	check(r.widgets.work.panel.sub ~= nil, "drawn")
end
