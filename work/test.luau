local function win(path: string, title: string, front: boolean)
	return { exe = string.lower(string.match(path, "([^\\]+)$") or ""), path = path, name = "", title = title, front = front, tool = false }
end

local function look(now: number, windows, idle: number, open: boolean?)
	return handle({ event = "processes", now = now, tz_offset = 0, settings = {}, windows = windows, idle_s = idle, open = { work = open or false } })
end

local function code(title: string?)
	return { win("C:\\x\\Code.exe", title or "main.rs - proj - Visual Studio Code", true) }
end

local function find(body, kind: string)
	for _, b in body do
		if b.kind == kind then
			return b
		end
	end
	return nil
end

-- 2026-11-02, a Monday, at noon
local MON = 1793620800000

tests = {}

function tests.time_counts_only_while_you_are_at_it()
	handle({ event = "start", now = 0, settings = {} })
	look(MON, code(), 0)
	look(MON + 2000, code(), 0)
	local w = look(MON + 4000, code(), 0).widgets.work
	eq(w.peek.text, "<1m", "counted")
	check(w.peek.ring.theme, "coding now")
	-- (the keyboard left alone: nothing more)
	local r = look(MON + 600000, code(), 300)
	eq(r.widgets.work.peek.ring.theme, false, "not now")
end

function tests.a_gap_is_not_work()
	handle({ event = "start", now = 0, settings = {} })
	look(MON, code(), 0)
	local r = look(MON + 3600000, code(), 0)
	eq(r.widgets.work.peek.text, "<1m", "an hour apart: six seconds at most")
end

function tests.filed_by_project_language_and_file()
	handle({ event = "start", now = 0, settings = {} })
	for i = 0, 40 do
		look(MON + i * 2000, code("● lib.rs - nadi - Visual Studio Code"), 0)
	end
	local w = look(MON + 82000, code("Settings - nadi - Visual Studio Code"), 0, true).widgets.work
	local body = w.panel.body
	local ranks = find(body, "ranks")
	eq(ranks.rows[1].title, "nadi", "the project")
	eq(ranks.rows[1].action, "open:nadi", "pressed: its editor")
	local split = find(body, "split")
	eq(string.sub(split.parts[1].label, 1, 4), "Rust", "the language")
	local card = find(body, "card")
	eq(card.body[1].text, "lib.rs ●", "a tab that is no file keeps the last file")
end

function tests.pages_for_coding_are_research()
	handle({ event = "start", now = 0, settings = {} })
	look(MON, code("main.rs - nadi - Visual Studio Code"), 0)
	look(MON + 2000, code("main.rs - nadi - Visual Studio Code"), 0)
	local page = { win("C:\\x\\chrome.exe", "c# - How do I read a surface? - Stack Overflow - Google Chrome", true) }
	for i = 1, 40 do
		look(MON + 2000 + i * 2000, page, 0)
	end
	local other = { win("C:\\x\\chrome.exe", "Some news - Google Chrome", true) }
	look(MON + 90000, other, 0)
	local r = look(MON + 92000, other, 0, true)
	local body = r.widgets.work.panel.body
	check(find(body, "goal").extra > 0, "research toward the goal")
	local sites = nil
	for _, b in body do
		if b.kind == "ranks" and b.rows[1].title == "Stack Overflow" then
			sites = b
		end
	end
	check(sites ~= nil, "the site")
	eq(find(body, "ranks").rows[1].title, "nadi", "filed under the project coded in last")
end

function tests.site_names()
	local function counts(title: string): boolean
		handle({ event = "start", now = 0, settings = {} })
		local page = { win("C:\\x\\msedge.exe", title, true) }
		local function at(now: number)
			return handle({ event = "processes", now = now, tz_offset = 0, settings = { sites = "Godot Forum" }, windows = page, idle_s = 0, open = { work = false } })
		end
		at(MON)
		return at(MON + 3000).widgets.work.peek ~= nil
	end
	check(counts("Fix the build · Pull Request #48 · owner/repo · GitHub"), "GitHub")
	check(counts("Array.prototype.map() - JavaScript | MDN"), "MDN")
	check(counts("HashMap in std::collections - Rust"), "docs.rs")
	check(counts("os.path — Python 3.12.1 documentation"), "docs")
	check(counts("Signals - Godot Forum"), "one of your own")
	check(not counts("The best pasta - Google Chrome"), "not research")
end

function tests.git_is_asked_while_the_panel_is_open()
	handle({ event = "start", now = 0, settings = {} })
	handle({ event = "file", now = 0, settings = {}, path = "storage.json", text = nadi.json.encode({ windowsState = { lastActiveWindow = { folder = "file:///c%3A/Workspace/proj" } } }) })
	check(look(MON, code(), 0).run == nil, "shut: not asked")
	local r = look(MON + 7000, code(), 0, true)
	eq(#r.run, 3, "three")
	eq(r.run[1].line[3], "c:\\Workspace\\proj", "the folder")
	check(look(MON + 9000, code(), 0, true).run == nil, "not again at once")
	local function cmd(id: string, c: number, out: string)
		return handle({ event = "command", now = MON + 9500, tz_offset = 0, settings = {}, id = id, code = c, stdout = out, open = { work = true } })
	end
	cmd("branch", 0, "main\n")
	cmd("diff", 0, "12\t3\tsrc/a.rs\n-\t-\timg/x.png")
	local w = cmd("untracked", 0, "notes.md").widgets.work
	local card = find(w.panel.body, "card")
	local chips = card.body[2].items
	eq(chips[1].text, "main", "branch")
	eq(chips[2].text, "+12 −3", "changes")
	eq(chips[3].text, "3", "files")
end

function tests.week_and_month()
	handle({ event = "start", now = 0, settings = {}, store = { days = { ["2026-10-30"] = { code = 5 * 3600, research = 0, projects = { nadi = 5 * 3600 }, languages = { Rust = 5 * 3600 }, files = {}, sites = {}, hours = { ["10"] = 3600 } } } } })
	look(MON, code(), 0)
	look(MON + 2000, code(), 0)
	local function act(a: string)
		return handle({ event = "action", now = MON + 3000, tz_offset = 0, settings = {}, action = a }).widgets.work.panel.body
	end
	eq(#find(act("span:week"), "columns").items, 7, "a week")
	eq(find(act("back"), "columns").items[5].mark, "second", "last week's best day")
	local cal = find(act("span:month"), "calendar")
	eq(cal.lead, 6, "November 2026 starts on a Sunday")
	eq(#cal.days, 30, "thirty days")
	check(cal.days[2].today, "today")
	check(cal.days[3].future, "to come")
	cal = find(act("back"), "calendar")
	check(cal.days[30].ring, "October 30 met its goal")
	eq(find(act(cal.days[30].action), "goal").text, "5h", "that day")
end

function tests.what_is_kept_comes_back()
	handle({ event = "start", now = 0, settings = {}, store = { days = { ["2026-11-02"] = { code = 3600, research = 0, projects = {}, languages = {}, files = {}, sites = {}, hours = {} } } } })
	local r = look(MON, code(), 0)
	eq(r.widgets.work.peek.text, "1h", "kept")
end
