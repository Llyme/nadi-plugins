--!strict
-- Sticky Notes: notes on the island. At rest: a folded note in the newest one's colour and how many there are (or, one marked to
-- show at rest, its first line). Opened: a field to write a new one, and the wall of them, two columns, each in its colour; pressed,
-- one opens to be written (its title, its words, its checklist, its colour, pinned to the screen, shown at rest, deleted).
--
-- Pinned to the screen, a note is a window of its own: a widget of this plugin ("note:<id>", a `many` widget) with a panel and no
-- peek, asking NADI for its panel out (`panel.out = { open, remember, ground }`): it comes out on the monitor it was on, where it stood,
-- when NADI starts again. Its window closed (sent back), the note is unpinned.
--
-- Kept in this plugin's store (sealed for this Windows user). Connected to Notion (an integration's secret and a database, from the
-- plugin's settings), each note is a page of the database too: its title, a `Text` property with its words (a checklist as
-- "- [ ] item" lines) and a `Color` property. Looked at every minute (every 10 s while the panel is open), at once after an edit:
-- what changed in Notion comes here, what changed here goes there; changed on both sides, this one is kept and Notion's is kept
-- beside it as a copy.

type Item = { text: string, done: boolean }
type Note = {
	id: string,
	title: string,
	text: string,
	items: { Item }?,
	color: string,
	pinned: boolean,
	rest: boolean,
	created: number,
	updated: number,
	-- (in Notion: its page, what it said there last time (a signature), when it was last in step)
	notion: string?,
	sig: string?,
	synced: number?,
	deleted: boolean?,
}

local COLORS: { [string]: { string } } = {
	yellow = { "#2e2a17", "#f3e3a3" },
	green = { "#17291f", "#b7e8c9" },
	blue = { "#16233a", "#b9d2fb" },
	pink = { "#2d1a2a", "#f2c0e2" },
	purple = { "#221b33", "#d4c4fa" },
	gray = { "#1e1f24", "#d6d6dc" },
}
local ORDER = { "yellow", "green", "blue", "pink", "purple", "gray" }
local NAMES = { yellow = "Yellow", green = "Green", blue = "Blue", pink = "Pink", purple = "Purple", gray = "Gray" }
local NOTION_VERSION = "2022-06-28"
local SYNC_MS, SYNC_OPEN_MS = 60000, 10000

local notes: { Note } = {}
local editing: string? = nil
local seq = 0
-- (pinned notes whose windows were seen out: a window closed after that unpins its note)
local seen_out: { [string]: boolean } = {}
-- (the notes whose windows were asked for: one no longer pinned (or gone) has its window taken away)
local windows_out: { [string]: boolean } = {}
-- (Notion: its secret, its database, the name of its title property, and where syncing stands)
local notion: { token: string?, db: string?, title_prop: string?, ready: boolean? } = {}
local last_sync = -1e15
local syncing = false
local problem: string? = nil

local function kept()
	return { notes = notes, notion = notion }
end

local function find(id: string): (Note?, number?)
	for i, n in notes do
		if n.id == id then
			return n, i
		end
	end
	return nil, nil
end

local function shown_notes(): { Note }
	local out = {}
	for _, n in notes do
		if not n.deleted then
			table.insert(out, n)
		end
	end
	table.sort(out, function(a, b)
		return a.updated > b.updated
	end)
	return out
end

local function first_line(n: Note): string
	if n.title ~= "" then
		return n.title
	end
	local l = string.match(n.text, "^[^\n]*") or ""
	if l == "" and n.items and n.items[1] then
		l = n.items[1].text
	end
	return l
end

local function card_of(n: Note, with_badge: boolean, action: string?)
	local c = COLORS[n.color] or COLORS.yellow
	local items = nil
	if n.items then
		items = {}
		for i, it in n.items do
			table.insert(items, { text = it.text, state = if it.done then "done" else "todo", action = "tick", args = { id = n.id, i = i } })
		end
	end
	return {
		color = c[1],
		ink = c[2],
		title = if n.title ~= "" then n.title else nil,
		text = if n.text ~= "" then n.text else nil,
		items = items,
		badge = if with_badge and n.notion then "link" else nil,
		tip = if with_badge and n.notion then "In Notion" else nil,
		action = action,
		args = { id = n.id },
		menu = if action
			then {
				{ icon = "pin", label = if n.pinned then "Unpin From the Screen" else "Pin to the Screen", action = "pin", args = { id = n.id } },
				{ icon = "trash", label = "Delete", action = "ask_delete", args = { id = n.id } },
			}
			else nil,
	}
