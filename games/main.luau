--!strict
-- Games: the game you play, its wiki in reach, and how full your controller is (else how smooth it runs).
--
-- A game is a program whose window covers its whole screen and has no title bar (NADI tells which windows do: `full`). It stays
-- until its windows are gone, however often you switch away from it. Its wiki is found by its name (wiki.gg first, then Fandom)
-- and kept; a search there shows the first page's picture and first lines in the widget, the others found as chips, the whole page
-- a press away. At rest: a controller's shape filled as full as its battery; no controller, the frames per second (when Windows
-- lets NADI count them); else how long you have played. While a game runs, ambient banners wait (`quiet`).
local MAX_GAMES = 4
local LINES = 10

-- programs that cover the screen and are not games
local IGNORE = {
	["explorer.exe"] = true,
	["shellexperiencehost.exe"] = true,
	["searchhost.exe"] = true,
	["searchapp.exe"] = true,
	["startmenuexperiencehost.exe"] = true,
	["applicationframehost.exe"] = true,
	["textinputhost.exe"] = true,
	["lockapp.exe"] = true,
	["systemsettings.exe"] = true,
	["screenclippinghost.exe"] = true,
	["gamebar.exe"] = true,
	["widgets.exe"] = true,
	["nadi.exe"] = true,
}
-- video players, browsers (F11, a fullscreen video), presentations and remote desktops cover the screen too (parts of the name)
local NOT_GAMES = {
	"potplayer", "vlc", "mpv", "mpc-hc", "mpc-be", "wmplayer", "kmplayer", "gomplayer", "smplayer", "video.ui", "moviesandtv",
	"chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "zoom", "teams", "powerpnt", "acrord", "mstsc", "vmconnect",
	"obs64", "obs32",
}

local games = {} -- the one most recently in front first
local gear = { controllers = {}, devices = {} }
-- each game's wiki (by its path): its address, or false when none was found
local wikis: { [string]: any } = {}
-- looking for a game's wiki: its path, the addresses to try, the one tried now
local probe: any = nil
-- the search: what was asked, the pages found, the one shown (title, text, picture, link)
local query = ""
local found: { string } = {}
local page: any = nil
local wanted: string? = nil -- a search asked before its wiki was known
-- how big the wiki's words are: normal, medium, large (kept)
local SIZES = { 1.0, 1.15, 1.3 }
local SIZE_NAMES = { "Normal", "Medium", "Large" }
local size = 1

-- what is kept between runs
local function kept()
	return { wikis = wikis, size = size }
end

local function is_not_game(exe: string): boolean
	local e = string.lower(exe)
	if IGNORE[e] then
		return true
	end
	for _, n in NOT_GAMES do
		if string.find(e, n, 1, true) then
			return true
		end
	end
	return false
end

-- a name from the file when the program has none: `deep_rock-galactic.exe` is "Deep Rock Galactic"
local function name_from_path(path: string): string
	local file = string.match(path, "([^\\/]+)$") or path
	local stem = string.gsub(file, "%.[eE][xX][eE]$", "")
	local words = {}
	for w in string.gmatch(stem, "[^_%-]+") do
		table.insert(words, string.upper(string.sub(w, 1, 1)) .. string.sub(w, 2))
	end
	return table.concat(words, " ")
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

-- an address's part, its odd letters written as %XX
local function enc(s: string): string
	return (string.gsub(s, "[^%w%-%._~]", function(c)
		return string.format("%%%02X", string.byte(c))
	end))
end

