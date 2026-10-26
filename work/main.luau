--!strict
-- Work: what you work on. Which kind of program has your focus (coding, writing, design, messaging, media, gaming, browsing), day
-- totals, a section for each kind you used lately, the coding one with the file, the project and its git state, and at rest the
-- session you are in.
--
-- A session is confirmed only once a kind has held steady for a while: alt-tabs do not start a new one every couple of seconds.
-- Ending one takes longer than starting one: a glance at Explorer or a chat does not end it.
--
-- It is told every two seconds which windows are open (`processes`, with their programs) and how long the keyboard and the mouse
-- have been left alone (`idle_s`), keeps the day totals of the last 30 days in its store, finds the folder of the project open in
-- the editor in the editors' own `storage.json` (`files`) and asks `git` about it (`run`) while its panel is open.

local IDLE_THRESHOLD_S = 90
local CONFIRM_MS = 8000
local LEAVE_GRACE_MS = 45000
local NON_SESSION = { idle = true, unknown = true, other = true }
local RECENT_ACTIVE_MS = 30000 -- a kind still counts as going on this long after it was last in front
local KEEP_DAYS = 30
local FLUSH_MS = 30000
local GIT_EVERY_MS = 6000
local MAX_FILES = 5

local LANGUAGE_EXT = {
	py = "Python", pyw = "Python", js = "JavaScript", jsx = "JavaScript", mjs = "JavaScript", cjs = "JavaScript",
	ts = "TypeScript", tsx = "TypeScript", java = "Java", kt = "Kotlin", kts = "Kotlin", c = "C", h = "C",
	cpp = "C++", cc = "C++", cxx = "C++", hpp = "C++", hh = "C++", cs = "C#", go = "Go", rs = "Rust", rb = "Ruby",
	php = "PHP", swift = "Swift", m = "Objective-C", mm = "Objective-C", scala = "Scala", dart = "Dart", lua = "Lua",
	luau = "Luau", r = "R", jl = "Julia", hs = "Haskell", ex = "Elixir", exs = "Elixir", erl = "Erlang", clj = "Clojure",
	sql = "SQL", html = "HTML", htm = "HTML", css = "CSS", scss = "SCSS", sass = "Sass", less = "Less", json = "JSON",
	yaml = "YAML", yml = "YAML", toml = "TOML", xml = "XML", md = "Markdown", sh = "Shell", bash = "Shell", zsh = "Shell",
	ps1 = "PowerShell", bat = "Batch", vue = "Vue", svelte = "Svelte", proto = "Protobuf", graphql = "GraphQL",
	gql = "GraphQL", ipynb = "Jupyter Notebook",
}

-- window-title parts that are the editor's own name, not a project
local EDITOR_NAMES = {
	["visual studio code"] = true, ["visual studio code - insiders"] = true, code = true, ["vs code"] = true, cursor = true,
	["sublime text"] = true, ["notepad++"] = true, ["android studio"] = true, pycharm = true, ["intellij idea"] = true,
	webstorm = true, clion = true, rider = true, goland = true, phpstorm = true, vim = true, neovim = true,
}

local BROWSERS = { chrome = true, msedge = true, firefox = true, brave = true, opera = true, vivaldi = true, arc = true }

local APP_CATEGORY = {
	code = "coding", ["code - insiders"] = "coding", codium = "coding", devenv = "coding", pycharm64 = "coding",
	idea64 = "coding", clion64 = "coding", webstorm64 = "coding", rider64 = "coding", goland64 = "coding",
	phpstorm64 = "coding", sublime_text = "coding", ["notepad++"] = "coding", vim = "coding", nvim = "coding",
	cursor = "coding", windowsterminal = "coding", cmd = "coding", powershell = "coding", pwsh = "coding", wt = "coding",
	["git-bash"] = "coding", mintty = "coding", androidstudio64 = "coding", postman = "coding", dbeaver = "coding",
	insomnia = "coding", ["docker desktop"] = "coding", gitkraken = "coding",
	winword = "writing", notion = "writing", obsidian = "writing", typora = "writing", onenote = "writing",
	figma = "design", figmaagent = "design", photoshop = "design", illustrator = "design", blender = "design", inkscape = "design",
	slack = "communication", discord = "communication", teams = "communication", outlook = "communication",
	thunderbird = "communication", zoom = "communication",
	spotify = "media", vlc = "media",
	steam = "gaming", epicgameslauncher = "gaming", ["battle.net"] = "gaming",
}

