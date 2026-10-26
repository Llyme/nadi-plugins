local T0 = 1800000000000

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

tests = {}

function tests.titles_lose_the_browser_and_the_profile()
	check(handle(ev("start", T0)).browser.config ~= nil, "the extension is told what to read at the start")
	local r = handle(ev("processes", T0, { windows = { win("msedge.exe", "Zeppelin - Wikipedia - Personal - Microsoft\u{200b} Edge", true) } }))
	eq(r.browser.watch, { { "msedge", "Zeppelin - Wikipedia" } }, "watched by its clean title")
	check(handle(ev("processes", T0 + 2000, { windows = { win("msedge.exe", "Zeppelin - Wikipedia - Personal - Microsoft\u{200b} Edge", true) } })).browser.config == nil, "not again")
	eq(r.note, "Waiting for the browser extension (without it, pages are not read).", "note")
end

function tests.a_private_window_is_not_watched()
	handle(ev("start", T0))
	local r = handle(ev("processes", T0, { windows = { win("msedge.exe", "InPrivate - Something - Microsoft\u{200b} Edge", true) } }))
	eq(#r.browser.watch, 0, "nothing")
end

function tests.a_read_page_of_a_known_kind_gets_a_section()
	handle(ev("start", T0))
	handle(ev("processes", T0, { windows = { win("chrome.exe", "Pancakes Recipe - Google Chrome", true) } }))
	local page = {
		browser = "chrome", title = "Pancakes Recipe", url = "https://www.allrecipes.com/recipe/21014/pancakes/",
		nodes = { { k = "h", l = 1, t = "Pancakes", x = 0, y = 0, w = 100, h = 20 }, { k = "h", l = 2, t = "Ingredients", x = 0, y = 40, w = 100, h = 20 }, { k = "h", l = 2, t = "Directions", x = 0, y = 80, w = 100, h = 20 } },
		ld = { { type = { "Recipe" }, name = "Pancakes", total = 25, ingredients = 7 } },
		meta = {}, words = 900, vh = 900,
	}
	local r = handle(ev("browser", T0 + 1000, { browser = { browsers = { "chrome" }, tabs = { chrome = { "pancakes recipe" } }, open = { chrome = { "Pancakes Recipe" } }, moves = {}, pages = { page } } }))
	local w = r.widgets.pages
	check(w.panel ~= nil, "a section")
	eq(w.panel.body[1].title, "Pancakes Recipe", "its title")
	check(string.find(w.panel.body[1].sub, "allrecipes.com", 1, true) ~= nil, "its domain")
	eq(r.note, "Reading pages in Chrome.", "note")
	local f = handle(ev("action", T0 + 2000, { action = "focus:p1", args = {} }))
	eq(f.focus.title, "Pancakes Recipe", "focus")
end
