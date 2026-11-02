local T0 = 1800000000000

-- (each page as its program reads it: what the plugin is told with it)
local READ_RECIPE = { buttons = {}, domain = "allrecipes.com", kind = { fields = { { key = "Dish", rough = false, value = "Pancakes" }, { key = "Time", rough = false, value = "25 min" }, { key = "Ingredients", rough = false, value = "7 items" } }, id = "recipe", label = "Recipe", main = "Pancakes", sub = "25 min" } }
local READ_STEPS = { buttons = {}, domain = "game8.co", kind = { fields = { { key = "Steps", rough = false, value = "6" }, { key = "Starts with", rough = false, value = "Room 1" } }, id = "walkthrough", label = "Walkthrough", main = "Chapter 3 Walkthrough", sub = "6 steps" } }
local READ_GUIDE = { buttons = {}, domain = "gamefaqs.gamespot.com", kind = { fields = { { key = "Guide", rough = true, value = "Persona 5 Strikers Walkthrough" } }, guide = { key = "g1", text = false }, id = "walkthrough", label = "Walkthrough", main = "Persona 5 Strikers Walkthrough", sub = "Walkthrough" } }
local READ_RECIPE2 = { buttons = {}, domain = "allrecipes.com", kind = { fields = { { key = "Dish", rough = false, value = "Pancakes" }, { key = "Time", rough = false, value = "25 min" } }, id = "recipe", label = "Recipe", main = "Pancakes", sub = "25 min" } }


local function win(exe: string, title: string, front: boolean)
	return { exe = exe, title = title, front = front, path = "C:\\Apps\\" .. exe, name = "Edge" }
end

local function ev(kind: string, now: number, more: any?)
	local e: any = { event = kind, now = now, settings = {} }
	for k, v in more or {} do
		e[k] = v
	end
	return e
end

-- (what its program was asked, by name)
local function ask(r, name: string)
	for _, c in r.helper or {} do
		if c.cmd == name then
			return c
		end
	end
	return nil
end

tests = {}

function tests.titles_lose_the_browser_and_the_profile()
	check(ask(handle(ev("start", T0)), "config") ~= nil, "the extension is told what to read at the start")
	local r = handle(ev("processes", T0, { windows = { win("msedge.exe", "Zeppelin - Wikipedia - Personal - Microsoft\u{200b} Edge", true) } }))
	eq(ask(r, "watch").pages, { { "msedge", "Zeppelin - Wikipedia" } }, "watched by its clean title")
	check(ask(handle(ev("processes", T0 + 2000, { windows = { win("msedge.exe", "Zeppelin - Wikipedia - Personal - Microsoft\u{200b} Edge", true) } })), "config") == nil, "not again")
	eq(r.note, "Waiting for the browser extension (without it, pages are not read).", "note")
end

