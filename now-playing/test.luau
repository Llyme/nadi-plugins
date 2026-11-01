-- The tests of Now Playing: `nadi-plugin test now-playing`.

local function session(source: string, title: string, playing: boolean)
	return { source = source, title = title, artist = "Artist", playing = playing, position = 30, duration = 120, can_previous = true, can_next = true, can_seek = true, can_rate = true, rate = 1.5, art_id = 7 }
end

local function media(list, settings: any?, now: number?)
	return handle({ event = "media", now = now or 0, settings = settings or {}, sessions = list })
end

local function act(a: string, args: any?)
	return handle({ event = "action", now = 0, settings = {}, action = a, args = args or {} })
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

function tests.it_follows_the_player_that_plays()
	local w = media({ session("a", "Paused", false), session("b", "Playing", true) }).widgets.song
	eq(w.peek.song.title, "Playing", "title")
	eq(w.peek.song.cover, "media:b:7", "the cover, by name")
	eq(w.peek.song.playing, true, "bars")
	eq(w.peek.song.stack, { "media:a:7" }, "the other player's cover behind it")
	eq(w.peek.song.left, "-1:30", "how long is left")
	eq(w.wheel, "next_player", "the wheel: the next player")
	eq(find(w.panel.body, "players").items[2].on, true, "its chip, chosen")
	eq(find(w.panel.body, "seek").value, 0.25, "a quarter in")
	eq(find(w.panel.body, "controls").items[2].main, true, "play, big")
	eq(find(w.panel.body, "choice").value, 1.5, "speed")
	eq(w.panel.wash, "media:b:7", "washed in its cover")
	eq((media({}).widgets.song).idle, true, "nothing playing: gone (idle)")
end

function tests.paused_and_the_setting()
	local w = media({ session("b", "Song", false) }).widgets.song
	eq(w.peek.song.playing, false, "no bars while paused")
	eq(find(w.panel.body, "controls").items[2].icon, "play", "play")
	eq(media({ session("b", "Song", true) }, { visualizer = false }).widgets.song.peek.song.playing, false, "the setting off")
end

function tests.the_newest_is_shown_and_the_wheel_steps()
	media({ session("a", "First", true) }, nil, 1)
	local w = media({ session("a", "First", true), session("b", "Second", true) }, nil, 2).widgets.song
	eq(w.peek.song.title, "Second", "the one that began last")
	local n = act("next_player", { up = false }).widgets.song
	eq(n.peek.song.title, "First", "the wheel: the next")
	local p = act("pick", { source = "b" }).widgets.song
	eq(p.peek.song.title, "Second", "a chip: shown")
	eq(act("toggle_src", { source = "a" }).media, { action = "play_pause", source = "a" }, "its bars: played or paused")
end

function tests.the_buttons_press_the_player()
	media({ session("b", "Playing", true) })
	eq(act("toggle").media, { action = "play_pause", source = "b" }, "toggle")
	eq(act("seek", { fraction = 0.5 }).media.value, 60, "seek")
	eq(act("rate", { value = 2 }).media, { action = "set_rate", source = "b", value = 2 }, "rate")
end

function tests.paused_it_stays()
	media({ session("x", "Other", false), session("y", "Mine", true) }, nil, 1)
	local w = media({ session("x", "Other", false), session("y", "Mine", false) }, nil, 2).widgets.song
	eq(w.peek.song.title, "Mine", "the one shown last stays when paused")
	eq(w.idle, nil, "paused: not idle")
end
