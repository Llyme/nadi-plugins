-- The tests of Sticky Notes: `nadi-plugin test sticky-notes`.

local T0 = 1800000000000

local function start(store: any?)
	return handle({ event = "start", now = T0, settings = {}, store = store, open = {} })
end

local function act(a: string, args: any?, now: number?)
	return handle({ event = "action", now = now or T0 + 1000, settings = {}, action = a, args = args or {}, open = { notes = true } })
end

local function find(body, kind)
	for _, b in body do
		if b.kind == kind then
			return b
		end
	end
	return nil
end

tests = {}

function tests.written_shown_and_kept()
	local w = start(nil).widgets.notes
	eq(w.peek.icon, "note", "a note at rest")
	eq(find(w.panel.body, "field").action, "add", "a field for a new one")
	local out = act("add", { text = "Call the dentist" })
	local cards = find(out.widgets.notes.panel.body, "cards").cards
	eq(#cards, 1, "on the wall")
	eq(cards[1].text, "Call the dentist", "its words")
	eq(cards[1].color, "#2e2a17", "yellow at first")
	eq(out.widgets.notes.peek.text, "1", "how many, at rest")
	eq(#out.store.notes, 1, "kept")
	eq(out.fetch, nil, "not connected: nothing asked of Notion")
end

function tests.edited_as_a_checklist_in_green()
	start(nil)
	local id = act("add", { text = "Shopping" }).store.notes[1].id
	act("edit", { id = id })
	act("title:" .. id, { text = "Groceries" })
	act("list", { id = id })
	act("item:" .. id, { text = "Milk" })
	local out = act("item:" .. id, { text = "Eggs" })
	local card = find(out.widgets.notes.panel.body, "cards").cards[1]
	eq(#card.items, 2, "its checklist, on the note itself")
	eq(card.edit.title, "title:" .. id, "its title written in place")
	eq(card.edit.item, "item:" .. id, "an item added in place")
	eq(#find(out.widgets.notes.panel.body, "colors").items, 6, "its colours as dots")
	act("tick", { id = id, i = 1 })
	out = act("color", { id = id, value = "green" })
	local n = out.store.notes[1]
	eq(n.title, "Groceries", "its title")
	eq(n.items[1].done, true, "ticked")
	eq(n.color, "green", "green")
	out = act("back")
	card = find(out.widgets.notes.panel.body, "cards").cards[1]
	eq(card.items[1].state, "done", "ticked on the wall too")
	eq(card.ink, "#b7e8c9", "in green")
end

function tests.pinned_to_the_screen_and_closed()
	start(nil)
	local id = act("add", { text = "Wi-Fi: 1234" }).store.notes[1].id
	local out = act("pin", { id = id })
	local w = out.widgets["note:" .. id]
	eq(w.window.open, true, "its window asked for")
	eq(w.window.remember, true, "remembered")
	eq(w.window.ground, "#2e2a17", "in its colour")
	eq(w.peek, nil, "a window only: no place on an edge")
	-- (its window: shut a moment as it comes out (ignored), out, then closed)
	eq(handle({ event = "open", now = T0 + 2000, settings = {}, widget = "note:" .. id, open = false }), nil, "shut before it was out: nothing")
	handle({ event = "open", now = T0 + 2100, settings = {}, widget = "note:" .. id, open = true })
	out = handle({ event = "open", now = T0 + 3000, settings = {}, widget = "note:" .. id, open = false })
	eq(out.store.notes[1].pinned, false, "closed: unpinned")
	check(next(out.widgets["note:" .. id]) == nil, "its window taken away")
end

function tests.deleted_after_asking()
	start(nil)
	local id = act("add", { text = "Old" }).store.notes[1].id
	act("pin", { id = id })
	local out = act("ask_delete", { id = id })
	eq(out.modal.buttons[2].action, "delete", "a modal asks")
	out = act("delete", { id = id })
	eq(#out.store.notes, 0, "gone")
	check(next(out.widgets["note:" .. id]) == nil, "its window too")
end

function tests.in_step_with_notion()
	start(nil)
	act("add", { text = "Local one" })
	act("notion_token", { text = "secret_abc" })
	local out = act("notion_db", { text = "https://www.notion.so/me/Notes-0123456789abcdef0123456789abcdef?v=1" })
	eq(out.fetch[1].id, "db", "the database looked at first")
	eq(out.fetch[1].url, "https://api.notion.com/v1/databases/0123456789abcdef0123456789abcdef", "its id from the link")
	eq(out.fetch[1].headers.Authorization, "Bearer secret_abc", "the secret")
	-- (a database with a title property "Task", and no Text or Color: they are added)
	local db = '{"properties":{"Task":{"type":"title","title":{}}}}'
	out = handle({ event = "fetched", now = T0 + 2000, settings = {}, id = "db", status = 200, body = db, open = {} })
	eq(out.fetch[1].id, "dbprep", "Text and Color added")
	out = handle({ event = "fetched", now = T0 + 2100, settings = {}, id = "dbprep", status = 200, body = "{}", open = {} })
	eq(out.fetch[1].id, "query", "then its pages")
	-- (Notion has one page; this PC one note not there yet)
	local q = nadi.json.encode({ results = { {
		id = "page-1",
		created_time = "2026-11-01T10:00:00.000Z",
		properties = {
			Task = { type = "title", title = { { plain_text = "Groceries" } } },
			Text = { type = "rich_text", rich_text = { { plain_text = "Weekly\n- [x] Milk\n- [ ] Eggs" } } },
			Color = { type = "select", select = { name = "blue" } },
		},
	} } })
	out = handle({ event = "fetched", now = T0 + 3000, settings = {}, id = "query", status = 200, body = q, open = {} })
	eq(#out.store.notes, 2, "Notion's page here too")
	local got = nil
	for _, n in out.store.notes do
		if n.notion == "page-1" then
			got = n
		end
	end
	eq(got.title, "Groceries", "its title")
	eq(got.text, "Weekly", "its words")
	eq(#got.items, 2, "its checklist")
	eq(got.items[1].done, true, "ticked")
	eq(got.color, "blue", "its colour")
	eq(#out.fetch, 1, "the local one sent")
	eq(out.fetch[1].method, "POST", "made there")
	local sent = nadi.json.decode(out.fetch[1].body)
	eq(sent.properties.Text.rich_text[1].text.content, "Local one", "its words")
	check(sent.properties.Task ~= nil, "under the database's own title property")
end

function tests.notion_secret_refused_said()
	start({ notes = {}, notion = { token = "x", db = "0123456789abcdef0123456789abcdef" } })
	local out = handle({ event = "fetched", now = T0 + 2000, settings = {}, id = "db", status = 401, body = "{}", open = {} })
	eq(out.note, "Notion did not take the secret.", "said in Settings")
end
