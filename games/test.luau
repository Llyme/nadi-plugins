local function win(pid: number, exe: string, full: boolean)
	return { pid = pid, exe = exe, path = "C:\\Games\\" .. exe, full = full, tool = false, name = "" }
end

local function look(now: number, windows)
	return handle({ event = "processes", now = now, settings = {}, windows = windows })
end

tests = {}

function tests.a_fullscreen_program_is_a_game_and_a_browser_is_not()
	local r = look(0, { win(1, "deep_rock-galactic.exe", true), win(2, "chrome.exe", true), win(3, "code.exe", false) })
	eq(r.widgets.game.panel.title, "Deep Rock Galactic", "name")
	eq(r.watch, { 1 }, "watched")
	eq(r.quiet, true, "quiet")
	eq(r.widgets.game.peek.image, "exe:C:\\Games\\deep_rock-galactic.exe", "its icon, by name")
	eq(#r.widgets.game.panel.body, 1, "one")
end

function tests.its_numbers_and_it_goes_when_its_process_ends()
	look(0, { win(1, "game.exe", true) })
	local r = handle({ event = "usage", now = 125000, settings = {}, usage = { { pid = 1, alive = true, fps = 143.6, low_fps = 98, frame_ms = { 7, 7.04 }, cpu = 30, ram = 20, ram_gb = 3.2, gpu = 80 } } })
	local w = r.widgets.game
	eq(w.peek.text, "144 fps", "fps")
	eq(w.panel.sub, "Playing for 2m", "for")
	local s = w.panel.body[1]
	eq(s.body[1].items[2], { value = "98", label = "1% low" }, "low")
	eq(s.body[2].values[2], 7, "frame times")
	eq(s.body[3].rows[1].title, "GPU", "gpu first")
	r = handle({ event = "usage", now = 130000, settings = {}, usage = { { pid = 1, alive = false } } })
	eq(r.widgets.game, {}, "gone")
	eq(r.quiet, false, "not quiet")
end

function tests.the_one_in_front_leads()
	look(0, { win(1, "a.exe", true) })
	local r = look(1000, { win(2, "b.exe", true), win(1, "a.exe", true) })
	eq(r.widgets.game.panel.title, "B", "front")
	eq(handle({ event = "action", action = "focus:1", now = 0, settings = {} }).focus.exe, "C:\\Games\\a.exe", "focus")
end
