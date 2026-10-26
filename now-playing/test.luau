-- The tests of Now Playing: `nadi-plugin test now-playing`.

local function session(source: string, title: string, playing: boolean)
	return { source = source, title = title, artist = "Artist", playing = playing, position = 30, duration = 120, can_previous = true, can_next = true, can_seek = true, can_rate = true, rate = 1.5, art_id = 7 }
end

local function media(list, settings: any?)
	return handle({ event = "media", now = 0, settings = settings or {}, sessions = list })
end

local function act(a: string, args: any?)
	return handle({ event = "action", now = 0, settings = {}, action = a, args = args or {} })
end

tests = {}

function tests.it_follows_the_player_that_plays()
	local w = media({ session("a", "Paused", false), session("b", "Playing", true) }).widgets.song
	eq(w.peek.text, "Playing", "title")
	eq(w.peek.image, "media:b:7", "the cover, by name")
	eq(w.peek.button.icon, "pause", "pause")
	eq(w.peek.viz, true, "bars")
	eq(w.panel.body[1].kind, "seek", "seek first")
	eq(w.panel.body[1].value, 0.25, "a quarter in")
	eq(w.panel.body[2].items[2].action, "toggle", "transport")
	eq(w.panel.body[3].value, 1.5, "speed")
	eq(w.panel.body[4].rows[1].title, "Paused", "the other player")
	eq(media({}).widgets.song, {}, "nothing playing: gone")
end

function tests.paused_offers_play_and_no_bars()
	local w = media({ session("b", "Song", false) }).widgets.song
	eq(w.peek.button.icon, "play", "play")
	check(w.peek.viz == nil, "no bars while paused")
	check(media({ session("b", "Song", true) }, { visualizer = false }).widgets.song.peek.viz == nil, "the setting off")
end

function tests.the_buttons_press_the_player()
	media({ session("b", "Playing", true) })
	eq(act("toggle").media, { action = "play_pause", source = "b" }, "toggle")
	eq(act("back").media.value, -5, "back")
	eq(act("seek", { fraction = 0.5 }).media.value, 60, "seek")
	eq(act("rate", { value = 2 }).media, { action = "set_rate", source = "b", value = 2 }, "rate")
	eq(act("pick", { source = "a" }).media, { action = "focus", source = "a" }, "pick")
end
