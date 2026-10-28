--!strict
-- Page Reader: what you are browsing. A section for each page you have open that is of a kind worth one (a walkthrough, a post, an
-- article, a recipe, a question and its answers, an API page, a product...): a page of no known kind has none, and a video has the
-- media widget. It needs the NADI browser extension (the `nadi-chrome` repository): the page describes itself, exactly, and costs the
-- browser next to nothing. The plugin says which sites are never read and which buttons are safe to press (the extension reads nothing
-- until it is told), keeps the pages that have a section, and asks NADI what kind each is (`nadi.page_read`).
--
-- Private windows, blocked sites (banking, mail, health) and pages the extension did not describe (a sign-in page) are never shown.

local CARDS = 8 -- pages with a section, across all browsers
local TTL_MS = 180000 -- a page not in front for this long drops out
local FRESH_MS = 30000 -- shown while a browser window was seen this recently
local PRUNE_MS = 5000
local PRUNE_GRACE_MS = 10000
local BROWSERS = { chrome = true, msedge = true, firefox = true, brave = true, opera = true, vivaldi = true, arc = true }
local NAMES = { msedge = "Edge", chrome = "Chrome", brave = "Brave", opera = "Opera", vivaldi = "Vivaldi", firefox = "Firefox", arc = "Arc" }
local GLYPH = { walkthrough = "steps", wiki = "book", news = "news", video = "play", recipe = "pot", qna = "ask", api = "code", product = "tag", search = "search", webapp = "branch" }
local NEW_TAB = { ["new tab"] = true, ["new tab page"] = true, ["start page"] = true, home = true, ["about:blank"] = true, [""] = true }

local mru = {} -- the pages, the one last in front first: { id, browser, title, path, app, last_seen, info }
local next_id = 0
local private = false
local seen: number? = nil
local windows = {} -- the browsers that have a window now
local browsers = {} -- the browsers with the extension connected
local last_prune = 0
local absent = {} -- "browser|title" -> since when it has been missing from the tab lists
local told: boolean? = nil

local function stem(exe: string): string
	local f = string.lower(string.match(exe, "([^\\/]+)$") or exe)
	return (string.gsub(f, "%.exe$", ""))
end

-- "Rust docs - Google Chrome" -> "Rust docs" (and the profile's name and Edge's tab count go too)
local function clean_title(title: string): string
	local lower = string.lower(title)
	local cut = #title + 1
	for _, marker in { " - google chrome", " - microsoft edge", " - microsoft\u{200b} edge", " - brave", " - vivaldi", " - opera", " \u{2014} mozilla firefox", " - mozilla firefox" } do
		local i = string.find(lower, marker, 1, true)
		if i and i < cut then
			cut = i
		end
	end
	local t = string.match(string.sub(title, 1, cut - 1), "^%s*(.-)%s*$") or ""
	local pre, profile = string.match(t, "^(.*) %- ([^%-]+)$")
	if pre then
		local pl = string.lower(string.match(profile, "^%s*(.-)%s*$") or "")
		if pl == "personal" or pl == "work" or pl == "school" or pl == "default" or pl == "guest" or string.sub(pl, 1, 8) == "profile " then
			t = pre
		end
	end
	local head = string.match(t, "^(.*) and %d+ more")
	if head then
		t = head
	end
	return t
end

local function key(b: string, t: string): string
	return b .. "|" .. t
end

local function push(browser: string, title: string, path: string, app: string, now: number)
	for i, e in mru do
		if e.browser == browser and e.title == title then
			table.remove(mru, i)
			e.last_seen = now
			if app ~= "" then
				e.app = app
			end
			if path ~= "" then
				e.path = path
			end
			table.insert(mru, 1, e)
			return
		end
	end
	next_id += 1
	table.insert(mru, 1, { id = next_id, browser = browser, title = title, path = path, app = app, last_seen = now })
	while #mru > CARDS do
		table.remove(mru)
	end
end

-- the open tabs the extension read have sections too, behind the ones that were in front
local function seed(browser: string, titles, now: number)
	local path, app = "", ""
	for _, e in mru do
		if e.browser == browser then
			path, app = e.path, e.app
			break
		end
	end
	for _, t in titles do
		local found = false
		for _, e in mru do
			if e.browser == browser and e.title == t then
				e.last_seen = now
				found = true
			end
		end
		if not found and #mru < CARDS and not NEW_TAB[string.lower(t)] then
			next_id += 1
			table.insert(mru, { id = next_id, browser = browser, title = t, path = path, app = app, last_seen = now })
		end
	end
end

-- a tab went to another page of its guide: its section follows it
local function rename(browser: string, old: string, new: string)
	local i = 1
	while i <= #mru do
		local e = mru[i]
		if e.browser == browser and e.title == old then
			local dup = false
			for _, x in mru do
				if x.browser == browser and x.title == new then
					dup = true
				end
			end
			if dup then
				table.remove(mru, i)
				continue
			end
			e.title = new
		end
		i += 1
	end
end

local function observe(list, now: number)
	local any = false
	windows = {}
	local front = nil
	for i = #list, 1, -1 do
		local w = list[i]
		if BROWSERS[stem(w.exe or "")] then
			any = true
			local b = stem(w.exe)
			if not table.find(windows, b) then
				table.insert(windows, b)
			end
			local cleaned = clean_title(w.title or "")
			if not nadi.page_private(w.title or "") and not NEW_TAB[string.lower(cleaned)] then
				push(b, cleaned, w.path or "", if (w.name or "") ~= "" then w.name else b, now)
			end
			if w.front then
				front = w
			end
		end
	end
	if front then
		private = nadi.page_private(front.title or "")
	end
	if any then
		seen = now
	end
