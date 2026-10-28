local function dl(id: string, kind: string, received: number, total: number?)
	return { id = id, source = if kind == "steam" then "Steam" else "Chrome", kind = kind, name = id .. ".bin", received = received, total = total }
end

local function look(now: number, observed, ended: any?)
	return handle({ event = "downloads", now = now, settings = {}, observed = observed, ended = ended or {} })
end

tests = {}

function tests.a_download_shows_how_far_and_how_fast()
	look(0, { dl("a", "browser", 0, 10000000) })
	local w = look(1000, { dl("a", "browser", 2000000, 10000000) }).widgets.downloads
	eq(w.peek.ring.part, 0.2, "a fifth")
	eq(w.peek.text, "1.9 MB/s", "speed")
	eq(w.panel.body[2].rows[1].sub, "Chrome, 20%, 1.9 MB/s", "row")
	eq(w.panel.title, nil, "no head")
end

function tests.one_that_stands_still_goes_and_comes_back_when_it_moves()
	look(0, { dl("a", "browser", 100, 1000) })
	eq((look(6000, { dl("a", "browser", 100, 1000) }).widgets.downloads).idle, true, "stuck: gone (idle)")
	eq((look(7000, { dl("a", "browser", 100, 1000) }).widgets.downloads).idle, true, "still stuck (idle)")
	check(look(8000, { dl("a", "browser", 200, 1000) }).widgets.downloads.peek ~= nil, "moves: back")
end

function tests.a_finished_one_stays_until_pressed_and_shows_its_file()
	look(0, { dl("a", "browser", 100, 1000) })
	local w = look(1000, {}, { { id = "a", finished = true, path = "C:\\Downloads\\a.bin" } }).widgets.downloads
	eq(w.peek.text, "1 done", "done")
	eq(w.panel.body[1].items[1].action, "done:a", "button")
	local r = handle({ event = "action", action = "done:a", now = 2000, settings = {} })
	eq(r.reveal, "C:\\Downloads\\a.bin", "shown")
	eq((r.widgets.downloads).idle, true, "gone (idle)")
end

function tests.steam_shows_no_progress()
	look(0, { dl("s", "steam", 0, 5000000000) })
	local w = look(1000, { dl("s", "steam", 1000000, 5000000000) }).widgets.downloads
	check(w.peek.ring == nil, "no ring")
	eq(w.panel.body[2].rows[1].sub, "Steam, 4.7 GB, 977 KB/s", "size and speed")
end
