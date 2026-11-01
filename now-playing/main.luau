--!strict
-- Now Playing: what is playing. At rest: its cover (the other players' covers just behind it), the song over who sings it, and three
-- bars that dance with the sound in the cover's colour; its icon alone, the bars alone. The wheel over it brings the next player
-- forward. Opened: the cover first, large, the panel washed in its colours; the song, who sings it; a bar to seek on; play big in the
-- middle, skip beside it; the speed, when the player takes one; and, several playing, a chip for each over it all (pressed: shown;
-- its bars pressed: played or paused). It reads the `media` sensor and presses the player's buttons through `media_control`. The
-- cover is never handled here: the widget names it ("media:<player>:<id>") and the shell draws the picture it already holds.

local RATES = { 0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3 }

local sessions = {}
local shown_source: string? = nil -- the player chosen (the wheel, a chip); none: the one that began playing last
local last_source: string? = nil -- the player shown last (paused, it stays shown)
local began: { [string]: number } = {} -- when each player last began playing
local was: { [string]: boolean } = {}

local function titled()
	local out = {}
	for _, s in sessions do
		if s.title and s.title ~= "" then
			table.insert(out, s)
		end
	end
	return out
end

-- the one shown: the one chosen (while it is there), else the one that began playing last, else the first with a title
local function primary()
	local list = titled()
	if shown_source then
		for _, s in list do
			if s.source == shown_source then
				return s
			end
		end
	end
	local best, at = nil, -1
	for _, s in list do
		if s.playing and (began[s.source] or 0) > at then
			best, at = s, began[s.source] or 0
		end
	end
	if best then
		return best
	end
	-- (none playing: the one shown last stays, paused, rather than another taking its place)
	if last_source then
		for _, s in list do
			if s.source == last_source then
				return s
			end
		end
	end
	return list[1] or sessions[1]
end

local function cover_of(s): string?
	return if (s.art_id or 0) ~= 0 then "media:" .. s.source .. ":" .. s.art_id else nil
end

-- the app a player is, by its id
local function app_of(source: string): string
	local l = string.lower(source)
	for _, k in { { "spotify", "Spotify" }, { "msedge", "Edge" }, { "chrome", "Chrome" }, { "firefox", "Firefox" }, { "applemusic", "Apple Music" }, { "zunemusic", "Media Player" }, { "vlc", "VLC" }, { "foobar", "foobar2000" }, { "musicbee", "MusicBee" }, { "tidal", "TIDAL" }, { "deezer", "Deezer" }, { "youtube", "YouTube" } } do
		if string.find(l, k[1], 1, true) then
			return k[2]
		end
	end
	local name = string.match(source, "([^\\/!]+)$") or source
	name = string.gsub(name, "%.exe$", "")
	name = string.match(name, "([^%.]+)$") or name
	return string.upper(string.sub(name, 1, 1)) .. string.sub(name, 2)
end

local function rate_text(r: number): string
	return tostring(math.floor(r * 100 + 0.5) / 100) .. "×"
end

local function clock(sec: number): string
	local t = math.max(0, math.floor(sec + 0.5))
	return string.format("%d:%02d", t // 60, t % 60)
end

local function draw(viz: boolean)
	local s = primary()
	if not s then
		-- (nothing playing: idle, so NADI can hide it)
		return { widgets = { song = { idle = true, peek = { icon = "music", text = "Nothing playing" }, tip = "Nothing playing" } } }
	end
	last_source = s.source
	local cover = cover_of(s)
	local list = titled()
	local busy = false
	for _, o in list do
		busy = busy or o.playing == true
	end
	local stack = {}
	for _, o in list do
		if o.source ~= s.source then
			local c = cover_of(o)
			if c then
				table.insert(stack, c)
			end
		end
	end
	local left = if (s.duration or 0) > 0 then "-" .. clock(s.duration - (s.position or 0)) else nil
	-- opened: the players, the cover, the song, its bar, its buttons, its speed
	local body: { any } = {}
	if #list > 1 then
		local chips = {}
		for _, o in list do
			table.insert(chips, { image = cover_of(o), name = app_of(o.source), on = o.source == s.source, playing = o.playing, action = "pick", toggle = "toggle_src", args = { source = o.source } })
		end
		table.insert(body, { kind = "players", items = chips })
	end
	table.insert(body, { kind = "cover", image = cover, size = 200, press = "focus" })
	table.insert(body, { kind = "songhead", title = s.title, sub = s.artist or "", viz = viz and s.playing, color_from = cover })
	if (s.duration or 0) > 0 then
		table.insert(body, { kind = "seek", value = math.clamp((s.position or 0) / s.duration, 0, 1), action = "seek", position = s.position, duration = s.duration, color_from = cover })
	end
	local transport = {}
	if s.can_previous then
		table.insert(transport, { icon = "prev", label = "Previous", action = "prev" })
	end
	table.insert(transport, { icon = if s.playing then "pause" else "play", label = if s.playing then "Pause" else "Play", action = "toggle", main = true })
	if s.can_next then
		table.insert(transport, { icon = "next", label = "Next", action = "next" })
	end
	table.insert(body, { kind = "controls", items = transport })
	if s.can_rate then
		local options = {}
		for _, r in RATES do
			table.insert(options, { value = r, label = rate_text(r) })
		end
		table.insert(body, { kind = "choice", label = "Speed", action = "rate", value = if (s.rate or 0) > 0 then s.rate else 1, options = options })
	end
	return {
		widgets = {
			song = {
				-- (paused, it stays: idle only when no player is left)
				peek = {
					song = { cover = cover, title = s.title, artist = s.artist or "", playing = viz and s.playing, busy = viz and busy, stack = stack, left = left },
					color_from = cover,
				},
				-- (several players: the wheel over it brings the next forward)
				wheel = if #list > 1 then "next_player" else nil,
				-- (a new song: it comes out of its edge for a moment)
				bump = if s.playing then s.source .. "|" .. s.title else nil,
				panel = { width = 340, body = body, wash = cover, press = "focus" },
				-- (its tip: the cover, the song and who sings it)
				tip = { title = s.title, sub = s.artist or "", tile = { image = cover, icon = "music" } },
			},
		},
	}
end

function handle(ev)
	local viz = ev.settings.visualizer ~= false
	if ev.event == "media" then
		sessions = ev.sessions or {}
		-- (each player that began playing now: noted, so the newest is the one shown)
		for _, s in sessions do
			if s.playing and not was[s.source] then
				began[s.source] = ev.now or 0
				shown_source = nil
			end
			was[s.source] = s.playing == true
		end
		return draw(viz)
	elseif ev.event == "tick" then
		-- (a setting changed: drawn again)
		return draw(viz)
	elseif ev.event ~= "action" then
		return nil
	end
	local a = ev.action
	-- (another player shown: the wheel, or its chip)
	if a == "next_player" then
		local list = titled()
		local s = primary()
		if #list > 1 and s then
			local at = 1
			for i, o in list do
				if o.source == s.source then
					at = i
				end
			end
			local step = if ev.args and ev.args.up then -1 else 1
			shown_source = list[(at - 1 + step) % #list + 1].source
		end
		return draw(viz)
	elseif a == "pick" and ev.args and ev.args.source then
		shown_source = ev.args.source
		return draw(viz)
	elseif a == "toggle_src" and ev.args and ev.args.source then
		return { media = { action = "play_pause", source = ev.args.source } }
	end
	local s = primary()
	if not s then
		return nil
	end
	local function media(action: string, value: number?)
		return { media = { action = action, source = s.source, value = value } }
	end
	if a == "toggle" then
		return media("play_pause")
	elseif a == "prev" then
		return media("previous")
	elseif a == "next" then
		return media("next")
	elseif a == "back" then
		return media("seek_by", -5)
	elseif a == "fwd" then
		return media("seek_by", 5)
	elseif a == "seek" and (s.duration or 0) > 0 then
		return media("seek", math.clamp(ev.args.fraction or 0, 0, 1) * s.duration)
	elseif a == "rate" then
		return media("set_rate", tonumber(ev.args.value) or 1)
	elseif a == "focus" then
		return media("focus")
	end
	return nil
end