end

-- closed tabs drop out: a page no tab list names any more is gone (after a grace: titles flicker while a page loads)
local function prune(feed, now: number)
	if now - last_prune < PRUNE_MS or #windows == 0 then
		return
	end
	last_prune = now
	local keep = {}
	local alive = {}
	for _, e in mru do
		local k = key(e.browser, e.title)
		local open
		if not table.find(windows, e.browser) then
			open = false
		else
			local tabs = (feed.tabs or {})[e.browser]
			if tabs and #tabs > 0 then
				open = table.find(tabs, string.lower(e.title)) ~= nil
			else
				open = true
			end
		end
		alive[k] = true
		if open then
			absent[k] = nil
			table.insert(keep, e)
		elseif absent[k] and now - absent[k] >= PRUNE_GRACE_MS then
			absent[k] = nil
		else
			absent[k] = absent[k] or now
			table.insert(keep, e)
		end
	end
	for k in absent do
		if not alive[k] then
			absent[k] = nil
		end
	end
	mru = keep
end

local function read_page(p, now: number)
	for _, e in mru do
		if e.browser == p.browser and e.title == p.title then
			if NEW_TAB[string.lower(p.title or "")] then
				e.info = nil
				return
			end
			local r = nadi.page_read(p, now)
			e.info = if r.blocked then nil else r
			return
		end
	end
end

local function section_of(e)
	local info = e.info
	local k = info.kind
	local domain = info.domain or ""
	local body: { any } = {}
	if k.fields and #k.fields > 0 then
		local rows = {}
		for _, f in k.fields do
			table.insert(rows, { f.key, f.value })
		end
		table.insert(body, { kind = "facts", rows = rows })
	end
	local has_summary = false
	for _, f in k.fields or {} do
		if f.key == "Summary" or f.key == "Gist" then
			has_summary = true
		end
	end
	if info.lead and not has_summary then
		table.insert(body, { kind = "text", text = info.lead })
	end
	if info.buttons and #info.buttons > 0 then
		local items = {}
		for _, b in info.buttons do
			table.insert(items, { label = b, action = "press:" .. b, args = { card = "p" .. e.id } })
		end
		table.insert(body, { kind = "buttons", items = items })
	end
	return {
		kind = "section",
		id = "p" .. e.id,
		title = e.title,
		sub = if domain ~= "" then k.label .. ", " .. domain else k.label,
		tile = { icon = GLYPH[k.id] or "globe", color = "#4f8ff0" },
		body = body,
		press = "focus:p" .. e.id,
	}
end

local function render(ev, extra)
	local now = ev.now
	local sections = {}
	if seen and now - seen <= FRESH_MS then
		for _, e in mru do
			if #sections >= CARDS then
				break
			end
			if now - e.last_seen <= TTL_MS and e.info and e.info.kind and e.info.kind.id ~= "video" then
				table.insert(sections, section_of(e))
			end
		end
	end
	local watch = {}
	if not private then
		for _, e in mru do
			table.insert(watch, { e.browser, e.title })
		end
	end
	local ask: any = { watch = watch }
	local images = ev.settings.images ~= false
	if told ~= images then
		told = images
		ask.config = nadi.page_config(images)
	end
	if extra then
		for k, v in extra do
			ask[k] = v
		end
	end
	local note
	if #browsers == 0 then
		note = "Waiting for the browser extension (without it, pages are not read)."
	else
		local names = {}
		for _, b in browsers do
			table.insert(names, NAMES[b] or b)
		end
		note = "Reading pages in " .. table.concat(names, ", ") .. "."
	end
	local widget = {}
	if #sections > 0 then
		local first = sections[1]
		widget = {
			peek = { icon = first.tile.icon, color = "#4f8ff0", text = if #sections > 1 then tostring(#sections) .. " pages" else first.sub, label = first.title },
			panel = { body = sections },
			tip = first.title,
		}
	end
	return { widgets = { pages = widget }, browser = ask, note = note }
end

local function find_card(card: string?)
	for _, e in mru do
		if card and "p" .. e.id == card then
			return e
		end
	end
	return mru[1]
end

function handle(ev)
	local e = ev.event
	if e == "start" then
		mru, next_id, private, seen, windows, browsers, last_prune, absent, told = {}, 0, false, nil, {}, {}, 0, {}, nil
		return render(ev)
	elseif e == "processes" then
		observe(ev.windows or {}, ev.now)
		return render(ev)
	elseif e == "browser" then
		local feed = ev.browser or {}
		browsers = feed.browsers or {}
		for _, m in feed.moves or {} do
			rename(m[1], m[2], m[3])
		end
		if not private then
			for _, b in windows do
				local list = (feed.open or {})[b]
				if list then
					seed(b, list, ev.now)
				end
			end
		end
		prune(feed, ev.now)
		if not private then
			for _, p in feed.pages or {} do
				read_page(p, ev.now)
			end
		end
		return render(ev)
	elseif e == "action" then
		local a = ev.action or ""
		local card = string.match(a, "^focus:(.+)$") or (ev.args and ev.args.card)
		local p = find_card(card)
		if not p then
			return nil
		end
		if string.match(a, "^focus:") then
			local out = render(ev)
			out.focus = { exe = if p.path ~= "" then p.path else nil, title = p.title }
			return out
		end
		local label = string.match(a, "^press:(.+)$")
		if label then
			return render(ev, { press = { id = "press", browser = p.browser, label = label } })
		end
	end
	return nil
end
