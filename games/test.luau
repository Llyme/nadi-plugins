local function win(pid: number, exe: string, full: boolean)
	return { pid = pid, exe = exe, path = "C:\\Games\\" .. exe, full = full, tool = false, name = "" }
end

local function look(now: number, windows)
	return handle({ event = "processes", now = now, settings = {}, windows = windows })
end

local function fetched(id: string, body)
	return handle({ event = "fetched", now = 0, settings = {}, id = id, status = 200, body = nadi.json.encode(body) })
end

tests = {}

function tests.a_fullscreen_program_is_a_game_and_a_browser_is_not()
	handle({ event = "start", now = 0, settings = {} })
	local r = look(0, { win(1, "deep_rock-galactic.exe", true), win(2, "chrome.exe", true), win(3, "code.exe", false) })
	eq(r.widgets.game.panel.title, "Deep Rock Galactic", "name")
	eq(r.quiet, true, "quiet")
	eq(r.widgets.game.peek.icon, "", "no icon (its icon is on the dock): words alone")
	eq(r.widgets.game.peek.image, nil, "not its icon")
	eq(r.widgets.game.peek.text, "<1m", "no battery: the time")
	-- (its wiki looked for: wiki.gg first)
	eq(r.fetch[1].url, "https://deeprockgalactic.wiki.gg/api.php?action=query&meta=siteinfo&format=json&formatversion=2", "probe")
end

function tests.it_goes_when_its_windows_do()
	handle({ event = "start", now = 0, settings = {} })
	look(0, { win(1, "game.exe", true) })
	local r = look(125000, { win(1, "game.exe", false) })
	eq(r.widgets.game.panel.sub, "2m", "switched away: still there")
	r = look(130000, {})
	eq(r.widgets.game.idle, true, "gone (idle)")
	eq(r.quiet, false, "not quiet")
end

function tests.the_names_a_wiki_may_have()
	eq(slugs("Hades II"), { "hadesii", "hades-ii", "hades" }, "a sequel")
	eq(slugs("Deep Rock Galactic"), { "deeprockgalactic", "deep-rock-galactic", "deep" }, "words")
	eq(candidates("Terraria"), { "https://terraria.wiki.gg", "https://terraria.fandom.com" }, "one word")
end

function tests.a_wiki_not_there_then_found_and_searched()
	handle({ event = "start", now = 0, settings = {} })
	look(0, { win(1, "terraria.exe", true) })
	-- (wiki.gg answers with a redirect: not it; Fandom answers)
	local r = handle({ event = "fetched", now = 0, settings = {}, id = "probe", status = 302, body = "" })
	eq(r.fetch[1].url, "https://terraria.fandom.com/api.php?action=query&meta=siteinfo&format=json&formatversion=2", "next")
	r = handle({ event = "action", action = "search", args = { text = "zenith" }, now = 0, settings = {} })
	eq(r.fetch, nil, "waits for its wiki")
	r = fetched("probe", { query = { general = { sitename = "Terraria Wiki" } } })
	eq(r.store.wikis["C:\\Games\\terraria.exe"], "https://terraria.fandom.com", "kept")
	eq(r.fetch[1].id, "search", "then searched")
	r = fetched("search", { query = { search = { { title = "Zenith" }, { title = "Zenith (disambiguation)" } } } })
	eq(r.fetch[1].id, "page", "the first page")
	eq(r.fetch[2].id, "secs", "and its sections")
	r = fetched("page", { parse = { title = "Zenith", text = '<aside><figure class="pi-item pi-image"><img src="https://static.wikia.nocookie.net/z.png" width="200"></figure><h3 class="pi-data-label pi-secondary-font">Damage</h3>\n<div class="pi-data-value pi-font">190</div></aside><p><i>This page may contain spoilers.</i> Read at your own risk!</p><p>The <b>Zenith</b> is a <a href="x">sword</a>.<sup>&#91;1&#93;</sup> It is the best one there is.</p>' } })
	r = fetched("secs", { parse = { sections = { { toclevel = 1, line = "<i>Crafting</i>", index = "1" }, { toclevel = 2, line = "Recipe", index = "2" }, { toclevel = 1, line = "Tips", index = "" } } } })
	local body = r.widgets.game.panel.body
	eq(body[2].kind, "infocard", "a card")
	eq(body[2].title, "Zenith", "its title")
	eq(body[2].facts, { { "Damage", "190" } }, "its facts")
	eq(body[2].image, "https://static.wikia.nocookie.net/z.png", "its picture")
	eq(body[4].text, "The Zenith is a sword. It is the best one there is.", "its words, plain, no spoiler warning")
	eq(body[3].items, { { text = "Overview", action = "sec:0", on = true }, { text = "Crafting", action = "sec:1", on = false } }, "its sections")
	eq(body[5].items[1].text, "Zenith (disambiguation)", "the others")
	r = handle({ event = "action", action = "sec:1", now = 0, settings = {} })
	eq(r.fetch[1].id, "sec", "a section asked")
	r = fetched("sec", { parse = { text = "<p>Crafted at a Mythril Anvil from many other swords.</p>" } })
	eq(r.widgets.game.panel.body[4].text, "Crafted at a Mythril Anvil from many other swords.", "its words, in place")
	eq(r.widgets.game.panel.body[3].items[2].on, true, "lit")
	r = handle({ event = "action", action = "open", now = 0, settings = {} })
	eq(r.open, "https://terraria.fandom.com/wiki/Zenith", "the page")
end