end

local function wall_body()
	local body: { any } = { { kind = "field", placeholder = "New Note", action = "add" } }
	local cards = {}
	for _, n in shown_notes() do
		table.insert(cards, card_of(n, true, "edit"))
	end
	if #cards > 0 then
		table.insert(body, { kind = "cards", cards = cards })
	end
	if problem then
		table.insert(body, { kind = "strip", text = problem, tone = "bad" })
	end
	return body
end

local function edit_body(n: Note)
	-- (the note itself, big, in its colour: its title and its words written where they are; its checklist ticked, added to)
	local card = card_of(n, false, nil)
	card.title = n.title
	card.text = n.text
	card.edit = { title = "title:" .. n.id, text = "text:" .. n.id, item = if n.items then "item:" .. n.id else nil }
	local colors = {}
	for _, k in ORDER do
		table.insert(colors, { color = COLORS[k][2], on = k == n.color, action = "color", args = { id = n.id, value = k }, tip = NAMES[k] })
	end
	return {
		{ kind = "cards", columns = 1, cards = { card } },
		{ kind = "colors", items = colors },
		{
			kind = "controls",
			items = {
				{ icon = "left", label = "Back", action = "back" },
				{ icon = "check", label = if n.items then "No Checklist" else "Checklist", action = "list", args = { id = n.id } },
				{ icon = "pin", label = if n.pinned then "Unpin From the Screen" else "Pin to the Screen", action = "pin", args = { id = n.id } },
				{ icon = "eye", label = if n.rest then "Not at Rest" else "Show at Rest", action = "rest", args = { id = n.id } },
				{ icon = "trash", label = "Delete", action = "ask_delete", args = { id = n.id } },
			},
		},
	}
end