local TITLE_KEYWORDS = {
	{ "coding", { "github.com", "gitlab.com", "stackoverflow.com", "localhost", "developer.mozilla", "docs.python.org", "leetcode", "bitbucket.org", "visual studio code", "readthedocs" } },
	{ "writing", { "docs.google.com", "notion.so", "overleaf.com" } },
	{ "communication", { "mail.google.com", "outlook.office.com", "discord.com", "web.whatsapp.com", "slack.com", "web.telegram.org" } },
	{ "media", { "youtube.com", "netflix.com", "twitch.tv", "reddit.com", "spotify.com" } },
}

local LABEL = {
	coding = "Coding", writing = "Writing", design = "Designing", communication = "Messaging", media = "Watching",
	gaming = "Gaming", browsing = "Browsing", other = "On the Computer",
}
local GLYPH = { coding = "code", writing = "pen", design = "sparkle", communication = "mail", media = "music", gaming = "gamepad", browsing = "globe" }
local TINT = "#6d6d78"

-- ------------------------------------------------------------------------------------------------------ reading titles

local function trim_marks(s: string): string
	s = string.gsub(s, "^[%s●•∙]+", "")
	s = string.gsub(s, "[%s●•∙]+$", "")
	return s
end

local function parts_of(title: string): { string }
	local t = string.gsub(string.gsub(title, " — ", " - "), " – ", " - ")
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

-- (project, language) from an editor's window title: "file.ext - project - App Name"
local function coding_context(title: string): (string?, string?)
	if title == "" then
		return nil, nil
	end
	local parts = {}
	for _, p in parts_of(title) do
		local x = trim_marks(p)
		if x ~= "" and not EDITOR_NAMES[string.lower(x)] then
			table.insert(parts, x)
		end
	end
	local file = parts[1]
	if not file then
		return nil, nil
	end
	return if #parts >= 2 then parts[#parts] else nil, language_of(file)
end

