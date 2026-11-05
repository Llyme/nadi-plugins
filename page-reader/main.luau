--!strict
-- NADI Chrome: what you are reading, with the NADI Chrome browser extension (the `nadi-chrome` repository). The page in front gets a
-- card made for its kind: a walkthrough or a recipe followed step by step (the step you are on read out, the browser scrolled to the
-- one before or after), an article's minutes left, a question's answer, a doc's signature. At rest: the one thing
-- that matters of it, in a ring of how far through it you are. The other pages, a line each, under the card.
--
-- The page describes itself, exactly, and costs the browser next to nothing. Its own program (bin/page-helper.exe) holds the link to
-- the extension and reads each page it describes (what kind it is, what is worth saying of it: `read`; a guide's words in full; its
-- picture, kept as a file). The plugin tells it which pages to follow, which sites are never read and that pictures may come (the
-- extension reads nothing until it is told), and keeps the pages that have a card. Private windows, blocked sites (banking, mail,
-- health), shops' pages and pages the extension did not describe (a sign-in page) are never shown; a video stays with Now Playing.
local CARDS = 8 -- pages with a section, across all browsers
local TTL_MS = 180000 -- a page not in front for this long drops out
local FRESH_MS = 30000 -- shown while a browser window was seen this recently
local PRUNE_MS = 5000
local PRUNE_GRACE_MS = 10000
local BROWSERS = { chrome = true, msedge = true, firefox = true, brave = true, opera = true, vivaldi = true, arc = true }
local NAMES = { msedge = "Edge", chrome = "Chrome", brave = "Brave", opera = "Opera", vivaldi = "Vivaldi", firefox = "Firefox", arc = "Arc" }
local GLYPH = { walkthrough = "steps", wiki = "book", news = "news", video = "play", recipe = "pot", qna = "ask", api = "code", webapp = "branch" }
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

-- a private window's title says so (InPrivate, Incognito, Private Browsing): never followed
local function private_title(t: string): boolean
	local s = string.lower(t)
	for _, w in { "inprivate", "incognito", "private browsing", "private window" } do
		if string.find(s, w, 1, true) then
			return true
		end
	end
	return false
end
local pinned: string? = nil -- a page pressed in the list: its card on top
local settings_images = true
local zoom = 1.0 -- how big its words are, as you set it (kept)
local ZOOM_MIN, ZOOM_MAX = 0.8, 1.6

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
			if not private_title(w.title or "") and not NEW_TAB[string.lower(cleaned)] then
				push(b, cleaned, w.path or "", if (w.name or "") ~= "" then w.name else b, now)
			end
			if w.front then
				front = w
			end
		end
	end
	if front then
		private = private_title(front.title or "")
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
			local r = p.read or { blocked = true }
			e.info = if r.blocked then nil else r
			-- (what the card follows a guide by: the headings, paragraphs and items, where they are now)
			local nodes = {}
			for _, n in p.nodes or {} do
				if n.k == "h" or n.k == "p" or n.k == "li" then
					table.insert(nodes, { k = n.k, l = n.l, t = n.t or "", y = n.y or 0, h = n.h or 0 })
				end
			end
			table.sort(nodes, function(a, b)
				return a.y < b.y
			end)
			e.nodes, e.vh = nodes, p.vh or 800
			-- (a guide the extension read whole: its words, fetched once for each version of them)
			local key = p.guide and p.guide.key
			if key and key ~= e.guide_key then
				e.guide_key, e.guide = key, p.guide
			elseif not key then
				e.guide_key, e.guide = nil, nil
			end
			-- (the page read again where it now is: the step asked for is where it is)
			e.asked = nil
			return
		end
	end
end

-- ------------------------------------------------------------------------------------------------------------- a guide's steps

local SKIP_HEADS = { contents = true, ["table of contents"] = true, navigation = true, ["navigation menu"] = true, comments = true, ["related articles"] = true, ["see also"] = true, references = true }

local function step_number(t: string): number?
	local n = string.match(t, "^[Ss]tep%s*(%d+)") or string.match(t, "^(%d+)[%.%):]%s") or string.match(t, "^[Pp]art%s+(%d+)") or string.match(t, "^[Cc]hapter%s+(%d+)")
	return tonumber(n)
end