local function draw(open: boolean)
	local list = shown_notes()
	local rest: Note? = nil
	for _, n in list do
		if n.rest then
			rest = n
			break
		end
	end
	local newest = list[1]
	local tint = if newest then (COLORS[newest.color] or COLORS.yellow)[2] else nil
	local peek: any = { icon = "note", color = tint }
	if rest then
		peek.text = first_line(rest)
	elseif #list > 0 then
		peek.text = tostring(#list)
	end
	local n = if editing then find(editing) else nil
	local widgets: { [string]: any } = {
		notes = {
			peek = peek,
			panel = { width = 400, body = if n then edit_body(n) else wall_body() },
			-- (words dragged onto it, or files: a note of them)
			drop = "drop",
			tip = if #list == 0 then "Sticky Notes" else nil,
		},
	}
	-- (each pinned note: its own window; one unpinned, or gone: its window taken away)
	local now_out: { [string]: boolean } = {}
	for _, m in notes do
		if m.pinned and not m.deleted then
			local c = COLORS[m.color] or COLORS.yellow
			widgets["note:" .. m.id] = {
				panel = { width = 240, body = { { kind = "cards", columns = 1, cards = { card_of(m, false, nil) } } }, out = { open = true, remember = true, ground = c[1] } },
			}
			now_out[m.id] = true
		end
	end
	for id in windows_out do
		if not now_out[id] then
			widgets["note:" .. id] = {}
			seen_out[id] = nil
		end
	end
	windows_out = now_out
	local _ = open
	return { widgets = widgets, store = kept() }
end

local function touch(n: Note, now: number)
	n.updated = math.max(now, n.updated + 1)
end

local function new_note(now: number, text: string): Note
	seq += 1
	local n: Note = { id = string.format("n%d_%d", now, seq), title = "", text = text, color = "yellow", pinned = false, rest = false, created = now, updated = now }
	table.insert(notes, n)
	return n
end

local function remove(n: Note)
	-- (in Notion: archived there first, then gone here)
	if n.notion then
		n.deleted = true
		n.pinned = false
		n.updated += 1
	else
		local _, i = find(n.id)
		if i then
			table.remove(notes, i)
		end
	end
	if editing == n.id then
		editing = nil
	end
end

-- ---------------------------------------------------------------------------------------------------- Notion

local function connected(): boolean
	return notion.token ~= nil and notion.db ~= nil
end

local function headers()
	return { Authorization = "Bearer " .. (notion.token or ""), ["Notion-Version"] = NOTION_VERSION, ["Content-Type"] = "application/json" }
end

-- (a database's id, from its link or as it is: 32 hex digits)
local function db_id(text: string): string?
	local s = string.gsub(text, "%-", "")
	return string.match(s, "(%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x)")
end

-- (a checklist in words: "- [ ] item" lines after the note's own words)
local function words_of(n: Note): string
	local out = n.text
	if n.items then
		local lines = {}
		for _, it in n.items do
			table.insert(lines, (if it.done then "- [x] " else "- [ ] ") .. it.text)
		end
		if #lines > 0 then
			out = (if out ~= "" then out .. "\n" else "") .. table.concat(lines, "\n")
		end
	end
	return string.sub(out, 1, 1990)
end

local function from_words(s: string): (string, { Item }?)
	local text, items = {}, {}
	local any = false
	for line in string.gmatch(s .. "\n", "([^\n]*)\n") do
		local mark, rest = string.match(line, "^%- %[([ xX])%] (.*)$")
		if mark then
			any = true
			table.insert(items, { text = rest, done = mark ~= " " })
		else
			table.insert(text, line)
		end
	end
	local t = table.concat(text, "\n")
	t = string.gsub(t, "\n+$", "")
	return t, if any then items else nil
end

local function sig(title: string, words: string, color: string): string
	return title .. "\0" .. words .. "\0" .. color
end

local function props_of(n: Note)
	local p: { [string]: any } = {}
	p[notion.title_prop or "Name"] = { title = { { text = { content = n.title } } } }
	p.Text = { rich_text = { { text = { content = words_of(n) } } } }
	p.Color = { select = { name = n.color } }
	return p
end

local function plain(list: any): string
	local out = {}
	for _, r in (if type(list) == "table" then list else {}) do
		table.insert(out, r.plain_text or (r.text and r.text.content) or "")
	end
	return table.concat(out)
end

-- What Notion has, made one with what is here.
local function merge(results: { any }, now: number)
	local by_page: { [string]: Note } = {}
	for _, n in notes do
		if n.notion then
			by_page[n.notion] = n
		end
	end
	local there: { [string]: boolean } = {}
	for _, page in results do
		local id = page.id
		if type(id) ~= "string" or page.archived then
			continue
		end
		there[id] = true
		local props = page.properties or {}
		local title = ""
		for _, v in props do
			if type(v) == "table" and v.type == "title" then
				title = plain(v.title)
			end
		end
		local words = plain(props.Text and props.Text.rich_text)
		local color = (props.Color and props.Color.select and props.Color.select.name) or "yellow"
		if not COLORS[color] then
			color = "yellow"
		end
		local s = sig(title, words, color)
		local n = by_page[id]
		if n then
			local remote_changed = s ~= n.sig
			local local_changed = n.updated > (n.synced or 0)
			if remote_changed and not local_changed and not n.deleted then
				local t, items = from_words(words)
				n.title, n.text, n.items, n.color = title, t, items, color
				n.sig, n.synced = s, now
				n.updated = now
			elseif remote_changed and local_changed and not n.deleted then
				-- (changed on both sides: this one goes there; Notion's is kept here as a copy beside it)
				local t, items = from_words(words)
				local copy = new_note(now, t)
				copy.title, copy.items, copy.color = title, items, color
				copy.updated = n.updated - 1
			end
		else
			local t, items = from_words(words)
			local m = new_note(now, t)
			m.title, m.items, m.color = title, items, color
			m.notion, m.sig, m.synced, m.updated = id, s, now, now
			m.created = nadi.iso(page.created_time or "") or now
		end
	end
	-- (gone from Notion: gone here too, unless changed here since)
	for i = #notes, 1, -1 do
		local n = notes[i]
		if n.notion and not there[n.notion] then
			if n.deleted or n.updated <= (n.synced or 0) then
				table.remove(notes, i)
			else
				n.notion, n.sig, n.synced = nil, nil, nil
			end
		end
	end
end

-- The next requests: what changed here sent to Notion (two at a time).
local function pushes(): { any }
	local out = {}
	for _, n in notes do
		if #out >= 2 then
			break
		end
		if n.deleted and n.notion then
			table.insert(out, { id = "push:" .. n.id, method = "PATCH", url = "https://api.notion.com/v1/pages/" .. n.notion, headers = headers(), body = nadi.json.encode({ archived = true }) })
		elseif not n.deleted and not n.notion then
			local body = { parent = { database_id = notion.db }, properties = props_of(n) }
			table.insert(out, { id = "push:" .. n.id, method = "POST", url = "https://api.notion.com/v1/pages", headers = headers(), body = nadi.json.encode(body) })
		elseif not n.deleted and n.updated > (n.synced or 0) then
			table.insert(out, { id = "push:" .. n.id, method = "PATCH", url = "https://api.notion.com/v1/pages/" .. n.notion, headers = headers(), body = nadi.json.encode({ properties = props_of(n) }) })
		end
	end
	return out
end

local function sync_start(now: number): { any }?
	if not connected() or syncing then
		return nil
	end
	syncing = true
	last_sync = now
	if not notion.ready then
		return { { id = "db", method = "GET", url = "https://api.notion.com/v1/databases/" .. (notion.db or ""), headers = headers() } }
	end
	return { { id = "query", method = "POST", url = "https://api.notion.com/v1/databases/" .. (notion.db or "") .. "/query", headers = headers(), body = nadi.json.encode({ page_size = 100 }) } }
end

local function said(status: number, body: string): string
	if status == 401 then
		return "Notion did not take the secret."
	elseif status == 404 then
		return "Notion: the database was not found, or not shared with the integration."
	elseif status == 0 then
		return "Notion could not be reached."
	end
	local ok, v = pcall(nadi.json.decode, body)
	return "Notion: " .. (if ok and type(v) == "table" and v.message then tostring(v.message) else ("error " .. status))
end

local function fetched(ev, now: number): { any }?
	local id: string = ev.id or ""
	local status: number = ev.status or 0
	local ok, v = pcall(nadi.json.decode, ev.body or "")
	local data: any = if ok then v else nil
	if status < 200 or status >= 300 then
		problem = said(status, ev.body or "")
		syncing = false
		return nil
	end
	if id == "db" then
		-- (its title property's name; Text and Color added when it has none)
		local props = (data and data.properties) or {}
		for name, p in props do
			if type(p) == "table" and p.type == "title" then
				notion.title_prop = name
			end
		end
		local add: { [string]: any } = {}
		if not props.Text then
			add.Text = { rich_text = {} }
		end
		if not props.Color then
			local options = {}
			for _, k in ORDER do
				table.insert(options, { name = k })
			end
			add.Color = { select = { options = options } }
		end
		if next(add) then
			return { { id = "dbprep", method = "PATCH", url = "https://api.notion.com/v1/databases/" .. (notion.db or ""), headers = headers(), body = nadi.json.encode({ properties = add }) } }
		end
		notion.ready = true
		syncing = false
		return sync_start(now)
	elseif id == "dbprep" then
		notion.ready = true
		syncing = false
		return sync_start(now)
	elseif id == "query" then
		problem = nil
		merge((data and data.results) or {}, now)
		local p = pushes()
		if #p == 0 then
			syncing = false
			return nil
		end
		return p
	elseif string.sub(id, 1, 5) == "push:" then
		local n, i = find(string.sub(id, 6))
		if n and i then
			if n.deleted then
				table.remove(notes, i)
			else
				if data and type(data.id) == "string" then
					n.notion = data.id
				end
				n.sig = sig(n.title, words_of(n), n.color)
				n.synced = n.updated
			end
		end
		local p = pushes()
		if #p == 0 then
			syncing = false
			return nil
		end
		return p
	end
	return nil
end

local function settings_note(): string?
	if not notion.token then
		return nil
	elseif not notion.db then
		return "Notion: pick its database (Notion Database)."
	end
	return problem or "In step with Notion."
end

-- ---------------------------------------------------------------------------------------------------- events

function handle(ev)
	local now: number = ev.now or 0
	local e = ev.event
	local open = type(ev.open) == "table" and ev.open.notes == true
	if e == "start" then
		local s = ev.store
		if type(s) == "table" then
			notes = if type(s.notes) == "table" then s.notes else {}
			notion = if type(s.notion) == "table" then s.notion else {}
			notion.ready = nil
		end
		editing, seen_out, windows_out, syncing, problem = nil, {}, {}, false, nil
		local out: any = draw(open)
		out.note = settings_note() or false
		out.fetch = sync_start(now)
		return out
	elseif e == "tick" then
		local every = if open then SYNC_OPEN_MS else SYNC_MS
		if connected() and not syncing and now - last_sync >= every then
			local out: any = draw(open)
			out.fetch = sync_start(now)
			return out
		end
		return nil
	elseif e == "fetched" then
		local f = fetched(ev, now)
		local out: any = draw(open)
		out.fetch = f
		out.note = settings_note() or false
		return out
	elseif e == "open" then
		-- (a pinned note's window: seen out; closed after that, its note unpinned)
		local w: string = ev.widget or ""
		local id = string.match(w, "^note:(.+)$")
		if id then
			if ev.open == true then
				seen_out[id] = true
			elseif seen_out[id] then
				seen_out[id] = nil
				local n = find(id)
				if n then
					n.pinned = false
				end
				return draw(open)
			end
			return nil
		end
		if w == "notes" and ev.open ~= true then
			editing = nil
			return draw(false)
		end
		return nil
	elseif e ~= "action" then
		return nil
	end
	local a: string = ev.action or ""
	local args = ev.args or {}
	local changed = false
	local out: any = nil
	local n: Note? = if type(args.id) == "string" then find(args.id) else nil
	local verb, target = string.match(a, "^(%a+):(.+)$")
	if verb and target then
		n = find(target)
		a = verb
	end
	local text: string = if type(args.text) == "string" then args.text else ""
	if a == "add" then
		if text ~= "" then
			new_note(now, text)
			changed = true
		end
	elseif a == "drop" then
		-- (words: a note of them; files: a note of their names)
		local words = text
		if words == "" and type(args.paths) == "table" then
			local names = {}
			for _, p in args.paths do
				table.insert(names, string.match(tostring(p), "[^\\/]+$") or tostring(p))
			end
			words = table.concat(names, "\n")
		end
		words = string.gsub(words, "^%s+", "")
		words = string.gsub(words, "%s+$", "")
		if words ~= "" then
			new_note(now, string.sub(words, 1, 4000))
			changed = true
		end
	elseif a == "new" then
		-- (a blank note, opened to be written)
		editing = new_note(now, "").id
		changed = true
	elseif a == "edit" and n then
		editing = n.id
	elseif a == "back" then
		editing = nil
	elseif a == "title" and n then
		n.title = text
		touch(n, now)
		changed = true
	elseif a == "text" and n then
		n.text = text
		touch(n, now)
		changed = true
	elseif a == "item" and n and text ~= "" then
		n.items = n.items or {}
		table.insert(n.items :: { Item }, { text = text, done = false })
		touch(n, now)
		changed = true
	elseif a == "tick" and n and n.items then
		local it = (n.items :: { Item })[tonumber(args.i) or 0]
		if it then
			it.done = not it.done
			touch(n, now)
			changed = true
		end
	elseif a == "list" and n then
		if n.items then
			-- (no checklist: its items kept as lines of its words)
			local lines = {}
			for _, it in n.items :: { Item } do
				table.insert(lines, it.text)
			end
			if #lines > 0 then
				n.text = (if n.text ~= "" then n.text .. "\n" else "") .. table.concat(lines, "\n")
			end
			n.items = nil
		else
			n.items = {}
		end
		touch(n, now)
		changed = true
	elseif a == "color" and (n or editing) then
		local m = n or find(editing :: string)
		if m and COLORS[tostring(args.value)] then
			m.color = tostring(args.value)
			touch(m, now)
			changed = true
		end
	elseif a == "pin" and n then
		n.pinned = not n.pinned
	elseif a == "rest" and n then
		local on = not n.rest
		for _, m in notes do
			m.rest = false
		end
		n.rest = on
	elseif a == "ask_delete" and n then
		out = draw(open)
		out.modal = {
			icon = "trash",
			tone = "red",
			title = "Delete This Note?",
			text = first_line(n),
			buttons = { { label = "Cancel" }, { label = "Delete", action = "delete", args = { id = n.id }, tone = "red" } },
		}
		return out
	elseif a == "delete" and n then
		remove(n)
		changed = true
	elseif a == "notion_token" then
		notion.token = if text ~= "" then text else nil
		notion.ready, problem = nil, nil
		changed = true
	elseif a == "notion_db" then
		notion.db = db_id(text)
		notion.ready, problem = nil, nil
		if not notion.db then
			problem = "That is not a link to a Notion database."
		end
		changed = true
	elseif a == "notion_off" then
		notion = {}
		problem = nil
		for _, m in notes do
			m.notion, m.sig, m.synced = nil, nil, nil
			if m.deleted then
				m.deleted = false
			end
		end
		for i = #notes, 1, -1 do
			if notes[i].deleted then
				table.remove(notes, i)
			end
		end
	else
		return nil
	end
	out = draw(open)
	out.note = settings_note() or false
	-- (an edit: sent to Notion at once)
	if changed and connected() and not syncing then
		out.fetch = sync_start(now)
	end
	return out
end