function tests.a_private_window_is_not_watched()
	handle(ev("start", T0))
	local r = handle(ev("processes", T0, { windows = { win("msedge.exe", "InPrivate - Something - Microsoft\u{200b} Edge", true) } }))
	eq(#ask(r, "watch").pages, 0, "nothing")
end

local function find(body, kind: string)
	for _, b in body do
		if b.kind == kind then
			return b
		end
	end
	return nil
end

function tests.a_read_page_of_a_known_kind_gets_a_card()
	handle(ev("start", T0))
	handle(ev("processes", T0, { windows = { win("chrome.exe", "Pancakes Recipe - Google Chrome", true) } }))
	local page = {
		browser = "chrome", title = "Pancakes Recipe", url = "https://www.allrecipes.com/recipe/21014/pancakes/",
		nodes = { { k = "h", l = 1, t = "Pancakes", x = 0, y = 0, w = 100, h = 20 }, { k = "h", l = 2, t = "Ingredients", x = 0, y = 40, w = 100, h = 20 }, { k = "h", l = 2, t = "Directions", x = 0, y = 80, w = 100, h = 20 } },
		ld = { { type = { "Recipe" }, name = "Pancakes", total = 25, ingredients = 7 } },
		meta = {}, words = 900, vh = 900, read = READ_RECIPE,
	}
	local r = handle(ev("helper", T0 + 1000, { data = { kind = "browser", browser = { browsers = { "chrome" }, tabs = { chrome = { "pancakes recipe" } }, open = { chrome = { "Pancakes Recipe" } }, moves = {}, pages = { page } } } }))
	local w = r.widgets.pages
	local card = find(w.panel.body, "card")
	eq(card.kind, "card", "a card")
	eq(card.body[1].text, "allrecipes.com", "its site")
	eq(find(card.body, "title").text, "Pancakes", "its title")
	eq(r.note, "Reading pages in Chrome.", "note")
	local f = handle(ev("action", T0 + 2000, { action = "focus:p1", args = {} }))
	eq(f.focus.title, "Pancakes Recipe", "focus")
end

function tests.a_walkthrough_is_followed_step_by_step()
	handle(ev("start", T0))
	handle(ev("processes", T0, { windows = { win("chrome.exe", "Chapter 3 Walkthrough - Google Chrome", true) } }))
	local nodes = { { k = "h", l = 1, t = "Chapter 3 Walkthrough", x = 0, y = -900, w = 100, h = 30 } }
	for i = 1, 6 do
		table.insert(nodes, { k = "h", l = 2, t = "Step " .. i .. ": Room " .. i, x = 0, y = -600 + i * 200, w = 100, h = 20 })
		table.insert(nodes, { k = "p", t = "Do the thing in room " .. i .. ".", x = 0, y = -580 + i * 200, w = 100, h = 40 })
	end
	local page = { browser = "chrome", title = "Chapter 3 Walkthrough", url = "https://game8.co/games/x/walkthrough", nodes = nodes, meta = {}, ld = {}, words = 3000, vh = 900, read = READ_STEPS }
	local r = handle(ev("helper", T0 + 1000, { data = { kind = "browser", browser = { browsers = { "chrome" }, tabs = { chrome = { "chapter 3 walkthrough" } }, open = { chrome = { "Chapter 3 Walkthrough" } }, moves = {}, pages = { page } } } }))
	local w = r.widgets.pages
	local card = find(w.panel.body, "card")
	local segs = find(card.body, "segs")
	eq(segs.n, 6, "six steps")
	-- (Step 4 sits at y 200, near the top of a 900 tall view; Step 5 at 400 is further down)
	eq(segs.at, 4, "the one at the top")
	local step = find(card.body, "step")
	eq(step.head, "Step 4: Room 4", "read out")
	eq(find(card.body, "text").text, "Do the thing in room 4.", "its words")
	eq(find(card.body, "stepnav").next, "go:p1:5", "the next one")
	eq(w.peek.text, "4 / 6", "at rest: where you are")
	r = handle(ev("action", T0 + 2000, { action = "go:p1:5" }))
	eq(ask(r, "scroll"), { cmd = "scroll", id = "scroll", browser = "chrome", title = "Chapter 3 Walkthrough", heading = "Step 5: Room 5" }, "the browser asked")
	eq(find(find(r.widgets.pages.panel.body, "card").body, "step").head, "Step 5: Room 5", "shown at once")
end

function tests.a_guide_read_whole_gives_its_steps_and_all_their_words()
	handle(ev("start", T0))
	handle(ev("processes", T0, { windows = { win("chrome.exe", "Persona 5 Strikers Walkthrough - Google Chrome", true) } }))
	local blocks = {
		{ "p", "And here we are in Sapporo." },
		{ "h", 3, "8/9" },
		{ "p", "Once you have control it is time for another **investigation**." },
		{ "ul", { "The crowd near Ann.", "Man outside the Drug store." } },
		{ "h", 3, "8/10" },
		{ "p", "After the scenes, enter the Jail." },
		{ "h", 4, "Suzushino (jail)" },
		{ "p", "A battle at once." },
	}
	local nodes = { { k = "h", l = 1, t = "Persona 5 Strikers Walkthrough", x = 0, y = -900, w = 100, h = 30 }, { k = "h", l = 3, t = "8/9", x = 0, y = 100, w = 100, h = 20 }, { k = "h", l = 3, t = "8/10", x = 0, y = 700, w = 100, h = 20 } }
	local page = { browser = "chrome", title = "Persona 5 Strikers Walkthrough", url = "https://gamefaqs.gamespot.com/ps4/1/faqs/2/8-8-8-13", nodes = nodes, meta = {}, ld = {}, words = 3000, vh = 900, guide = { key = "g1", text = false, heads = {}, blocks = blocks }, read = READ_GUIDE }
	local r = handle(ev("helper", T0 + 1000, { data = { kind = "browser", browser = { browsers = { "chrome" }, tabs = { chrome = { "persona 5 strikers walkthrough" } }, open = { chrome = { "Persona 5 Strikers Walkthrough" } }, moves = {}, pages = { page } } } }))
	local card = find(r.widgets.pages.panel.body, "card")
	local segs = find(card.body, "segs")
	eq(segs.n, 3, "the words before, 8/9, 8/10")
	eq(segs.at, 2, "8/9 is at the top")
	eq(find(card.body, "step").head, "8/9", "its heading")
	local texts = {}
	for _, b in card.body do
		if b.kind == "text" then
			table.insert(texts, b.text)
		end
	end
	eq(texts, { "Once you have control it is time for another investigation.", "• The crowd near Ann.\n• Man outside the Drug store." }, "all its words")
	r = handle(ev("action", T0 + 2000, { action = "go:p1:3" }))
	local body = find(r.widgets.pages.panel.body, "card").body
	local heads = {}
	for _, b in body do
		if b.kind == "step" then
			table.insert(heads, b.head)
		end
	end
	eq(heads, { "8/10", "Suzushino (jail)" }, "the next, with its own sub-heading")
end

function tests.its_words_bigger_or_smaller_and_kept()
	handle(ev("start", T0, { store = { zoom = 1.6 } }))
	handle(ev("processes", T0, { windows = { win("chrome.exe", "Pancakes Recipe - Google Chrome", true) } }))
	local page = { browser = "chrome", title = "Pancakes Recipe", url = "https://www.allrecipes.com/recipe/1/pancakes/", nodes = { { k = "h", l = 1, t = "Pancakes", x = 0, y = 0, w = 100, h = 20 } }, ld = { { type = { "Recipe" }, name = "Pancakes", total = 25 } }, meta = {}, words = 900, vh = 900, read = READ_RECIPE2 }
	local r = handle(ev("helper", T0 + 1000, { data = { kind = "browser", browser = { browsers = { "chrome" }, tabs = { chrome = { "pancakes recipe" } }, open = { chrome = { "Pancakes Recipe" } }, moves = {}, pages = { page } } } }))
	local panel = r.widgets.pages.panel
	eq(panel.zoom, 1.6, "as it was kept")
	local tools = panel.body[1].items
	eq(tools[2].icon, "minus", "smaller")
	eq(#tools, 3, "no bigger: as big as it goes")
	r = handle(ev("action", T0 + 2000, { action = "zoom:-" }))
	eq(r.widgets.pages.panel.zoom, 1.5, "smaller")
	eq(r.store, { zoom = 1.5 }, "kept")
end