-- a guide's words without the extension's picture markers and its bold marks
local MARK_OPEN, MARK_SHUT = utf8.char(0xE000), utf8.char(0xE001)
local function plain(t: string): string
	local out = t
	while true do
		local i = string.find(out, MARK_OPEN, 1, true)
		if not i then
			break
		end
		local j = string.find(out, MARK_SHUT, i, true)
		out = string.sub(out, 1, i - 1) .. (if j then string.sub(out, j + #MARK_SHUT) else "")
	end
	out = string.gsub(out, "%*%*", "")
	return (string.match(out, "^%s*(.-)%s*$") or "")
end

local function has_words(blocks, from: number, to: number): boolean
	for i = from, to do
		local b = blocks[i]
		if b and b[1] ~= "h" then
			return true
		end
	end
	return false
end

-- the steps of a guide the extension read whole: its headings of the level that repeats most shallowly ("8/9", "8/10"), each with
-- the blocks under it; the words before the first, a step of their own
local function guide_steps(e): { any }?
	local blocks = e.guide and e.guide.blocks
	if type(blocks) ~= "table" then
		return nil
	end
	local main = string.lower((e.info and e.info.kind and e.info.kind.main) or "")
	local counts = {}
	for _, b in blocks do
		if b[1] == "h" and string.lower(plain(b[3] or "")) ~= main then
			counts[b[2]] = (counts[b[2]] or 0) + 1
		end
	end
	local lvl = nil
	for l = 2, 6 do
		if (counts[l] or 0) >= 2 then
			lvl = l
			break
		end
	end
	if not lvl then
		-- (no headings that repeat: the whole guide, read out as one)
		return if #blocks >= 2 then { { t = "", at = 0, from = 1, to = #blocks, y = -math.huge } } else nil
	end
	local steps = {}
	for i, b in blocks do
		local t = if b[1] == "h" then plain(b[3] or "") else ""
		if b[1] == "h" and b[2] == lvl and t ~= "" and string.lower(t) ~= main and not SKIP_HEADS[string.lower(t)] then
			table.insert(steps, { t = t, at = i, from = i + 1 })
		end
	end
	if #steps < 2 then
		return { { t = "", at = 0, from = 1, to = #blocks, y = -math.huge } }
	end
	for i, s in steps do
		s.to = if steps[i + 1] then steps[i + 1].at - 1 else #blocks
	end
	if steps[1].at > 1 and has_words(blocks, 1, steps[1].at - 1) then
		table.insert(steps, 1, { t = "", at = 0, from = 1, to = steps[1].at - 1, y = -math.huge })
	end
	-- (where each is on the page now: its heading among what the extension saw)
	for _, s in steps do
		if s.t ~= "" then
			local want = string.lower(s.t)
			for _, n in e.nodes or {} do
				if n.k == "h" and string.lower(n.t) == want then
					s.y = n.y
					break
				end
			end
		end
	end
	return steps
end

-- the steps of a guide, in order: the guide's own when the extension read it whole; else headings that are numbered ("Step 3",
-- "4. Drain the cistern"), else its sections
local function steps_of(e): { any }
	local g = guide_steps(e)
	if g then
		return g
	end
	local numbered, sections = {}, {}
	for _, n in e.nodes or {} do
		-- (the page's own title is no step)
		if (n.k == "h" and (n.l or 2) >= 2) or (n.k == "li" and #n.t < 80) then
			local num = step_number(n.t)
			if num and (n.k == "h" or num <= 60) then
				table.insert(numbered, n)
			elseif n.k == "h" and (n.l == 2 or n.l == 3) and not SKIP_HEADS[string.lower(n.t)] and #n.t < 90 then
				table.insert(sections, n)
			end
		end
	end
	return if #numbered >= 2 then numbered elseif #sections >= 3 then sections else {}
end

-- which step is at the top of the view (1-based; the one asked for, while the page has not been read again)
local function step_at(e, steps): number
	if e.asked and steps[e.asked] then
		return e.asked
	end
	local line = (e.vh or 800) * 0.35
	local at = 1
	for i, s in steps do
		if s.y and s.y <= line then
			at = i
		end
	end
	return at
end

local STEP_CHARS = 6000 -- the most of a step read out

-- what a step says, as blocks: its sub-headings, paragraphs and lists, whole (the panel scrolls)
local function step_blocks(e, steps, i: number): { any }
	local s = steps[i]
	local out = {}
	if s.from and e.guide and e.guide.blocks then
		local used = 0
		for k = s.from, s.to do
			local b = e.guide.blocks[k]
			if used > STEP_CHARS or not b then
				break
			end
			local kind = b[1]
			local text = nil
			if kind == "h" then
				local t = plain(b[3] or "")
				if t ~= "" then
					table.insert(out, { kind = "step", head = t })
					used += #t
				end
			elseif kind == "p" then
				text = plain(b[2] or "")
			elseif kind == "ul" or kind == "ol" then
				local lines = {}
				for n, item in b[2] or {} do
					table.insert(lines, (if kind == "ol" then n .. ". " else "• ") .. plain(item))
				end
				text = table.concat(lines, "\n")
			elseif kind == "tbl" then
				local rows = {}
				for _, row in b[2] or {} do
					local cells = {}
					for _, c in row do
						table.insert(cells, plain(c))
					end
					table.insert(rows, cells)
					used += 20
				end
				if #rows > 0 then
					table.insert(out, { kind = "table", rows = rows, head = #rows > 1 })
				end
			elseif kind == "facts" then
				local lines = {}
				for _, f in b[2] or {} do
					table.insert(lines, plain(f[1] or "") .. ": " .. plain(f[2] or ""))
				end
				text = table.concat(lines, "\n")
			end
			if text and text ~= "" then
				table.insert(out, { kind = "text", text = text, lines = 200 })
				used += #text
			end
		end
		return out
	end
	-- (no guide: what lies between its heading and the next one on the page)
	local from = s.y or 0
	local to = if steps[i + 1] and steps[i + 1].y then steps[i + 1].y else math.huge
	local words = {}
	for _, n in e.nodes or {} do
		if n.y > from and n.y < to and n.k ~= "h" and n.t ~= "" then
			table.insert(words, n.t)
		end
	end
	if #words > 0 then
		table.insert(out, { kind = "text", text = table.concat(words, "\n"), lines = 200 })
	end
	return out
end

-- how far down the page you are: the share of its words above the view
local function read_part(e): number?
	local above, all = 0, 0
	for _, n in e.nodes or {} do
		all += #n.t
		if n.y + n.h < 0 then
			above += #n.t
		end
	end
	return if all > 200 then above / all else nil
end

-- ------------------------------------------------------------------------------------------------------------- the cards

local function field(k, key: string): string?
	for _, f in k.fields or {} do
		if f.key == key then
			return (string.gsub(f.value, " \u{b7} ", ", "))
		end
	end
	return nil
end

local function first_words(e, after_y: number, min_len: number): string?
	for _, n in e.nodes or {} do
		if n.k == "p" and n.y > after_y and #n.t >= min_len then
			return n.t
		end
	end
	return nil
end

-- the one thing worth saying of a page at rest, and how far through it you are
local function fact_of(e): (string?, number?)
	local k = e.info.kind
	local id = k.id
	if e.guide or id == "walkthrough" or id == "recipe" then
		local steps = steps_of(e)
		if #steps >= 2 then
			local at = step_at(e, steps)
			return at .. " / " .. #steps, at / #steps
		end
	end
	local part = read_part(e)
	if id == "news" then
		local mins = tonumber(string.match(field(k, "Read time") or "", "(%d+)"))
		if mins then
			return math.max(1, math.ceil(mins * (1 - (part or 0)))) .. " min", part
		end
	elseif id == "qna" then
		return field(k, "Answers"), nil
	elseif id == "recipe" then
		return field(k, "Time"), part
	elseif id == "webapp" then
		return field(k, "State"), nil
	end
	return nil, part
end

local function card_of(e)
	local info = e.info
	local k = info.kind
	local id = k.id
	local cid = "p" .. e.id
	local body: { any } = {}
	local fact, part = fact_of(e)
	local steps = if e.guide or id == "walkthrough" or id == "recipe" then steps_of(e) else {}
	local guide_pages = (e.guide and { prev = e.guide.prev, next = e.guide.next }) or {}
	local site: any = { kind = "site", icon = GLYPH[id] or "globe", text = info.domain or e.browser }
	if #steps >= 2 then
		site.chip = { icon = "steps", text = fact }
	elseif part and (id == "news" or id == "wiki") then
		site.ring = part
	elseif fact then
		site.chip = { text = fact }
	end
	table.insert(body, site)
	if info.image and settings_images then
		table.insert(body, { kind = "picture", image = "img:" .. tostring(info.image), height = if id == "recipe" then 110 else 90 })
	end
	table.insert(body, { kind = "title", text = if k.main and k.main ~= "" then k.main else e.title, action = "focus:" .. cid })
	if #steps >= 2 or (#steps == 1 and e.guide) then
		local at = step_at(e, steps)
		if #steps >= 2 then
			table.insert(body, { kind = "segs", n = #steps, at = at, action = "go:" .. cid .. ":" })
		end
		if steps[at].t ~= "" then
			table.insert(body, { kind = "step", head = steps[at].t })
		end
		for _, b in step_blocks(e, steps, at) do
			table.insert(body, b)
		end
		-- (past its first or last step: the guide's page before or after, on the same site)
		local prev = if at > 1 then "go:" .. cid .. ":" .. (at - 1) elseif guide_pages.prev then "page:prev:" .. cid else nil
		local next = if at < #steps then "go:" .. cid .. ":" .. (at + 1) elseif guide_pages.next then "page:next:" .. cid else nil
		local label = if steps[at + 1] then steps[at + 1].t elseif guide_pages.next then guide_pages.next.t or "" else ""
		if prev or next then
			table.insert(body, { kind = "stepnav", prev = prev, next = next, label = label })
		end
	elseif id == "news" or id == "wiki" then
		local words = field(k, "Gist") or field(k, "Summary") or info.lead
		if words then
			table.insert(body, { kind = "text", text = words, tone = "muted", lines = 3 })
		end
		local chips = {}
		if fact and id == "news" then
			table.insert(chips, { icon = "timer", text = fact })
		end
		local by = field(k, "Byline")
		if by then
			table.insert(chips, { icon = "pen", text = by })
		end
		-- (a wiki: the section you are in)
		if id == "wiki" then
			local sec = nil
			for _, n in e.nodes or {} do
				if n.k == "h" and (n.l == 2 or n.l == 3) and n.y <= (e.vh or 800) * 0.35 then
					sec = n.t
				end
			end
			if sec then
				table.insert(chips, { icon = "book", text = sec })
			end
		end
		if #chips > 0 then
			table.insert(body, { kind = "chips", items = chips })
		end
	elseif id == "qna" then
		local ans_y = 0
		for _, n in e.nodes or {} do
			if n.k == "h" and string.find(string.lower(n.t), "answer") then
				ans_y = n.y
				break
			end
		end
		local words = if ans_y > 0 then first_words(e, ans_y, 40) else nil
		if words then
			table.insert(body, { kind = "answer", votes = field(k, "Answers") or "", accepted = field(k, "Accepted") ~= nil, text = words })
		end
	elseif id == "api" then
		local path = field(k, "Path")
		if path then
			table.insert(body, { kind = "text", text = path, tone = "muted", lines = 1 })
		end
		local words = field(k, "Returns") or info.lead
		if words then
			table.insert(body, { kind = "text", text = words, lines = 3 })
		end
	elseif id == "webapp" then
		local chips = {}
		local st = field(k, "State")
		if st then
			table.insert(chips, { icon = "dot", text = st, tone = "ok" })
		end
		local checks = field(k, "Checks")
		if checks then
			table.insert(chips, { icon = "check", text = checks })
		end
		if #chips > 0 then
			table.insert(body, { kind = "chips", items = chips })
		end
	elseif id == "recipe" then
		local chips = {}
		local t = field(k, "Time")
		if t then
			table.insert(chips, { icon = "timer", text = t })
		end
		local n = field(k, "Ingredients")
		if n then
			table.insert(chips, { icon = "pot", text = n })
		end
		if #chips > 0 then
			table.insert(body, { kind = "chips", items = chips })
		end
	end
	-- (the page's own buttons that are safe to press: Next page, Show more)
	if info.buttons and #info.buttons > 0 then
		local items = {}
		for _, b in info.buttons do
			table.insert(items, { text = b, action = "press:" .. b, args = { card = cid } })
		end
		table.insert(body, { kind = "chips", items = items })
	end
	return { kind = "card", body = body }
end

local function row_of(e)
	local fact = fact_of(e)
	return { icon = GLYPH[e.info.kind.id] or "globe", title = if e.info.kind.main and e.info.kind.main ~= "" then e.info.kind.main else e.title, right = fact or (e.info.domain or ""), action = "show:p" .. e.id }
end

local function shown_pages(now: number)
	local list = {}
	if seen and now - seen <= FRESH_MS then
		for _, e in mru do
			if #list >= CARDS then
				break
			end
			-- (a video has its own widget; search results say nothing a page of them does not; a shop's page is an ad: none is shown)
			if now - e.last_seen <= TTL_MS and e.info and e.info.kind and e.info.kind.id ~= "video" and e.info.kind.id ~= "search" and e.info.kind.id ~= "product" then
				table.insert(list, e)
			end
		end
	end
	-- (one pressed: on top, while it is still here)
	if pinned then
		for i, e in list do
			if "p" .. e.id == pinned and i > 1 then
				table.remove(list, i)
				table.insert(list, 1, e)
				break
			end
		end
	end
	return list
end

local function render(ev, extra)
	local now = ev.now
	settings_images = ev.settings.images ~= false
	local list = shown_pages(now)
	local watch = {}
	if not private then
		for _, e in mru do
			table.insert(watch, { e.browser, e.title })
		end
	end
	-- (what its program is asked: the pages to follow; what the extension may read, once and when it changes; a button, a tab, a
	-- heading)
	local ask: { any } = { { cmd = "watch", pages = watch } }
	local images = settings_images
	if told ~= images then
		told = images
		table.insert(ask, { cmd = "config", images = images })
	end
	for op, v in extra or {} do
		local c: any = { cmd = op }
		for k, x in v do
			c[k] = x
		end
		table.insert(ask, c)
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
	if #list > 0 then
		local first = list[1]
		local fact, part = fact_of(first)
		-- (on top: a button for each page, the one shown lit; its words smaller or bigger, a button each, only while it can go further)
		local tools: { any } = {}
		if #list > 1 then
			for i, e in list do
				if i > 6 then
					break
				end
				local nm = if e.info.kind.main and e.info.kind.main ~= "" then e.info.kind.main else e.title
				table.insert(tools, { kind = "icon", icon = GLYPH[e.info.kind.id] or "globe", action = "show:p" .. e.id, on = i == 1, tip = nm })
			end
		end
		table.insert(tools, { kind = "grow" })
		if zoom > ZOOM_MIN + 0.01 then
			table.insert(tools, { kind = "icon", icon = "minus", action = "zoom:-", tip = "Smaller" })
		end
		table.insert(tools, { kind = "text", text = math.floor(zoom * 100 + 0.5) .. "%" })
		if zoom < ZOOM_MAX - 0.01 then
			table.insert(tools, { kind = "icon", icon = "plus", action = "zoom:+", tip = "Bigger" })
		end
		local body: { any } = { { kind = "toolbar", items = tools }, card_of(first) }
		local name = if first.info.kind.main and first.info.kind.main ~= "" then first.info.kind.main else first.title
		widget = {
			peek = {
				ring = if part then { part = part, theme = true } else nil,
				icon = GLYPH[first.info.kind.id] or "globe",
				color = "#4f8ff0",
				text = fact or first.info.kind.label,
				label = name,
			},
			panel = { body = body, zoom = zoom },
			tip = name,
		}
	end
	return { widgets = { pages = widget }, helper = ask, note = note }
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
		mru, next_id, private, seen, windows, browsers, last_prune, absent, told, pinned = {}, 0, false, nil, {}, {}, 0, {}, nil, nil
		zoom = math.clamp(tonumber(ev.store and ev.store.zoom) or 1.0, ZOOM_MIN, ZOOM_MAX)
		return render(ev)
	elseif e == "processes" then
		observe(ev.windows or {}, ev.now)
		return render(ev)
	elseif e == "helper" and ev.data and ev.data.kind == "browser" then
		local feed = ev.data.browser or {}
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
		local z = string.match(a, "^zoom:([%+%-])$")
		if z then
			zoom = math.clamp(math.floor((zoom + (if z == "+" then 0.1 else -0.1)) * 10 + 0.5) / 10, ZOOM_MIN, ZOOM_MAX)
			local out = render(ev)
			out.store = { zoom = zoom }
			return out
		end
		local shown = string.match(a, "^show:(.+)$")
		if shown then
			pinned = shown
			return render(ev)
		end
		local dir, pcard = string.match(a, "^page:(%a+):(.+)$")
		if dir then
			local p = find_card(pcard)
			local to = p and p.guide and p.guide[dir]
			if p and to and to.u then
				return render(ev, { go = { id = "go", browser = p.browser, title = p.title, url = to.u } })
			end
			return nil
		end
		local gcard, gn = string.match(a, "^go:([^:]+):(%d+)$")
		if gcard then
			-- (a step asked for: the card shows it now, and the browser brings it to the top)
			local p = find_card(gcard)
			local steps = if p then steps_of(p) else {}
			local n = tonumber(gn) :: number
			if p and steps[n] then
				p.asked = n
				local main = (p.info and p.info.kind and p.info.kind.main) or p.title
				return render(ev, { scroll = { id = "scroll", browser = p.browser, title = p.title, heading = if steps[n].t ~= "" then steps[n].t else main } })
			end
			return nil
		end
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