-- the names a game's wiki may have: "deeprockgalactic", "deep-rock-galactic", "deep"; and without a number at its end ("hades")
function slugs(name: string): { string }
	local words = {}
	for w in string.gmatch(string.lower(name), "[%w]+") do
		table.insert(words, w)
	end
	local out, seen = {}, {}
	local function add(s: string)
		if s ~= "" and not seen[s] then
			seen[s] = true
			table.insert(out, s)
		end
	end
	local function forms(ws: { string })
		add(table.concat(ws, ""))
		add(table.concat(ws, "-"))
	end
	forms(words)
	-- (a sequel: its number, or its roman numeral, off)
	local last = words[#words]
	if #words > 1 and last and (string.match(last, "^%d+$") or string.match(last, "^[ivx]+$")) then
		local less = table.clone(words)
		table.remove(less)
		forms(less)
	end
	if #words > 1 then
		add(words[1])
	end
	return out
end

-- the wikis to try for a game, in order: wiki.gg's, then Fandom's
function candidates(name: string): { string }
	local out = {}
	for _, s in slugs(name) do
		table.insert(out, "https://" .. s .. ".wiki.gg")
		table.insert(out, "https://" .. s .. ".fandom.com")
	end
	return out
end

local function api(base: string, params: string): string
	return base .. "/api.php?" .. params .. "&format=json&formatversion=2"
end

-- words cut to `n` letters (never inside one), "…" after
function cut(t: string, n: number): string
	if (utf8.len(t) or #t) <= n then
		return t
	end
	local at = utf8.offset(t, n) or (#t + 1)
	return string.sub(t, 1, at - 1) .. "…"
end

-- markup to plain words: no tags, no notes ("[1]"), its entities read, its spaces single
function plain(s: string): string
	local t = string.gsub(s, "<[^>]->", " ")
	t = string.gsub(t, "&#91;.-&#93;", "")
	t = string.gsub(t, "%[%d+%]", "")
	t = string.gsub(t, "&shy;", "")
	t = string.gsub(t, "\u{AD}", "")
	t = string.gsub(t, "&nbsp;", " ")
	t = string.gsub(t, "&#160;", " ")
	-- (a letter by its number: "&#44;" is a comma, "&#x2019;" a quote)
	t = string.gsub(t, "&#[xX](%x+);", function(h)
		local n = tonumber(h, 16)
		return if n and n > 0 and n < 0x110000 then utf8.char(n) else ""
	end)
	t = string.gsub(t, "&#(%d+);", function(d)
		local n = tonumber(d)
		return if n and n > 0 and n < 0x110000 then utf8.char(n) else ""
	end)
	t = string.gsub(t, "&quot;", '"')
	t = string.gsub(t, "&#0?39;", "'")
	t = string.gsub(t, "&lt;", "<")
	t = string.gsub(t, "&gt;", ">")
	t = string.gsub(t, "&amp;", "&")
	t = string.gsub(t, "%s+", " ")
	t = string.gsub(t, "^%s+", "")
	t = string.gsub(t, "%s+$", "")
	t = string.gsub(t, " ([%.,;:!%?%)])", "%1")
	return t
end

-- a page's words: its first paragraph that says something (a spoiler warning does not)
function first_words(html: string): string
	for p in string.gmatch(html, "<p[^>]*>(.-)</p>") do
		local t = plain(p)
		if #t >= 50 and not string.find(string.lower(t), "spoiler", 1, true) then
			return t
		end
	end
	return ""
end

-- a section's words: its paragraphs, and its lists (references, who is who) a line each, as many as fit in a panel
function section_words(html: string): string
	local out, size = {}, 0
	local function add(t: string)
		if t ~= "" and size < 1600 and not string.find(string.lower(t), "spoiler", 1, true) then
			table.insert(out, t)
			size += #t
		end
	end
	-- (in the order they come: a paragraph, or a list's item)
	local i = 1
	while size < 1600 do
		local a, b, tag, inner = string.find(html, "<(p)[^>]*>(.-)</p>", i)
		local la, lb, ltag, linner = string.find(html, "<(li)[^>]*>(.-)</li>", i)
		if la and (not a or la < a) then
			a, b, tag, inner = la, lb, ltag, linner
		end
		if not a then
			break
		end
		local t = plain(inner)
		add(if tag == "li" then "\u{2022} " .. t else t)
		i = b + 1
	end
	return table.concat(out, "\n")
end

-- a fact's value: its words; a picture alone (stars), what the picture says
local function value_of(v: string): string
	local t = plain(v)
	if t == "" then
		t = string.match(v, 'alt="([^"]+)"') or ""
		t = string.gsub(t, "%.%w+$", "")
	end
	t = cut(t, 47)
	return t
end

-- the facts of a page's infobox (Fandom's, wiki.gg's, or a table): label and value, the first few
function infobox(html: string): { { string } }
	local facts: { { string } } = {}
	local function add(label: string, value: string)
		local l, v = plain(label), value_of(value)
		if #facts < 5 and l ~= "" and v ~= "" and #l <= 24 then
			table.insert(facts, { l, v })
		end
	end
	-- Fandom: a label and its value in a row
	for label, value in string.gmatch(html, '<h3 class="pi%-data%-label[^"]*"[^>]*>(.-)</h3>%s*<div class="pi%-data%-value[^"]*"[^>]*>(.-)</div>') do
		add(label, value)
	end
	-- Fandom: labels over values (a group side by side)
	local labels: { [string]: string } = {}
	for src, label in string.gmatch(html, '<th class="[^"]*pi%-data%-label[^"]*" data%-source="([^"]+)"[^>]*>(.-)</th>') do
		labels[src] = label
	end
	for src, value in string.gmatch(html, '<td class="[^"]*pi%-data%-value[^"]*" data%-source="([^"]+)"[^>]*>(.-)</td>') do
		if labels[src] then
			add(labels[src], value)
		end
	end
	-- wiki.gg (its "druid" infobox): the rows not folded away
	for class, label, value in string.gmatch(html, '<div class="druid%-row([^"]*)"[^>]*><div class="druid%-label[^"]*"[^>]*>(.-)</div><div class="druid%-data[^"]*"[^>]*>(.-)</div>') do
		if not string.find(class, "collapsed", 1, true) then
			add(label, value)
		end
	end
	-- a table: a heading, its value beside it
	if #facts == 0 then
		local box = string.match(html, '<table[^>]-class="[^"]-infobox.-</table>') or ""
		for label, value in string.gmatch(box, "<tr[^>]*>%s*<th[^>]*>(.-)</th>%s*<td[^>]*>(.-)</td>") do
			add(label, value)
		end
	end
	return facts
end

-- a page's picture, when the wiki cannot say it: its first one big enough (a small one is an icon), as a whole address
function first_picture(html: string, base: string): string
	for attrs in string.gmatch(html, "<img([^>]-)>") do
		local src = string.match(attrs, 'data%-src="([^"]+)"') or string.match(attrs, 'src="([^"]+)"')
		local w = tonumber(string.match(attrs, 'width="(%d+)"') or "0") or 0
		if src and w >= 60 and not string.find(src, "^data:") then
			src = string.gsub(src, "&amp;", "&")
			if string.sub(src, 1, 2) == "//" then
				return "https:" .. src
			elseif string.sub(src, 1, 1) == "/" then
				return base .. src
			end
			return src
		end
	end
	return ""
end

-- the infobox's picture, else the page's first big one; a thumbnail asked smaller
function picture_of(html: string, base: string): string
	local at = string.find(html, "druid%-main%-image") or string.find(html, "pi%-image") or string.find(html, "infobox")
	local pic = if at then first_picture(string.sub(html, at), base) else ""
	if pic == "" then
		pic = first_picture(html, base)
	end
	if string.find(pic, "/thumb/", 1, true) then
		pic = string.gsub(pic, "/(%d+)px%-", "/240px-")
	end
	return pic
end

-- a picture's whole address (its own, or the one it waits to load), none when it has none
local function src_of(attrs: string, base: string): string
	local src = string.match(attrs, 'data%-src="([^"]+)"') or string.match(attrs, 'src="([^"]+)"') or ""
	if src == "" or string.find(src, "^data:") then
		return ""
	end
	src = string.gsub(src, "&amp;", "&")
	if string.sub(src, 1, 2) == "//" then
		return "https:" .. src
	elseif string.sub(src, 1, 1) == "/" then
		return base .. src
	end
	return src
end

-- a table's cell: its words alone; else its pictures, each with the words after it ("8", "1.6K")
function cell_of(html: string, base: string): any
	local items = {}
	local i = 1
	local first = true
	while true do
		local a, b, attrs = string.find(html, "<img([^>]-)>", i)
		local t = plain(string.sub(html, i, if a then a - 1 else #html))
		t = cut(t, 400)
		if first then
			if t ~= "" then
				table.insert(items, { image = "", text = t })
			end
			first = false
		elseif #items > 0 then
			items[#items].text = t
		end
		if not a then
			break
		end
		local src = src_of(attrs, base)
		if src ~= "" and #items < 6 then
			table.insert(items, { image = src, text = "" })
		end
		i = b + 1
	end
	-- (with pictures: a sentence among them is left out, "Raise Arcane to level 20:")
	local pics = false
	for _, it in items do
		pics = pics or it.image ~= ""
	end
	if pics then
		local kept = {}
		for _, it in items do
			if it.image ~= "" or (utf8.len(it.text) or #it.text) <= 10 then
				table.insert(kept, it)
			end
		end
		items = kept
	end
	if #items == 0 then
		return ""
	elseif #items == 1 and items[1].image == "" then
		return items[1].text
	end
	return { items = items }
end

-- a section's tables (not an infobox, not a list of links to other pages), each with the heading over it: a few rows each
function tables_of(html: string, base: string): { any }
	local out = {}
	local at = 1
	local heading = ""
	while #out < 3 do
		local a, b = string.find(html, "<table(.-)</table>", at)
		if not a then
			break
		end
		-- (the last heading before it)
		for h in string.gmatch(string.sub(html, at, a), "<h[2-5][^>]*>(.-)</h[2-5]>") do
			heading = plain(h)
		end
		local t = string.sub(html, a, b)
		local open = string.match(t, "^<table([^>]*)>") or ""
		if not string.find(open, "infobox") and not string.find(open, "navbox") then
			local rows = {}
			local head = false
			for r in string.gmatch(t, "<tr[^>]*>(.-)</tr>") do
				local row, all_th, any = {}, true, false
				for kind, attrs, inner in string.gmatch(r, "<t([hd])([^>]*)>(.-)</t[hd]>") do
					local c = cell_of(inner, base)
					local span = tonumber(string.match(attrs, 'colspan="?(%d+)') or "1") or 1
					if span > 1 then
						c = if type(c) == "string" then { items = { { image = "", text = c } } } else c
						c.span = span
					end
					if kind ~= "h" then
						all_th = false
					end
					if c ~= "" then
						any = true
					end
					table.insert(row, c)
				end
				if any and #row > 0 and #rows < 14 then
					if #rows == 0 then
						head = all_th
					end
					table.insert(rows, row)
				end
			end
			if #rows > 0 then
				table.insert(out, { heading = heading, rows = rows, head = head })
				heading = ""
			end
		end
		at = b + 1
	end
	return out
end

-- a cell's words, all of them
local function words_of(c: any): string
	if type(c) == "string" then
		return c
	end
	local out = {}
	for _, it in (c and c.items) or {} do
		if it.text ~= "" then
			table.insert(out, it.text)
		end
	end
	return table.concat(out, " ")
end

-- a cell as a row shows it: its words cut short
local function short_cell(c: any): any
	if type(c) == "string" then
		return cut(c, 80)
	end
	local items = {}
	for _, it in c.items do
		table.insert(items, { image = it.image, text = cut(it.text, 80) })
	end
	return { items = items, span = c.span }
end

-- a table as blocks: a grid when its cells are short; when it holds sentences (what a talent does), each row as a paragraph under
-- its short words ("Keen Mind"), its pictures left out
function blocks_of(t): { any }
	local wordy = false
	for _, r in t.rows do
		for _, c in r do
			wordy = wordy or (utf8.len(words_of(c)) or 0) > 40
		end
	end
	if not wordy then
		local rows = {}
		for _, r in t.rows do
			local row = {}
			for _, c in r do
				table.insert(row, short_cell(c))
			end
			table.insert(rows, row)
		end
		return { { kind = "table", rows = rows, head = t.head } }
	end
	local out = {}
	for _, r in t.rows do
		local label, text = {}, {}
		for _, c in r do
			local w = words_of(c)
			if (utf8.len(w) or 0) > 40 then
				table.insert(text, w)
			elseif w ~= "" then
				table.insert(label, w)
			end
		end
		if #text > 0 or #label > 0 then
			table.insert(out, { kind = "text", label = table.concat(label, " "), text = table.concat(text, " "), lines = 6 })
		end
	end
	return out
end

local function link_of(base: string, title: string): string
	return base .. "/wiki/" .. enc((string.gsub(title, " ", "_")))
end

-- the controller on, and how full it is (none: none; wired, or not told: its level none)
function controller_of(g): any
	for _, c in g.controllers or {} do
		return { level = c.level }
	end
	return nil
end

-- how smooth: green, amber, red
local function fps_color(fps: number): string
	return if fps >= 55 then "#5AC88C" elseif fps >= 30 then "#E8B04A" else "#E87A7A"
end

local function body_of(front)
	local base = wikis[front.path]
	local body: { any } = {}
	table.insert(body, {
		kind = "field",
		placeholder = if base == false then "No Wiki Found" else "Search the Wiki",
		action = if base == false then "" else "search",
		value = query,
		-- (the words' size, a press to the next)
		button = if base then { text = "Aa", action = "size", tip = "Text Size: " .. SIZE_NAMES[size] } else nil,
	})
	if page then
		-- its picture and its facts; a line of what it is (or of the section picked); its sections; the other pages found
		table.insert(body, { kind = "infocard", image = page.image or "", title = page.title, facts = page.facts or {}, action = "open" })
		if page.sections and #page.sections > 0 then
			local chips = { { text = "Overview", action = "sec:0", on = page.section == "0" } }
			for _, s in page.sections do
				table.insert(chips, { text = s.name, action = "sec:" .. s.index, on = page.section == s.index })
			end
			table.insert(body, { kind = "chips", items = chips })
		end
		local tables = page.tables or {}
		if #tables > 0 then
			for _, t in tables do
				if t.heading ~= "" then
					table.insert(body, { kind = "text", label = t.heading, text = "" })
				end
				for _, b in blocks_of(t) do
					table.insert(body, b)
				end
			end
		elseif (page.text or "") ~= "" then
			table.insert(body, { kind = "text", text = page.text, lines = if page.section == "0" then LINES else 40 })
		end
		local others = {}
		for i, t in found do
			if t ~= page.title and #others < 3 then
				table.insert(others, { text = t, action = "pick:" .. i })
			end
		end
		if #others > 0 then
			table.insert(body, { kind = "chips", items = others })
		end
	end
	return body
end

local function draw(now: number)
	local front = games[1]
	if not front then
		-- (no game: idle, so NADI can hide it)
		return { widgets = { game = { idle = true, peek = { icon = "gamepad", text = "No game" }, tip = "No game" } }, quiet = false }
	end
	local played = duration(now - front.since)
	-- (its own icon is on the dock. Here: a controller's shape, as full as its battery; else its frames per second, coloured by
	-- how smooth; else the time)
	local pad = controller_of(gear)
	-- (the frame rate as shown: it moves on only when it changed by a few, or a while passed; a number that ticks by one every
	-- second is noise, and each change is drawn)
	local raw = front.usage and front.usage.fps
	if raw then
		local shown = front.fps_shown
		if not shown or math.abs(raw - shown) >= 3 or now - (front.fps_at or 0) >= 4000 then
			front.fps_shown = math.floor(raw + 0.5)
			front.fps_at = now
		end
	end
	local fps = if raw then front.fps_shown else nil
	local peek: any = { label = front.name .. ", " .. played }
	if pad then
		peek.pad = pad
	elseif fps then
		-- (its number, its digits rolling as it changes; in the colour of how smooth)
		peek.roll = { value = math.floor(fps + 0.5), color = fps_color(fps) }
	else
		peek.icon = ""
		peek.text = played
	end
	return {
		widgets = {
			game = {
				peek = peek,
				panel = { width = if page and page.tables and #page.tables > 0 then 540 else 430, zoom = SIZES[size], title = front.name, sub = played, tile = { image = "exe:" .. front.path, icon = "gamepad" }, body = body_of(front) },
				tip = front.name .. "\n" .. played,
			},
		},
		watch = { front.pid },
		quiet = true,
	}
end

-- ask the next address for the wiki being looked for (none left: there is none)
local function probe_next(out)
	if not probe then
		return
	end
	probe.at += 1
	local base = probe.tries[probe.at]
	if not base then
		wikis[probe.path] = false
		out.store = kept()
		probe = nil
		return
	end
	out.fetch = { { id = "probe", url = api(base, "action=query&meta=siteinfo") } }
end

-- the front game's wiki: known, or looked for now
local function find_wiki(out)
	local front = games[1]
	if not front or wikis[front.path] ~= nil or (probe and probe.path == front.path) then
		return
	end
	probe = { path = front.path, tries = candidates(front.name), at = 0 }
	probe_next(out)
end

local function show(out, title: string)
	local front = games[1]
	local base = front and wikis[front.path]
	if not base then
		return
	end
	page = { title = title, text = "", image = "", facts = {}, sections = {}, section = "0", url = link_of(base, title), base = base }
	out.fetch = {
		{ id = "page", url = api(base, "action=parse&prop=text&section=0&redirects=1&disablelimitreport=1&disableeditsection=1&page=" .. enc(title)), max_kb = 1024 },
		{ id = "secs", url = api(base, "action=parse&prop=sections&redirects=1&page=" .. enc(title)) },
	}
end

local function search(out, q: string)
	query = q
	local front = games[1]
	local base = front and wikis[front.path]
	if q == "" then
		page, found = nil, {}
		return
	end
	if not base then
		wanted = q
		return
	end
	out.fetch = { { id = "search", url = api(base, "action=query&list=search&srlimit=6&srsearch=" .. enc(q)) } }
end

-- the windows that cover their screen join; the one in front goes first; a game whose windows are all gone goes
local function see_windows(ev)
	local seen, at, alive = {}, {}, {}
	for _, w in ev.windows or {} do
		alive[w.pid] = true
		if w.full and not w.tool and (w.path or "") ~= "" and not is_not_game(w.exe or "") and not at[w.pid] then
			at[w.pid] = true
			table.insert(seen, w)
		end
	end
	local kept = {}
	for _, g in games do
		if alive[g.pid] then
			table.insert(kept, g)
		end
	end
	games = kept
	local before = games[1] and games[1].pid
	for i = #seen, 1, -1 do
		local w = seen[i]
		local found_at = nil
		for j, g in games do
			if g.pid == w.pid then
				found_at = j
				break
			end
		end
		if found_at then
			table.insert(games, 1, table.remove(games, found_at))
		else
			table.insert(games, 1, { pid = w.pid, name = if (w.name or "") ~= "" then w.name else name_from_path(w.path), path = w.path, since = ev.now })
		end
	end
	while #games > MAX_GAMES do
		table.remove(games)
	end
	-- (another game in front: its own search)
	if (games[1] and games[1].pid) ~= before then
		query, found, page, wanted = "", {}, nil, nil
	end
end

local function merge(out, d)
	for k, v in d do
		out[k] = v
	end
	return out
end

function handle(ev)
	if ev.event == "start" then
		games, probe, query, found, page, wanted = {}, nil, "", {}, nil, nil
		wikis = (ev.store and ev.store.wikis) or {}
		-- (one not found last time is looked for again: wikis come, and the looking gets better)
		for path, base in wikis do
			if base == false then
				wikis[path] = nil
			end
		end
		size = math.clamp(tonumber(ev.store and ev.store.size) or 1, 1, #SIZES)
		return nil
	elseif ev.event == "processes" then
		see_windows(ev)
		local out = draw(ev.now)
		find_wiki(out)
		return out
	elseif ev.event == "usage" then
		for _, u in ev.usage or {} do
			for _, g in games do
				if g.pid == u.pid then
					g.usage = u
				end
			end
		end
		return draw(ev.now)
	elseif ev.event == "gear" then
		gear = { controllers = ev.controllers or {}, devices = ev.devices or {} }
		return draw(ev.now)
	elseif ev.event == "fetched" then
		local out: any = {}
		local data = nadi.json.decode(ev.body or "")
		local ok = ev.status == 200 and type(data) == "table"
		if ev.id == "probe" and probe then
			local general = ok and data.query and data.query.general
			if general and general.sitename then
				wikis[probe.path] = probe.tries[probe.at]
				out.store = kept()
				probe = nil
				if wanted then
					local q = wanted
					wanted = nil
					search(out, q)
				end
			else
				-- (a name that is another's: Fandom points to the wiki's own, as "nte" to "neverness-to-everness"; that is tried next)
				local moved = ev.location and string.match(ev.location, "^(https://[%w%-]+%.fandom%.com)/")
					or ev.location and string.match(ev.location, "^(https://[%w%-]+%.wiki%.gg)/")
				if moved and not table.find(probe.tries, moved) then
					table.insert(probe.tries, probe.at + 1, moved)
				end
				probe_next(out)
			end
		elseif ev.id == "search" then
			found = {}
			for _, r in (ok and data.query and data.query.search) or {} do
				table.insert(found, r.title)
			end
			if found[1] then
				show(out, found[1])
			else
				page = nil
			end
		elseif (ev.id == "page" or ev.id == "sec" or ev.id == "sec0") and page then
			local html = ok and data.parse and data.parse.text
			if type(html) == "table" then
				html = html["*"]
			end
			if type(html) == "string" then
				-- (the overview: its first paragraph; a section: all it says, its lists too)
				page.text = if ev.id == "sec" then section_words(html) else first_words(html)
				-- (a section: its tables, when it has any; the overview has its facts)
				page.tables = if ev.id == "sec" then tables_of(html, page.base) else {}
				if ev.id == "page" then
					page.facts = infobox(html)
					page.image = picture_of(html, page.base)
				end
			end
			if ev.id == "page" and ok and data.parse and data.parse.title then
				page.title = data.parse.title
			end
		elseif ev.id == "secs" and page then
			page.sections = {}
			for _, s in (ok and data.parse and data.parse.sections) or {} do
				if s.toclevel == 1 and (s.index or "") ~= "" and #page.sections < 4 then
					table.insert(page.sections, { name = plain(s.line or ""), index = tostring(s.index) })
				end
			end
		end
		return merge(out, draw(ev.now))
	elseif ev.event == "action" then
		local a = ev.action or ""
		local out: any = {}
		if a == "search" then
			search(out, (ev.args and ev.args.text) or "")
		elseif a == "size" then
			size = size % #SIZES + 1
			out.store = kept()
		elseif a == "open" then
			if page and page.url then
				out.open = page.url
			end
		elseif string.match(a, "^sec:") and page and page.base then
			-- (a section: its first lines in place; the overview, the page's own)
			page.section = string.sub(a, 5)
			out.fetch = { { id = if page.section == "0" then "sec0" else "sec", url = api(page.base, "action=parse&prop=text&redirects=1&disablelimitreport=1&disableeditsection=1&section=" .. page.section .. "&page=" .. enc(page.title)), max_kb = 1024 } }
		else
			local i = tonumber(string.match(a, "^pick:(%d+)$"))
			if i and found[i] then
				show(out, found[i])
			end
		end
		return merge(out, draw(ev.now))
	end
	return nil
end