function tests.a_page_picture_from_its_markup()
	eq(first_picture('<img src="/images/a.png" width="16"><img alt="x" src="/images/thumb/b.png/120px-b.png" width="120" height="90">', "https://x.wiki.gg"), "https://x.wiki.gg/images/thumb/b.png/120px-b.png", "the first big one, whole")
	eq(first_picture('<img src="data:image/gif;base64,R0" data-src="https://static.wikia.nocookie.net/c.png?a=1&amp;b=2" width="200">', "https://y.fandom.com"), "https://static.wikia.nocookie.net/c.png?a=1&b=2", "a lazy one")
	eq(first_picture("<p>none</p>", "https://x.wiki.gg"), "", "none")
end

function tests.long_words_are_cut_between_letters()
	eq(cut("When Arcane's Intellect ≥ Will, switch", 25), "When Arcane's Intellect …", "before the ≥")
	eq(cut("When Arcane's Intellect ≥ Will, switch", 26), "When Arcane's Intellect ≥…", "the ≥ whole")
	eq(cut("short", 10), "short", "short")
end

function tests.a_table_of_sentences_reads_as_paragraphs()
	local long = "Operator Intellect and Will are raised by eight points for the whole team."
	local b = blocks_of({ head = true, rows = { { "Keen Mind" }, { { items = { { image = "x", text = "" } } }, long } } })
	eq(b, { { kind = "text", label = "Keen Mind", text = "", lines = 6 }, { kind = "text", label = "", text = long, lines = 6 } }, "paragraphs")
	b = blocks_of({ head = false, rows = { { "HP", "500" } } })
	eq(b[1].kind, "table", "short: a grid")
end

function tests.the_words_size_cycles_and_is_kept()
	handle({ event = "start", now = 0, settings = {}, store = { size = 3 } })
	look(0, { win(1, "game.exe", true) })
	local r = handle({ event = "action", action = "size", now = 0, settings = {} })
	eq(r.widgets.game.panel.zoom, 1.0, "after large: normal")
	eq(r.store.size, 1, "kept")
	r = handle({ event = "action", action = "size", now = 0, settings = {} })
	eq(r.widgets.game.panel.zoom, 1.15, "medium")
end

function tests.a_section_of_lists_says_them()
	eq(section_words('<h2>References</h2><ol><li><a href="x">Crisis Team</a></li><li>Reconveners</li></ol><p>Short.</p>'), "• Crisis Team\n• Reconveners\nShort.", "its lines, in order")
end

function tests.a_wiki_by_another_name()
	handle({ event = "start", now = 0, settings = {} })
	look(0, { win(1, "nte.exe", true) })
	local r = handle({ event = "fetched", now = 0, settings = {}, id = "probe", status = 401, body = "" })
	r = handle({ event = "fetched", now = 0, settings = {}, id = "probe", status = 301, body = "", location = "https://neverness-to-everness.fandom.com/api.php?action=query" })
	eq(r.fetch[1].url, "https://neverness-to-everness.fandom.com/api.php?action=query&meta=siteinfo&format=json&formatversion=2", "where it points, next")
end

function tests.letters_by_their_numbers()
	eq(plain("April 29&#44; 2026 &#x2014; it&#39;s"), "April 29, 2026 — it's", "decoded")
end

function tests.no_wiki_anywhere()
	handle({ event = "start", now = 0, settings = {} })
	look(0, { win(1, "zz.exe", true) })
	local r = handle({ event = "fetched", now = 0, settings = {}, id = "probe", status = 404, body = "" })
	r = handle({ event = "fetched", now = 0, settings = {}, id = "probe", status = 302, body = "" })
	eq(r.store.wikis["C:\\Games\\zz.exe"], false, "none, kept")
	eq(r.widgets.game.panel.body[1].placeholder, "No Wiki Found", "said")
end

function tests.at_rest_the_controller_else_the_frames_else_the_time()
	handle({ event = "start", now = 0, settings = {} })
	look(0, { win(1, "game.exe", true) })
	local r = handle({ event = "gear", now = 0, settings = {}, controllers = { { slot = 1, level = 0.65, wired = false } }, devices = {} })
	eq(r.widgets.game.peek.pad, { level = 0.65 }, "a controller: its shape, as full")
	r = handle({ event = "gear", now = 0, settings = {}, controllers = {}, devices = { { name = "Buds", level = 0.2, kind = "headset" } } })
	eq(r.widgets.game.peek.text, "<1m", "none (a headset is the Devices widget's): the time")
	eq(r.watch, { 1 }, "its frames watched")
	r = handle({ event = "usage", now = 0, settings = {}, usage = { { pid = 1, alive = true, fps = 143.6 } } })
	eq(r.widgets.game.peek.roll, { value = 144, color = "#5AC88C" }, "the frames, rolling, smooth")
end

function tests.facts_of_each_kind_of_infobox()
	eq(infobox('<div class="druid-row druid-row-ep" data-druid-section-row="Intro"><div class="druid-label druid-label-theme">EP</div><div class="druid-data druid-data-theme">\n<a href="/x">Guided by Echoes</a></div></div><div class="druid-row druid-row-cn druid-collapsed"><div class="druid-label">CN</div><div class="druid-data">x</div></div>'), { { "EP", "Guided by Echoes" } }, "wiki.gg, folded rows left out")
	eq(infobox('<th class="pi-horizontal-group-item pi-data-label pi-secondary-font" data-source="quality">Quality</th><td class="pi-horizontal-group-item pi-data-value pi-font" data-source="quality"><img alt="5 Stars" src="data:x"></td>'), { { "Quality", "5 Stars" } }, "Fandom side by side; a picture's words")
	eq(infobox('<table class="infobox"><tr><th>Type</th><td>Mineral</td></tr></table>'), { { "Type", "Mineral" } }, "a table")
end