-- (file, unsaved) from an editor's window title: "● main.rs - project - Visual Studio Code"; only what looks like a file
local function editor_file(title: string): (string?, boolean)
	local first = string.match(parts_of(title)[1] or "", "^%s*(.-)%s*$") or ""
	local unsaved = string.find(first, "^[●•∙]") ~= nil
	local file = trim_marks(first)
	local lower = string.lower(file)
	local stem, ext = string.match(file, "^(.+)%.([%w]+)$")
	local looks = (utf8.len(file) or 0) <= 80
		and string.sub(file, -3) ~= "…"
		and ((stem ~= nil and #ext >= 1 and #ext <= 8) or lower == "makefile" or lower == "dockerfile" or lower == "license" or lower == "readme" or lower == "procfile")
	if file ~= "" and not EDITOR_NAMES[lower] and looks then
		return file, unsaved
	end
	return nil, unsaved
end

local function categorize(path: string, title: string): string
	if path == "" then
		return "other"
	end
	local file = string.lower(string.match(path, "([^\\/]+)$") or path)
	local base = string.gsub(file, "%.exe$", "")
	if BROWSERS[base] then
		local t = string.lower(title)
		for _, k in TITLE_KEYWORDS do
			for _, word in k[2] do
				if string.find(t, word, 1, true) then
					return k[1]
				end
			end
		end
		return "browsing"
	end
	return APP_CATEGORY[base] or "other"
end

local function exe_name(path: string): string
	local file = string.match(path, "([^\\/]+)$") or path
	return (string.gsub(file, "%.[eE][xX][eE]$", ""))
end

local function duration(ms: number): string
	local s = ms // 1000
	local h, m = s // 3600, (s % 3600) // 60
	if h > 0 then
		return h .. "h " .. m .. "m"
	elseif m > 0 then
		return m .. "m"
	end
	return "<1m"
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
	local files = {}
	for line in string.gmatch(diff, "[^\n]+") do
		local a, d, path = string.match(line, "^(%S+)\t(%S+)\t(.+)$")
		if path then
			path = string.gsub(path, '^"(.*)"$', "%1")
			table.insert(files, { name = string.match(path, "([^/]+)$") or path, path = path, added = tonumber(a) or 0, deleted = tonumber(d) or 0, status = "M" })
		end
	end
	table.sort(files, function(x, y)
		return x.added + x.deleted > y.added + y.deleted
	end)
	local n = 0
	for line in string.gmatch(untracked, "[^\n]+") do
		local p = string.match(line, "^%s*(.-)%s*$") or ""
		if p ~= "" and n < 300 then
			n += 1
			p = string.gsub(p, '^"(.*)"$', "%1")
			table.insert(files, { name = string.match(p, "([^/]+)$") or p, path = p, added = 0, deleted = 0, status = "A" })
		end
	end
	local added, deleted = 0, 0
	for _, f in files do
		added += f.added
		deleted += f.deleted
	end
	local changed = #files
	while #files > MAX_FILES do
		table.remove(files)
	end
	return { branch = branch, files = files, changed = changed, added = added, deleted = deleted }
end

-- ---------------------------------------------------------------------------------------------------------------- state

local stats: any = { days = {} } -- { days = { ["2026-10-11"] = { categories = {}, projects = {}, languages = {} } } }
local dirty = false
local last_flush = 0
local recent = {} -- category -> { last_seen, started_at, app_name, path }
local candidate, candidate_since, candidate_app = "idle", 0, ""
local confirmed: string? = nil
local confirmed_app, confirmed_since = "", 0
local last_tick = 0
local active_project = ""
local coding_file: string? = nil
local coding_unsaved = false
local folders = {} -- by the file they came from
local git: any = { at = 0, pending = 0 }
local project: any = nil

local function day_key(ms: number, tz: number): string
	local t = nadi.time(ms, tz)
	return string.format("%04d-%02d-%02d", t.year, t.month, t.day)
end

local function bump(day: string, kind: string, key: string, secs: number)
	local d = stats.days[day]
	if not d then
		d = { categories = {}, projects = {}, languages = {} }
		stats.days[day] = d
	end
	d[kind] = d[kind] or {}
	d[kind][key] = (d[kind][key] or 0) + secs
end

local function today(day: string, kind: string, key: string): number
	local d = stats.days[day]
	return if d and d[kind] then d[kind][key] or 0 else 0
end

local function observe(ev, category: string, elapsed: number, title: string, app_name: string, path: string)
	local now = ev.now
	if category == "coding" then
		local proj = coding_context(title)
		local file, unsaved = editor_file(title)
		-- a tab that is not a file (chat, settings) keeps showing the last real file
		if file then
			coding_file, coding_unsaved = file, unsaved
		end
		active_project = proj or (if app_name ~= "" then app_name else "your editor")
	end
	if elapsed > 0 then
		local day = day_key(now, ev.tz_offset or 0)
		bump(day, "categories", category, elapsed)
		if category == "coding" then
			local proj, language = coding_context(title)
			if proj then
				bump(day, "projects", proj, elapsed)
			end
			if language then
				bump(day, "languages", language, elapsed)
			end
		end
		dirty = true
	end
	if category == "other" then
		return
	end
	local prev = recent[category]
	local started = if prev and now - prev.last_seen <= RECENT_ACTIVE_MS then prev.started_at else now
	recent[category] = { last_seen = now, started_at = started, app_name = if app_name ~= "" then app_name elseif prev then prev.app_name else "", path = path }
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

-- ------------------------------------------------------------------------------------------------------------- the answer

local function sections(ev)
	local now = ev.now
	local day = day_key(now, ev.tz_offset or 0)
	local list = {}
	for category, r in recent do
		if now - r.last_seen <= RECENT_ACTIVE_MS and category ~= "browsing" then
			table.insert(list, { category = category, r = r })
		end
	end
	table.sort(list, function(a, b)
		return a.r.started_at > b.r.started_at
	end)
	local out = {}
	for _, x in list do
		local category, r = x.category, x.r
		if category == "coding" then
			local in_session = confirmed == "coding"
			local name = if active_project == "" then "Coding" else active_project
			local body: { any } = {}
			if coding_file then
				local language = language_of(coding_file)
				local mark = if coding_unsaved then " ●" else ""
				table.insert(body, { kind = "text", text = if language then language .. " · " .. coding_file .. mark else coding_file .. mark })
			end
			local side = duration(today(day, "categories", "coding") * 1000) .. " today"
			if project and project.changed > 0 then
				local peak = 1
				for _, f in project.files do
					peak = math.max(peak, f.added + f.deleted)
				end
				local rows = {}
				for _, f in project.files do
					local sum = f.added + f.deleted
					table.insert(rows, { title = f.name, sub = if f.status == "A" then "new" else "+" .. f.added .. " −" .. f.deleted, progress = if sum > 0 then math.max(0.1, sum / peak) else nil })
				end
				table.insert(body, { kind = "chips", items = { { icon = "branch", text = project.branch, tone = "" }, { icon = "pen", text = "+" .. project.added .. " −" .. project.deleted .. " · " .. project.changed .. " files", tone = "" } } })
				table.insert(body, { kind = "list", rows = rows })
				if project.changed > #project.files then
					table.insert(body, { kind = "text", text = "+" .. (project.changed - #project.files) .. " more", tone = "muted" })
				end
			elseif project then
				table.insert(body, { kind = "chips", items = { { icon = "branch", text = project.branch, tone = "" }, { icon = "check", text = "No changes", tone = "" } } })
			end
			table.insert(out, {
				kind = "section",
				id = "coding",
				title = name,
				sub = (if in_session then "Editing" else "Not at the keyboard") .. " · " .. side,
				tile = { image = "exe:" .. r.path, icon = "code", color = TINT },
				body = body,
				press = "focus:coding",
			})
		else
			local label = LABEL[category] or "Away"
			table.insert(out, {
				kind = "section",
				id = category,
				title = if r.app_name ~= "" then r.app_name else label,
				sub = label .. " for " .. duration(now - r.started_at),
				tile = { image = "exe:" .. r.path, icon = GLYPH[category] or "dot", color = TINT },
				body = {},
				press = "focus:" .. category,
			})
		end
	end
	return out
end

local function draw(ev)
	local now = ev.now
	local body = sections(ev)
	local session = if confirmed and not NON_SESSION[confirmed] and confirmed ~= "browsing" then confirmed else nil
	if #body == 0 and not session then
		return {}
	end
	local day = day_key(now, ev.tz_offset or 0)
	local total = 0
	for k, v in (stats.days[day] or {}).categories or {} do
		if k ~= "other" then
			total += v
		end
	end
	local text = if session then duration(now - confirmed_since) else duration(total * 1000)
	local label = if session then (LABEL[session] or "") .. " · " .. confirmed_app .. " · " .. text else "Today · " .. text
	local icon = if session then GLYPH[session] or "dot" else "code"
	local r = if session then recent[session] else nil
	return {
		peek = { image = if r then "exe:" .. r.path else nil, icon = icon, color = TINT, text = text, label = label },
		panel = { title = if session then LABEL[session] or "Work" else "Work", sub = "Today " .. duration(total * 1000), tile = { icon = icon, color = TINT }, body = body },
		tip = label,
	}
end

local function look(ev)
	local now = ev.now
	local out: any = {}
	-- every visible window keeps its kind going, in front or not; elapsed 0: the totals and the session follow the front one
	local seen = {}
	for _, w in ev.windows or {} do
		if not w.tool then
			local cat = categorize(w.path or "", w.title or "")
			if cat ~= "other" and not seen[cat] then
				seen[cat] = true
				observe(ev, cat, 0, w.title or "", if (w.name or "") ~= "" then w.name else exe_name(w.path or ""), w.path or "")
			end
		end
	end
	local front = nil
	for _, w in ev.windows or {} do
		if w.front and not w.tool then
			front = w
			break
		end
	end
	local title, path = "", ""
	local state, app = "unknown", ""
	if (ev.idle_s or 0) >= IDLE_THRESHOLD_S then
		state = "idle"
	elseif front then
		title, path = front.title or "", front.path or ""
		app = if (front.name or "") ~= "" then front.name else exe_name(path)
		state = categorize(path, title)
	end
	-- counted at every look, not after the session settles: the first seconds of each switch count too
	local elapsed = math.min(6, (now - last_tick) / 1000)
	if last_tick == 0 then
		elapsed = 0
	end
	last_tick = now
	if state ~= "idle" and state ~= "unknown" then
		observe(ev, state, elapsed, title, app, path)
	end
	if state ~= candidate then
		candidate, candidate_since, candidate_app = state, now, app
	end
	local in_session = confirmed ~= nil and not NON_SESSION[confirmed]
	local dwell = if in_session and NON_SESSION[state] then LEAVE_GRACE_MS else CONFIRM_MS
	if now - candidate_since >= dwell and confirmed ~= state then
		confirmed, confirmed_app, confirmed_since = state, candidate_app, now
	end
	out.widgets = { work = draw(ev) }
	-- git, for the project in the editor: only while its panel is open (nobody sees it otherwise)
	local open = ev.open and ev.open.work
	if open and active_project ~= "" and git.pending == 0 and now - git.at >= GIT_EVERY_MS then
		local dir = project_dir(active_project)
		if dir then
			git = { project = active_project, at = now, pending = 3 }
			out.run = {
				{ id = "branch", line = { "git", "-C", dir, "rev-parse", "--abbrev-ref", "HEAD" } },
				{ id = "diff", line = { "git", "-C", dir, "diff", "--numstat", "--no-renames", "HEAD" } },
				{ id = "untracked", line = { "git", "-C", dir, "ls-files", "--others", "--exclude-standard" } },
			}
		else
			project = nil
		end
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
		dirty, last_flush, recent = false, ev.now, {}
		candidate, candidate_since, candidate_app, confirmed, confirmed_app, confirmed_since = "idle", 0, "", nil, "", 0
		last_tick, active_project, coding_file, coding_unsaved, folders, git, project = 0, "", nil, false, {}, { at = 0, pending = 0 }, nil
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
			return { widgets = { work = draw(ev) } }
		end
		return nil
	elseif e == "processes" then
		return look(ev)
	elseif e == "action" then
		local category = string.match(ev.action or "", "^focus:(.+)$")
		local r = if category then recent[category] else nil
		if r then
			return { focus = { exe = r.path, title = if category == "coding" and active_project ~= "" then active_project else nil } }
		end
	end
	return nil
end
