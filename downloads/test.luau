local function dl(id: string, kind: string, received: number, total: number?)
	return { id = id, source = if kind == "steam" then "Steam" else "Chrome", kind = kind, name = id .. ".bin", received = received, total = total }
end

local function look(now: number, observed, ended: any?)
	return handle({ event = "downloads", now = now, settings = {}, observed = observed, ended = ended or {} })
end

local function find(body, kind: string)
	for _, b in body do
		if b.kind == kind then
			return b
		end
	end
	return nil
end

tests = {}

function tests.a_download_shows_how_far_how_fast_and_how_long()
	look(0, { dl("a", "browser", 0, 10000000) })
	local w = look(1000, { dl("a", "browser", 2000000, 10000000) }).widgets.downloads
	eq(w.peek.ring.part, 0.2, "a fifth")
	eq(w.peek.text, "4 s", "time left")
	local t = find(w.panel.body, "transfer")
	eq(t.sub, "1.9 MB/s", "speed")
	eq(t.right, "4 s", "time left")
	eq(t.part, 0.2, "bar")
	check(find(w.panel.body, "linegraph") ~= nil, "the speed as a line")
end

function tests.one_that_stands_still_goes_and_comes_back_when_it_moves()
	look(0, { dl("a", "browser", 100, 1000) })
	eq((look(6000, { dl("a", "browser", 100, 1000) }).widgets.downloads).idle, true, "stuck: gone (idle)")
	eq((look(7000, { dl("a", "browser", 100, 1000) }).widgets.downloads).idle, true, "still stuck (idle)")
	check(look(8000, { dl("a", "browser", 200, 1000) }).widgets.downloads.peek ~= nil, "moves: back")
end

function tests.a_finished_one_opens_shows_or_clears()
	look(0, { dl("a", "browser", 100, 1000) })
	local w = look(1000, {}, { { id = "a", finished = true, path = "C:\Downloads\a.bin" } }).widgets.downloads
	eq(w.peek.icon, "check", "done")
	local t = find(w.panel.body, "transfer")
	check(t.done, "a line")
	eq(t.action, "open:a", "pressed: opened")
	eq(t.buttons[1].action, "show:a", "its folder")
	eq(handle({ event = "action", action = "open:a", now = 2000, settings = {} }).open_file, "C:\Downloads\a.bin", "opened")
	eq(handle({ event = "action", action = "show:a", now = 2000, settings = {} }).reveal, "C:\Downloads\a.bin", "shown")
	eq((handle({ event = "action", action = "clear:a", now = 2000, settings = {} }).widgets.downloads).idle, true, "cleared (idle)")
end

function tests.steam_shows_its_art_and_no_progress()
	look(0, { dl("steam:2161700", "steam", 0, 5000000000) })
	local w = look(1000, { dl("steam:2161700", "steam", 1000000, 5000000000) }).widgets.downloads
	check(w.peek.ring == nil, "no ring")
	local t = find(w.panel.body, "transfer")
	eq(t.image, "steamart:2161700", "its art")
	check(t.part == nil, "no bar to fill")
end
