--!strict
-- Work: time coded, as WakaTime counts it, kept on this PC. Time counts only while you are at it: an editor (or a terminal) in front
-- and the keyboard or the mouse used in the last two minutes. Each moment is filed under the project (the editor's folder, from its
-- window title), the language (the file's extension), the file, and the hour. Pages read for coding (a browser in front on GitHub,
-- Stack Overflow, docs...) count as research: toward the goal, filed under the project you last coded in (or the one the page names).
--
-- At rest: today against the daily goal, in a ring. Opened: a day, a week or a month: the goal, the streak, the days as columns split
-- by project, the projects, the languages, the sites, when you coded, and (today) the file you are in with its git state.
--
-- It is told every two seconds which windows are open (`processes`) and how long the keyboard and the mouse have been left alone
-- (`idle_s`), keeps 70 days in its store, finds the folders of the editors' projects in their `storage.json` (`files`), and asks `git`
-- about the project in front (`run`) while its panel is open.

local ACTIVE_S = 120 -- the keyboard or the mouse used this recently: you are at it
local STEP_MAX_S = 6 -- one look never counts more than this (a gap is not work)
local RESEARCH_LINK_MS = 15 * 60000 -- research goes to the project coded in this recently
local KEEP_DAYS = 70
local FLUSH_MS = 30000
local GIT_EVERY_MS = 6000
local MAX_FILES = 3
local DAY_MS = 86400000

local LANGUAGE_EXT = {
	py = "Python", pyw = "Python", js = "JavaScript", jsx = "JavaScript", mjs = "JavaScript", cjs = "JavaScript",
	ts = "TypeScript", tsx = "TypeScript", java = "Java", kt = "Kotlin", kts = "Kotlin", c = "C", h = "C",
	cpp = "C++", cc = "C++", cxx = "C++", hpp = "C++", hh = "C++", cs = "C#", go = "Go", rs = "Rust", rb = "Ruby",
	php = "PHP", swift = "Swift", m = "Objective-C", mm = "Objective-C", scala = "Scala", dart = "Dart", lua = "Lua",
	luau = "Luau", r = "R", jl = "Julia", hs = "Haskell", ex = "Elixir", exs = "Elixir", erl = "Erlang", clj = "Clojure",
	sql = "SQL", html = "HTML", htm = "HTML", css = "CSS", scss = "SCSS", sass = "Sass", less = "Less", json = "JSON",
	yaml = "YAML", yml = "YAML", toml = "TOML", xml = "XML", md = "Markdown", sh = "Shell", bash = "Shell", zsh = "Shell",
	ps1 = "PowerShell", bat = "Batch", vue = "Vue", svelte = "Svelte", proto = "Protobuf", graphql = "GraphQL",
	gql = "GraphQL", ipynb = "Jupyter Notebook", gd = "GDScript", zig = "Zig", nim = "Nim", fs = "F#", vb = "Visual Basic",
}

local LANGUAGE_COLOR = {
	Rust = "#d97757", Luau = "#3b82f6", Lua = "#5a6bd8", Python = "#4b8bbe", JavaScript = "#e5c07b", TypeScript = "#3178c6",
	HTML = "#e34c26", CSS = "#a77bf3", SCSS = "#c6538c", ["C#"] = "#3fae49", ["C++"] = "#f34b7d", C = "#8e8e96", Go = "#00add8",
	Java = "#b07219", Kotlin = "#a97bff", Ruby = "#cc342d", PHP = "#777bb4", Swift = "#f05138", Dart = "#00b4ab", Shell = "#89e051",
	PowerShell = "#3d6fb8", Markdown = "#9a9aa4", JSON = "#9a9aa4", TOML = "#9c4221", YAML = "#cb171e", Vue = "#41b883",
	Svelte = "#ff3e00", SQL = "#e8b04a", GDScript = "#478cbf", Zig = "#ec915c",
}

-- the projects' colours, by their rank in what is shown (the first in the theme's own)
local PALETTE = { "theme", "#4f8ff0", "#a77bf3", "#e8b04a", "#e86a9a", "#5ac8e6" }
local OTHER = "#4a4a52"

-- window-title parts that are the editor's own name, not a project
local EDITOR_NAMES = {
	["visual studio code"] = true, ["visual studio code - insiders"] = true, code = true, ["vs code"] = true, cursor = true,
	["sublime text"] = true, ["notepad++"] = true, ["android studio"] = true, pycharm = true, ["intellij idea"] = true,
	webstorm = true, clion = true, rider = true, goland = true, phpstorm = true, vim = true, neovim = true, zed = true,
	windsurf = true, vscodium = true, ["microsoft visual studio"] = true, ["visual studio"] = true,
}

