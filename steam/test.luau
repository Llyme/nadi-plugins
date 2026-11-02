local function game(id: number, name: string, played: number, more: any?)
	local g = { id = id, name = name, last_played = played, size = 0, updating = false, progress = nil, art = "C:/Steam/" .. id .. "/library_600x900.jpg", icon = "C:/Steam/" .. id .. "/icon.jpg" }
	for k, v in more or {} do
		g[k] = v
	end
	return g
end

local function look(now: number, games, running: number?)
	return handle({ event = "helper", now = now, settings = {}, data = { kind = "library", games = games, running = running or 0 } })
end

tests = {}

function tests.the_library_last_played_first_and_play_opens_steam()
	handle({ event = "start", now = 0, settings = {} })
	local w = look(0, { game(620, "Puzzle Rooms", 300), game(1145360, "Underworld Run", 200) }).widgets.library
	local covers = w.panel.body[2]
	eq(covers.kind, "covers", "covers")
	eq(covers.items[1], { image = "img:C:/Steam/620/library_600x900.jpg", title = "Puzzle Rooms", action = "play:620" }, "a cover")
	eq(w.peek, { image = "own:steam.png", image_icon = "own:steam-icon.png", icon = "gamepad", big = true }, "at rest: Steam's logo alone")
	eq(handle({ event = "action", now = 0, settings = {}, action = "play:620" }).open, "steam://rungameid/620", "plays")
end

function tests.its_games_go_to_the_launcher()
	handle({ event = "start", now = 0, settings = {} })
	local out = look(0, { game(620, "Puzzle Rooms", 300), game(1145360, "Underworld Run", 200) })
	eq(#out.apps, 2, "each game")
	eq(out.apps[1], { name = "Puzzle Rooms", open = "steam://rungameid/620", image = "img:C:/Steam/620/icon.jpg", badge = "own:steam-icon.png", tip = "Steam" }, "started through Steam, with its icon and its badge")
end

function tests.search_and_what_shows_at_rest()
	handle({ event = "start", now = 0, settings = {} })
	look(0, { game(620, "Puzzle Rooms", 300), game(1145360, "Underworld Run", 200, { updating = true, progress = 0.4 }) })
	local w = handle({ event = "action", now = 0, settings = {}, action = "search", args = { text = "under" } }).widgets.library
	eq(#w.panel.body[2].items, 1, "found")
	eq(w.peek.ring.part, 0.4, "updating")
	w = look(60000, { game(620, "Puzzle Rooms", 300), game(1145360, "Underworld Run", 200) }, 620).widgets.library
	eq(w.peek, { image = "own:steam.png", image_icon = "own:steam-icon.png", icon = "gamepad", big = true }, "playing: Steam's logo alone")
	eq(string.sub(w.tip, 1, 12), "Puzzle Rooms", "its tip names the game")
	w = look(60000 + 75 * 60000, { game(620, "Puzzle Rooms", 300) }, 620).widgets.library
	eq(w.tip, "Puzzle Rooms\nPlaying for 1h 15m", "for how long")
end
