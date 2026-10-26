--!strict
-- Now Playing: what is playing, at rest the cover, the song and play/pause (with three bars that dance with the sound), and open,
-- the transport, a bar to seek on, a jump of 5 seconds each way, the speed, and the other players. It reads the `media` sensor and
-- presses the player's buttons through `media_control`. The cover is never handled here: the widget names it ("media:<player>:<id>")
-- and the shell draws the picture it already holds.
--
-- It follows one player: the one that plays, else the first with a title.

local RATES = { 0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3 }
local TINT = "#c23a7a"

local sessions = {}

local function primary()
	for _, s in sessions do
		if s.playing then
			return s
		end
	end
	for _, s in sessions do
		if s.title and s.title ~= "" then
			return s
		end
	end
	return sessions[1]
end

local function rate_text(r: number): string
	return tostring(math.floor(r * 100 + 0.5) / 100) .. "×"
end

local function transport(s)
	local b = {}
	if s.can_previous then
		table.insert(b, { icon = "prev", label = "Previous", action = "prev" })
	end
	table.insert(b, { icon = if s.playing then "pause" else "play", label = if s.playing then "Pause" else "Play", action = "toggle" })
	if s.can_next then
		table.insert(b, { icon = "next", label = "Next", action = "next" })
	end
	return b
end

local function draw(viz: boolean)
	local s = primary()
	if not s then
		return { widgets = { song = {} } }
	end
	local cover = if (s.art_id or 0) ~= 0 then "media:" .. s.source .. ":" .. s.art_id else nil
	local body: { any } = {}
	if (s.duration or 0) > 0 then
		local seek: any = { kind = "seek", value = math.clamp(s.position / s.duration, 0, 1), action = "seek", position = s.position, duration = s.duration }
		if s.can_seek then
			seek.before = { { icon = "back", label = "Back 5 seconds", action = "back" } }
			seek.after = { { icon = "fwd", label = "Forward 5 seconds", action = "fwd" } }
		end
		table.insert(body, seek)
	end
	table.insert(body, { kind = "controls", items = transport(s) })
	if s.can_rate then
		local options = {}
		for _, r in RATES do
			table.insert(options, { value = r, label = rate_text(r) })
		end
		table.insert(body, { kind = "choice", label = "Speed", action = "rate", value = if (s.rate or 0) > 0 then s.rate else 1, options = options })
	end
	local others = {}
	for _, o in sessions do
		if o.source ~= s.source and o.title and o.title ~= "" then
			table.insert(others, { title = o.title, sub = o.artist or "", action = "pick", args = { source = o.source } })
		end
	end
	if #others > 0 then
		table.insert(body, { kind = "list", rows = others })
	end
	local label = if s.artist and s.artist ~= "" then s.title .. " · " .. s.artist else s.title
	return {
		widgets = {
			song = {
				peek = {
					image = cover,
					icon = "music",
					viz = viz and s.playing or nil,
					color = TINT,
					text = s.title,
					label = label,
					button = { icon = if s.playing then "pause" else "play", label = if s.playing then "Pause" else "Play", action = "toggle" },
				},
				panel = { title = s.title, sub = s.artist or "", tile = { image = cover, icon = "music", color = TINT }, body = body, press = "focus" },
				tip = label,
			},
		},
	}
end

function handle(ev)
	local viz = ev.settings.visualizer ~= false
	if ev.event == "media" then
		sessions = ev.sessions or {}
		return draw(viz)
	elseif ev.event == "tick" then
		-- (a setting changed: drawn again)
		return draw(viz)
	elseif ev.event ~= "action" then
		return nil
	end
	local s = primary()
	if not s then
		return nil
	end
	local function media(action: string, value: number?)
		return { media = { action = action, source = s.source, value = value } }
	end
	local a = ev.action
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
	elseif a == "pick" then
		return { media = { action = "focus", source = ev.args.source } }
	end
	return nil
end