local EDITORS = {
	code = true, ["code - insiders"] = true, codium = true, devenv = true, pycharm64 = true, idea64 = true, clion64 = true,
	webstorm64 = true, rider64 = true, goland64 = true, phpstorm64 = true, sublime_text = true, ["notepad++"] = true, vim = true,
	nvim = true, cursor = true, androidstudio64 = true, zed = true, windsurf = true, rustrover64 = true, fleet = true,
}
local TERMINALS = { windowsterminal = true, cmd = true, powershell = true, pwsh = true, wt = true, ["git-bash"] = true, mintty = true, alacritty = true, wezterm = true }
local BROWSERS = { chrome = true, msedge = true, firefox = true, brave = true, opera = true, vivaldi = true, arc = true }

-- sites for coders, by how their pages' titles name them: the first or the last part of the title
local SITES = {
	{ "github", "GitHub" }, { "gitlab", "GitLab" }, { "bitbucket", "Bitbucket" }, { "stack overflow", "Stack Overflow" },
	{ "stack exchange", "Stack Exchange" }, { "super user", "Super User" }, { "mdn", "MDN" }, { "mdn web docs", "MDN" },
	{ "docs.rs", "docs.rs" }, { "crates.io", "crates.io" }, { "npm", "npm" }, { "pypi", "PyPI" }, { "read the docs", "Read the Docs" },
	{ "microsoft learn", "Microsoft Learn" }, { "luau", "Luau" }, { "cppreference.com", "cppreference" }, { "devdocs", "DevDocs" },
	{ "rust", "Rust Docs" }, { "the rust programming language", "Rust Docs" }, { "localhost", "localhost" }, { "codeberg", "Codeberg" },
	{ "hacker news", "Hacker News" }, { "leetcode", "LeetCode" }, { "regex101", "regex101" }, { "godot docs", "Godot Docs" },
}
local SITE_ICON = { GitHub = "branch", GitLab = "branch", Bitbucket = "branch", Codeberg = "branch", ["Stack Overflow"] = "ask", ["Stack Exchange"] = "ask", ["Super User"] = "ask", localhost = "term" }

-- ------------------------------------------------------------------------------------------------------ reading titles

local function trim_marks(s: string): string
	s = string.gsub(s, "^[%s●•∙]+", "")
	s = string.gsub(s, "[%s●•∙]+$", "")
	return s
end

