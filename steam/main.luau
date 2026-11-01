--!strict
-- Steam: the games installed (their covers, the last played first), one pressed to play it, and a search among them. At rest: the
-- game you play now (its icon, its name), else one that updates (how far), else Steam's logo (its own: steam.png).
--
-- NADI reads Steam's own files for it (`steam`): the games, the one running, and where Steam keeps each cover ("steamart:<id>").

local MAX = 48 -- covers shown at most

local library = {}
local running: number? = nil
local since: number? = nil -- when the game running began (as seen here)
local filter = ""

local function span(ms: number): string
	local m = ms // 60000
	if m < 1 then
		return "just now"
	elseif m < 60 then
		return m .. "m"
	end
	return string.format("%dh %02dm", m // 60, m % 60)
end

local function draw(now: number)
	local playing, updating = nil, nil
	for _, g in library do
		if running and g.id == running then
			playing = g
		end
		if g.updating and updating == nil then
			updating = g
		end
	end
	local items = {}
	local q = string.lower(filter)
	for _, g in library do
		if #items >= MAX then
			break
		end
		if q == "" or string.find(string.lower(g.name), q, 1, true) then
			table.insert(items, { image = "steamart:" .. g.id, title = g.name, action = "play:" .. g.id, progress = g.progress })
		end
	end
	local body = { { kind = "field", placeholder = "Search", action = "search", live = true, value = filter }, { kind = "covers", items = items } }
	local peek, tip
	if playing then
		-- (the game's own icon is on the dock and the Game widget: here, Steam's)
		peek = { image = "own:steam.png", image_icon = "own:steam-icon.png", icon = "gamepad", big = true }
		tip = playing.name .. "\nPlaying for " .. span(now - (since or now))
	elseif updating then
		peek = { icon = "download", ring = { part = updating.progress or 0 }, text = updating.name }
		tip = updating.name .. "\nUpdating"
	else
		local last = library[1]
		peek = { image = "own:steam.png", image_icon = "own:steam-icon.png", icon = "gamepad", big = true }
		tip = if last then "Steam\n" .. last.name else "Steam"
	end
	return { widgets = { library = { peek = peek, panel = { width = 380, body = body }, tip = tip } } }
end

function handle(ev)
	if ev.event == "start" then
		library, running, since, filter = {}, nil, nil, ""
		return nil
	elseif ev.event == "steam" then
		library = ev.games or {}
		local now_running = if (ev.running or 0) ~= 0 then ev.running else nil
		if now_running ~= running then
			running = now_running
			since = if running then ev.now else nil
		end
		return draw(ev.now)
	elseif ev.event == "action" then
		local a = ev.action or ""
		local id = string.match(a, "^play:(%d+)$")
		if id then
			return { open = "steam://rungameid/" .. id }
		end
		if a == "search" then
			filter = (ev.args and ev.args.text) or ""
			return draw(ev.now)
		end
	end
	return nil
end