local function parts_of(title: string): { string }
	local t = string.gsub(string.gsub(string.gsub(string.gsub(title, " — ", " - "), " – ", " - "), " · ", " - "), " | ", " - ")
	local out = {}
	local start = 1
	while true do
		local i, j = string.find(t, " - ", start, true)
		table.insert(out, string.sub(t, start, (i or (#t + 1)) - 1))
		if not i then
			break
		end
		start = (j :: number) + 1
	end
	return out
end

local function language_of(file: string): string?
	local ext = string.match(file, "%.([^%.]+)$")
	return if ext then LANGUAGE_EXT[string.lower(ext)] else nil
end

-- (project, file, unsaved) from an editor's window title: "● file.ext - project - App Name"
local function editor_title(title: string): (string?, string?, boolean)
	local parts = {}
	local unsaved = false
	for i, p in parts_of(title) do
		if i == 1 and string.find(p, "^%s*[●•∙]") then
			unsaved = true
		end
		local x = trim_marks(p)
		if x ~= "" and not EDITOR_NAMES[string.lower(x)] then
			table.insert(parts, x)
		end
	end
	local first = parts[1]
	if not first then
		return nil, nil, false
	end
	local lower = string.lower(first)
	local stem, ext = string.match(first, "^(.+)%.([%w]+)$")
	local looks = (utf8.len(first) or 0) <= 80 and string.sub(first, -3) ~= "…"
		and ((stem ~= nil and #ext >= 1 and #ext <= 8) or lower == "makefile" or lower == "dockerfile" or lower == "license" or lower == "readme")
	local file = if looks then first else nil
	local project = if #parts >= 2 then parts[#parts] elseif not looks then first else nil
	-- ("[Administrator]", "(Workspace)": marks, not names)
	if project then
		project = string.gsub(project, "%s*%b[]$", "")
		project = string.gsub(project, "%s*%(Workspace%)$", "")
	end
	return project, file, unsaved
end

local function stem(path: string): string
	local f = string.lower(string.match(path, "([^\\/]+)$") or path)
	return (string.gsub(f, "%.exe$", ""))
end

-- a page read for coding: the site its title names (its first or its last part), or nil
local BROWSER_PARTS = {
	["google chrome"] = true, ["microsoft edge"] = true, ["microsoft\u{200b} edge"] = true, ["mozilla firefox"] = true, brave = true,
	opera = true, vivaldi = true, arc = true, personal = true, work = true, school = true, default = true, guest = true,
}

local function site_of(title: string, extra: { string }): string?
	local parts = parts_of(title)
	-- (the browser's own name and its profile's at the end: not the page's)
	while #parts > 1 do
		local last = string.lower(trim_marks(parts[#parts]))
		if BROWSER_PARTS[last] or string.sub(last, 1, 8) == "profile " or string.find(last, "^and %d+ more") then
			table.remove(parts)
		else
			break
		end
	end
	local ends = { string.lower(trim_marks(parts[1] or "")), string.lower(trim_marks(parts[#parts] or "")) }
	local lower = string.lower(title)
	for _, e in ends do
		for _, s in SITES do
			if e == s[1] or string.sub(e, 1, #s[1] + 1) == s[1] .. " " or string.sub(e, 1, #s[1] + 1) == s[1] .. ":" then
				return s[2]
			end
		end
		if string.find(e, "stack exchange$") then
			return "Stack Exchange"
		end
		if string.find(e, "documentation") then
			return "Docs"
		end
	end
	if string.find(lower, "localhost:%d") or string.find(lower, "127%.0%.0%.1") then
		return "localhost"
	end
	for _, name in extra do
		if name ~= "" and string.find(lower, string.lower(name), 1, true) then
			return name
		end
	end
	return nil
end

local function duration(secs: number): string
	local s = math.floor(secs)
	local h, m = s // 3600, (s % 3600) // 60
	if h > 0 then
		return if m > 0 then h .. "h " .. m .. "m" else h .. "h"
	elseif m > 0 then
		return m .. "m"
	end
	return if s > 0 then "<1m" else "0m"
end

-- file:///c%3A/Workspace/x -> C:\Workspace\x
local function uri_to_path(uri: string): string?
	local rest = string.match(uri, "^file:///(.*)$")
	if not rest then
		return nil
	end
	rest = string.gsub(rest, "%%(%x%x)", function(h)
		return string.char(tonumber(h, 16) :: number)
	end)
	return (string.gsub(rest, "/", "\\"))
end

-- the folders an editor's `storage.json` knows about
local function editor_folders(text: string): { string }
	local v = nadi.json.decode(text)
	if type(v) ~= "table" then
		return {}
	end
	local uris = {}
	local ws = v.windowsState or {}
	if ws.lastActiveWindow and type(ws.lastActiveWindow.folder) == "string" then
		table.insert(uris, ws.lastActiveWindow.folder)
	end
	for _, w in ws.openedWindows or {} do
		if type(w.folder) == "string" then
			table.insert(uris, w.folder)
		end
	end
	for k in (v.profileAssociations or {}).workspaces or {} do
		table.insert(uris, k)
	end
	local out = {}
	for _, u in uris do
		local p = uri_to_path(u)
		if p then
			table.insert(out, p)
		end
	end
	return out
end

-- ----------------------------------------------------------------------------------------------------------- git state

local function project_info(branch: string, diff: string, untracked: string)
	local added, deleted, changed = 0, 0, 0
	for line in string.gmatch(diff, "[^\n]+") do
		local a, d = string.match(line, "^(%S+)\t(%S+)\t")
		if a then
			changed += 1
			added += tonumber(a) or 0
			deleted += tonumber(d) or 0
		end
	end
	for line in string.gmatch(untracked, "[^\n]+") do
		if string.match(line, "%S") then
			changed += 1
		end
	end
	return { branch = branch, changed = changed, added = added, deleted = deleted }
end

-- ---------------------------------------------------------------------------------------------------------------- state

local stats: any = { days = {} } -- { days = { ["2026-10-11"] = { code, research, projects, languages, files, sites, hours } } }
local dirty = false
local last_flush = 0
local last_tick = 0
local now_project: string? = nil -- the project coded in last, and when
local now_project_at = 0
local now_file: string? = nil
local now_unsaved = false
local coding_now = false
local editor_of = {} -- project -> the editor's program it was last coded in
local folders = {} -- by the file they came from
local git: any = { at = 0, pending = 0 }
local project: any = nil
local span = "day" -- what the panel shows: "day", "week" or "month"
local shift = 0 -- days, weeks or months back
local bumped_for = ""

local function day_key(ms: number, tz: number): string
	local t = nadi.time(ms, tz)
	return string.format("%04d-%02d-%02d", t.year, t.month, t.day)
end

local function day_of(day: string)
	local d = stats.days[day]
	if not d or d.code == nil then
		d = { code = 0, research = 0, projects = {}, languages = {}, files = {}, sites = {}, hours = {} }
		stats.days[day] = d
	end
	return d
end

local function add(t: any, key: string, secs: number)
	t[key] = (t[key] or 0) + secs
end

local function total(d): number
	return if d then (d.code or 0) + (d.research or 0) else 0
end

local function kept()
	local keys = {}
	for k in stats.days do
		table.insert(keys, k)
	end
	table.sort(keys)
	for i = 1, #keys - KEEP_DAYS do
		stats.days[keys[i]] = nil
	end
	return stats
end

local function project_dir(name: string): string?
	local lname = string.lower(name)
	for _, list in folders do
		for _, p in list do
			if string.lower(string.match(p, "([^\\]+)$") or "") == lname then
				return p
			end
		end
	end
	return nil
end

local function extra_sites(settings): { string }
	local out = {}
	for name in string.gmatch(tostring(settings.sites or ""), "[^,]+") do
		local n = string.match(name, "^%s*(.-)%s*$") or ""
		if n ~= "" then
			table.insert(out, n)
		end
	end
	return out
end

-- a page that names a project (a GitHub page of owner/not-another-dynamic-island): that project
local function project_named(title: string, now: number, tz: number): string?
	local lower = string.lower(title)
	local best: string? = nil
	for i = 0, 13 do
		local d = stats.days[day_key(now - i * DAY_MS, tz)]
		for name in (d and d.projects) or {} do
			if #name >= 4 and string.find(lower, string.lower(name), 1, true) and (not best or #name > #(best :: string)) then
				best = name
			end
		end
	end
	return best
end

-- ------------------------------------------------------------------------------------------------------------ counting

local function count(ev)
	local now, tz = ev.now, ev.tz_offset or 0
	local step = if last_tick == 0 then 0 else math.min(STEP_MAX_S, (now - last_tick) / 1000)
	last_tick = now
	coding_now = false
	if (ev.idle_s or 0) >= ACTIVE_S then
		return
	end
	local front = nil
	for _, w in ev.windows or {} do
		if w.front and not w.tool then
			front = w
			break
		end
	end
	if not front then
		return
	end
	local exe = stem(front.path or "")
	local title = front.title or ""
	local day = day_key(now, tz)
	local hour = tostring(nadi.time(now, tz).hour)
	if EDITORS[exe] or TERMINALS[exe] then
		local proj, file, unsaved = nil, nil, false
		if EDITORS[exe] then
			proj, file, unsaved = editor_title(title)
		end
		-- (a terminal, or an editor's tab that is no file: the project coded in last, when it was lately)
		if not proj and now_project and now - now_project_at <= RESEARCH_LINK_MS then
			proj = now_project
		end
		if file then
			now_file, now_unsaved = file, unsaved
		end
		if proj then
			now_project, now_project_at = proj, now
			editor_of[proj] = front.path or ""
		end
		coding_now = true
		if step > 0 then
			local d = day_of(day)
			d.code += step
			add(d.hours, hour, step)
			if proj then
				add(d.projects, proj, step)
			end
			local language = if file then language_of(file) elseif TERMINALS[exe] then "Shell" else nil
			add(d.languages, language or "Other", step)
			if file then
				add(d.files, file, step)
			end
			dirty = true
		end
	elseif BROWSERS[exe] then
		local site = site_of(title, extra_sites(ev.settings or {}))
		if site and step > 0 then
			local d = day_of(day)
			d.research += step
			add(d.hours, hour, step)
			add(d.sites, site, step)
			local proj = project_named(title, now, tz) or (if now_project and now - now_project_at <= RESEARCH_LINK_MS then now_project else nil)
			if proj then
				add(d.projects, proj, step)
			end
			dirty = true
		end
	end
end

-- ------------------------------------------------------------------------------------------------------------- reading the days

local function goal_s(settings): number
	return math.max(0.25, tonumber(settings.goal_h) or 4) * 3600
end

local function counted(weekday: number, settings): boolean
	return settings.streak_days == "every" or weekday < 5
end

-- the days (keys, oldest first) from `from` for `n` days
local function days_from(from_ms: number, n: number, tz: number): { { key: string, ms: number, t: any } }
	local out = {}
	for i = 0, n - 1 do
		local ms = from_ms + i * DAY_MS
		table.insert(out, { key = day_key(ms, tz), ms = ms, t = nadi.time(ms, tz) })
	end
	return out
end

-- noon of the day `ms` falls on
local function noon(ms: number, tz: number): number
	local t = nadi.time(ms, tz)
	return nadi.moment(t.year, t.month, t.day, 12, 0, 0, tz)
end

local function streak(now: number, tz: number, settings): number
	local goal = goal_s(settings)
	local n = 0
	local ms = noon(now, tz)
	-- (today, while it is not met yet, does not break it)
	if total(stats.days[day_key(ms, tz)]) < goal then
		ms -= DAY_MS
	end
	for _ = 1, KEEP_DAYS do
		local t = nadi.time(ms, tz)
		if counted(t.weekday, settings) then
			if total(stats.days[day_key(ms, tz)]) >= goal then
				n += 1
			else
				break
			end
		end
		ms -= DAY_MS
	end
	return n
end

-- the average of the days with time, of the 14 before `ms`
local function average(ms: number, tz: number): number
	local sum, n = 0, 0
	for i = 1, 14 do
		local v = total(stats.days[day_key(ms - i * DAY_MS, tz)])
		if v > 0 then
			sum += v
			n += 1
		end
	end
	return if n > 0 then sum / n else 0
end

-- what the days hold, summed: projects, languages, files, sites, hours
local function summed(keys: { string })
	local out = { code = 0, research = 0, projects = {}, languages = {}, files = {}, sites = {}, hours = {} }
	for _, k in keys do
		local d = stats.days[k]
		if d and d.code ~= nil then
			out.code += d.code
			out.research += d.research or 0
			for name, v in d.projects do
				add(out.projects, name, v)
			end
			for name, v in d.languages do
				add(out.languages, name, v)
			end
			for name, v in d.files do
				add(out.files, name, v)
			end
			for name, v in d.sites do
				add(out.sites, name, v)
			end
			for h, v in d.hours do
				add(out.hours, h, v)
			end
		end
	end
	return out
end

local function ranked(t): { { name: string, v: number } }
	local out = {}
	for name, v in t do
		if v >= 30 then
			table.insert(out, { name = name, v = v })
		end
	end
	table.sort(out, function(a, b)
		return a.v > b.v
	end)
	return out
end

local function colours_of(projects): { [string]: string }
	local out = {}
	for i, p in ranked(projects) do
		out[p.name] = PALETTE[i] or OTHER
	end
	return out
end

local function pct(v: number, of: number): string?
	if of <= 0 then
		return nil
	end
	local p = math.floor((v - of) / of * 100 + 0.5)
	return if p >= 0 then "▲ " .. p .. "%" else "▼ " .. -p .. "%"
end

-- ------------------------------------------------------------------------------------------------------------- the blocks

local function columns(days, colours, mark_key: string?, best_key: string?, goal: number?)
	local items = {}
	for _, d in days do
		local parts = {}
		local rec = stats.days[d.key]
		if rec and rec.projects then
			local rest = total(rec)
			for _, p in ranked(rec.projects) do
				table.insert(parts, { v = p.v, color = colours[p.name] or OTHER })
				rest -= p.v
			end
			if rest > 30 then
				table.insert(parts, { v = rest, color = OTHER })
			end
		end
		local t = total(rec)
		table.insert(items, {
			label = string.sub(d.t.wday, 1, 1),
			parts = parts,
			mark = if d.key == mark_key then "theme" elseif d.key == best_key then "second" else nil,
			action = "day:" .. d.key,
			tip = d.t.wday .. " " .. d.t.day .. ", " .. duration(t),
		})
	end
	return { kind = "columns", items = items, goal = goal, height = 96 }
end

local function project_ranks(projects, colours, limit: number)
	local list = ranked(projects)
	if #list == 0 then
		return nil
	end
	local top = list[1].v
	local rows = {}
	for i, p in list do
		if i > limit then
			break
		end
		table.insert(rows, { icon = "folder", title = p.name, part = p.v / top, color = colours[p.name] or OTHER, text = duration(p.v), action = "open:" .. p.name })
	end
	return { kind = "ranks", rows = rows }
end

local function language_split(languages)
	local parts = {}
	local other = 0
	for i, l in ranked(languages) do
		if i <= 4 and l.name ~= "Other" then
			table.insert(parts, { v = l.v, color = LANGUAGE_COLOR[l.name] or PALETTE[i + 1] or OTHER, label = l.name .. " " .. duration(l.v) })
		else
			other += l.v
		end
	end
	if other >= 60 then
		table.insert(parts, { v = other, color = OTHER, label = "Other " .. duration(other) })
	end
	return if #parts > 0 then { kind = "split", parts = parts } else nil
end

local function site_ranks(sites, limit: number)
	local list = ranked(sites)
	if #list == 0 then
		return nil
	end
	local rows = {}
	for i, s in list do
		if i > limit then
			break
		end
		table.insert(rows, { icon = SITE_ICON[s.name] or "globe", title = s.name, part = s.v / list[1].v, color = "#a7dcbf", text = duration(s.v) })
	end
	return { kind = "ranks", rows = rows }
end

local function file_ranks(files, limit: number)
	local list = ranked(files)
	if #list == 0 then
		return nil
	end
	local rows = {}
	for i, f in list do
		if i > limit then
			break
		end
		local l = language_of(f.name)
		table.insert(rows, { icon = "file", title = f.name, part = f.v / list[1].v, color = (l and LANGUAGE_COLOR[l]) or OTHER, text = duration(f.v) })
	end
	return { kind = "ranks", rows = rows }
end

local function hours_of(sum)
	local vals = {}
	local top = 1
	for h = 0, 23 do
		top = math.max(top, sum.hours[tostring(h)] or 0)
	end
	for h = 0, 23 do
		table.insert(vals, (sum.hours[tostring(h)] or 0) / top)
	end
	return { kind = "hours", values = vals }
end

local function tabs()
	local items = {}
	for _, s in { { "day", "Day" }, { "week", "Week" }, { "month", "Month" } } do
		table.insert(items, { label = s[2], action = "span:" .. s[1], on = span == s[1] })
	end
	return { kind = "toolbar", items = { { kind = "grow" }, { kind = "seg", items = items }, { kind = "grow" } } }
end

local function nav(label: string)
	return { kind = "stepnav", prev = "back", next = if shift > 0 then "ahead" else nil, label = label }
end

local function day_view(ev, settings)
	local now, tz = ev.now, ev.tz_offset or 0
	local at = noon(now, tz) - shift * DAY_MS
	local key = day_key(at, tz)
	local t = nadi.time(at, tz)
	local d = stats.days[key]
	local sum = summed({ key })
	local goal = goal_s(settings)
	local body: { any } = {}
	table.insert(body, nav(if shift == 0 then "Today" elseif shift == 1 then "Yesterday" else t.wday .. " " .. t.day .. " " .. t.mon))
	local chips = {}
	local st = streak(now, tz, settings)
	if st > 0 then
		table.insert(chips, { icon = "bolt", text = tostring(st), tone = "hot" })
	end
	local vs = pct(total(d), average(at, tz))
	if vs and total(d) > 0 then
		table.insert(chips, { text = vs, tone = if string.sub(vs, 1, 3) == "▲" then "ok" else "no" })
	end
	table.insert(body, {
		kind = "goal", icon = "code", part = sum.code / goal, extra = sum.research / goal,
		text = duration(total(d)), sub = "of " .. duration(goal), chips = chips,
	})
	-- (the week it is in, Monday first)
	local monday = at - t.weekday * DAY_MS
	local week = days_from(monday, 7, tz)
	local colours = colours_of(summed((function()
		local ks = {}
		for _, w in week do
			table.insert(ks, w.key)
		end
		return ks
	end)()).projects)
	table.insert(body, columns(week, colours, key, nil, goal))
	local pr = project_ranks(sum.projects, colours, 5)
	if pr then
		table.insert(body, pr)
	end
	if sum.research > 0 then
		table.insert(body, { kind = "split", parts = { { v = sum.code, color = "theme", label = "Code " .. duration(sum.code) }, { v = sum.research, color = "#a7dcbf", label = "Research " .. duration(sum.research) } } })
	end
	local ls = language_split(sum.languages)
	if ls then
		table.insert(body, ls)
	end
	local sr = site_ranks(sum.sites, 3)
	if sr then
		table.insert(body, sr)
	end
	if total(d) > 0 then
		table.insert(body, hours_of(sum))
	end
	-- (today: the file you are in, its time today, its project's git state)
	if shift == 0 and now_file then
		local l = language_of(now_file)
		local card: { any } = {
			{ kind = "site", icon = "file", color = (l and LANGUAGE_COLOR[l]) or nil, text = now_file .. (if now_unsaved then " ●" else ""), chip = { icon = "clock", text = duration((d and d.files and d.files[now_file]) or 0) } },
		}
		if project then
			local items = { { icon = "branch", text = project.branch } }
			if project.changed > 0 then
				table.insert(items, { icon = "pen", text = "+" .. project.added .. " −" .. project.deleted })
				table.insert(items, { icon = "file", text = tostring(project.changed) })
			else
				table.insert(items, { icon = "check", text = "Clean", tone = "ok" })
			end
			table.insert(card, { kind = "chips", items = items })
		end
		table.insert(body, { kind = "card", body = card })
	end
	return body
end

local function week_view(ev, settings)
	local now, tz = ev.now, ev.tz_offset or 0
	local today = noon(now, tz)
	local tt = nadi.time(today, tz)
	local monday = today - tt.weekday * DAY_MS - shift * 7 * DAY_MS
	local week = days_from(monday, 7, tz)
	local keys, prev_keys = {}, {}
	for _, w in week do
		table.insert(keys, w.key)
	end
	for _, w in days_from(monday - 7 * DAY_MS, 7, tz) do
		table.insert(prev_keys, w.key)
	end
	local sum = summed(keys)
	local before = summed(prev_keys)
	local goal = goal_s(settings)
	local goal_week = 0
	local best, best_v = nil, 0
	local active = 0
	for _, w in week do
		if counted(w.t.weekday, settings) then
			goal_week += goal
		end
		local v = total(stats.days[w.key])
		if v > best_v then
			best, best_v = w.key, v
		end
		if v > 0 then
			active += 1
		end
	end
	local body: { any } = {}
	local first, last = week[1].t, week[7].t
	table.insert(body, nav(if shift == 0 then "This Week" elseif shift == 1 then "Last Week" else first.day .. " " .. first.mon .. " to " .. last.day .. " " .. last.mon))
	local all = sum.code + sum.research
	local chips = { { icon = "timer", text = duration(if active > 0 then all / active else 0) } }
	local vs = pct(all, before.code + before.research)
	if vs then
		table.insert(chips, { text = vs, tone = if string.sub(vs, 1, 3) == "▲" then "ok" else "no" })
	end
	table.insert(body, { kind = "goal", icon = "code", part = sum.code / goal_week, extra = sum.research / goal_week, text = duration(all), sub = "of " .. duration(goal_week), chips = chips })
	local colours = colours_of(sum.projects)
	table.insert(body, columns(week, colours, if shift == 0 then day_key(today, tz) else nil, best, goal))
	local rows = {}
	for _, w in week do
		local d = stats.days[w.key]
		local vals = {}
		local top = 0
		for h = 0, 23 do
			top = math.max(top, (d and d.hours and d.hours[tostring(h)]) or 0)
		end
		for h = 0, 23 do
			local v = (d and d.hours and d.hours[tostring(h)]) or 0
			table.insert(vals, if v > 0 then math.min(1, v / 1800) else 0)
		end
		table.insert(rows, { label = string.sub(w.t.wday, 1, 1), values = vals })
	end
	if all > 0 then
		table.insert(body, { kind = "heat", rows = rows })
	end
	local pr = project_ranks(sum.projects, colours, 5)
	if pr then
		table.insert(body, pr)
	end
	local ls = language_split(sum.languages)
	if ls then
		table.insert(body, ls)
	end
	local fr = file_ranks(sum.files, 3)
	if fr then
		table.insert(body, { kind = "card", body = { fr } })
	end
	local sr = site_ranks(sum.sites, 3)
	if sr then
		table.insert(body, sr)
	end
	return body
end

local function month_view(ev, settings)
	local now, tz = ev.now, ev.tz_offset or 0
	local tt = nadi.time(now, tz)
	local year, month = tt.year, tt.month - shift
	while month < 1 do
		month += 12
		year -= 1
	end
	local first = nadi.moment(year, month, 1, 12, 0, 0, tz)
	local ft = nadi.time(first, tz)
	local today_key = day_key(now, tz)
	local goal = goal_s(settings)
	local days = {}
	local keys = {}
	local met, countable = 0, 0
	local ms = first
	while nadi.time(ms, tz).month == ft.month do
		local t = nadi.time(ms, tz)
		local key = day_key(ms, tz)
		local future = key > today_key
		local v = total(stats.days[key])
		table.insert(keys, key)
		if not future and counted(t.weekday, settings) then
			countable += 1
			if v >= goal then
				met += 1
			end
		end
		local top = ""
		local rp = ranked((stats.days[key] or {}).projects or {})
		if rp[1] then
			top = ", " .. rp[1].name
		end
		table.insert(days, {
			v = if future then 0 else math.min(1, v / (goal * 1.5)),
			ring = not future and v >= goal,
			today = key == today_key,
			future = future,
			action = if future then nil else "day:" .. key,
			tip = t.wday .. " " .. t.day .. ", " .. duration(v) .. top,
		})
		ms += DAY_MS
	end
	local sum = summed(keys)
	local all = sum.code + sum.research
	local active = 0
	for _, k in keys do
		if total(stats.days[k]) > 0 then
			active += 1
		end
	end
	local body: { any } = {}
	table.insert(body, nav(if shift == 0 then "This Month" else ft.mon .. " " .. ft.year))
	local chips = { { icon = "check", text = met .. " / " .. countable, tone = "ok" } }
	local st = streak(now, tz, settings)
	if st > 0 and shift == 0 then
		table.insert(chips, { icon = "bolt", text = tostring(st), tone = "hot" })
	end
	table.insert(body, { kind = "goal", ring = false, text = duration(all), sub = "⌀ " .. duration(if active > 0 then all / active else 0), chips = chips })
	table.insert(body, { kind = "calendar", lead = ft.weekday, days = days, labels = { "M", "T", "W", "T", "F", "S", "S" } })
	local colours = colours_of(sum.projects)
	local pr = project_ranks(sum.projects, colours, 5)
	if pr then
		table.insert(body, pr)
	end
	local ls = language_split(sum.languages)
	if ls then
		table.insert(body, ls)
	end
	local sr = site_ranks(sum.sites, 3)
	if sr then
		table.insert(body, sr)
	end
	return body
end

-- ------------------------------------------------------------------------------------------------------------- the answer

local function draw(ev)
	local now, tz = ev.now, ev.tz_offset or 0
	local settings = ev.settings or {}
	local d = stats.days[day_key(now, tz)]
	local today = total(d)
	if today <= 0 and not coding_now then
		return {}
	end
	local goal = goal_s(settings)
	local code, research = (d and d.code) or 0, (d and d.research) or 0
	local body = if span == "week" then week_view(ev, settings) elseif span == "month" then month_view(ev, settings) else day_view(ev, settings)
	table.insert(body, tabs())
	local text = duration(today)
	local met = today >= goal
	return {
		peek = {
			ring = { part = code / goal, extra = research / goal, theme = coding_now },
			icon = "code",
			color = if coding_now then nil else "#6d6d78",
			text = text,
			label = text .. " of " .. duration(goal),
		},
		-- (the goal met: it comes out of its edge, once a day)
		bump = if met then "met|" .. day_key(now, tz) else nil,
		panel = { body = body },
		tip = text .. " of " .. duration(goal),
	}
end

local function answer(ev)
	local out: any = { widgets = { work = draw(ev) } }
	return out
end

local function look(ev)
	local now = ev.now
	count(ev)
	local out = answer(ev)
	-- git, for the project in the editor: only while its panel is open (nobody sees it otherwise)
	local open = ev.open and ev.open.work
	if open and now_project and git.pending == 0 and now - git.at >= GIT_EVERY_MS then
		local dir = project_dir(now_project :: string)
		if dir then
			git = { project = now_project, at = now, pending = 3 }
			out.run = {
				{ id = "branch", line = { "git", "-C", dir, "rev-parse", "--abbrev-ref", "HEAD" } },
				{ id = "diff", line = { "git", "-C", dir, "diff", "--numstat", "--no-renames", "HEAD" } },
				{ id = "untracked", line = { "git", "-C", dir, "ls-files", "--others", "--exclude-standard" } },
			}
		else
			project = nil
		end
	end
	-- (the panel shut: back to today)
	if not open and (span ~= "day" or shift ~= 0) then
		span, shift = "day", 0
	end
	if dirty and now - last_flush >= FLUSH_MS then
		last_flush, dirty = now, false
		out.store = kept()
	end
	return out
end

function handle(ev)
	local e = ev.event
	if e == "start" then
		stats = if ev.store and type(ev.store.days) == "table" then ev.store else { days = {} }
		dirty, last_flush, last_tick = false, ev.now, 0
		now_project, now_project_at, now_file, now_unsaved, coding_now = nil, 0, nil, false, false
		editor_of, folders, git, project, span, shift, bumped_for = {}, {}, { at = 0, pending = 0 }, nil, "day", 0, ""
		return nil
	elseif e == "file" then
		folders[ev.path] = editor_folders(ev.text or "")
		return nil
	elseif e == "command" then
		local stdout = ev.stdout or ""
		if ev.id == "branch" then
			git.branch = { code = ev.code, text = string.match(stdout, "^%s*(.-)%s*$") }
		elseif ev.id == "diff" then
			git.diff = if ev.code == 0 then stdout else ""
		elseif ev.id == "untracked" then
			git.untracked = if ev.code == 0 then stdout else ""
		else
			return nil
		end
		git.pending = math.max(0, git.pending - 1)
		if git.pending == 0 then
			project = if git.branch and git.branch.code == 0 and git.branch.text ~= "" then project_info(git.branch.text, git.diff or "", git.untracked or "") else nil
			return answer(ev)
		end
		return nil
	elseif e == "processes" then
		return look(ev)
	elseif e == "action" then
		local a = ev.action or ""
		local s = string.match(a, "^span:(%a+)$")
		if s then
			span, shift = s, 0
			return answer(ev)
		elseif a == "back" then
			shift += 1
			return answer(ev)
		elseif a == "ahead" then
			shift = math.max(0, shift - 1)
			return answer(ev)
		end
		local day = string.match(a, "^day:(%d+%-%d+%-%d+)$")
		if day then
			-- (a day pressed: that day)
			local tz = ev.tz_offset or 0
			local y, m, dd = string.match(day, "^(%d+)%-(%d+)%-(%d+)$")
			local at = nadi.moment(tonumber(y) :: number, tonumber(m) :: number, tonumber(dd) :: number, 12, 0, 0, tz)
			span, shift = "day", math.max(0, math.floor((noon(ev.now, tz) - at) / DAY_MS + 0.5))
			return answer(ev)
		end
		local name = string.match(a, "^open:(.+)$")
		if name then
			local exe = editor_of[name]
			local out = answer(ev)
			out.focus = { exe = if exe and exe ~= "" then exe else nil, title = name }
			return out
		end
	end
	return nil
end
