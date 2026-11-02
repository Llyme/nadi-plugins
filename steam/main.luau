--!strict
-- Steam: the games installed (their covers, the last played first), one pressed to play it, and a search among them. At rest: the
-- game you play now (its icon, its name), else one that updates (how far), else Steam's logo (its own: steam.png).
--
-- Its own program (bin/steam-helper.exe) reads Steam's own files: the games, the one running, and where Steam keeps each one's icon
-- and cover (shown as "img:<path>"); a line each time something changed. It tells NADI its games in turn (`apps`): they are found
-- in the launcher's search, each started through Steam.

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
			table.insert(items, { image = if g.art then "img:" .. g.art else nil, title = g.name, action = "play:" .. g.id, progress = g.progress })
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
	-- (the games, for the launcher's search: each started through Steam, with its own icon)
	local apps = {}
	for _, g in library do
		table.insert(apps, { name = g.name, open = "steam://rungameid/" .. g.id, image = if g.icon then "img:" .. g.icon else nil, badge = "own:steam-icon.png", tip = "Steam" })
	end
	return { widgets = { library = { peek = peek, panel = { width = 380, body = body }, tip = tip } }, apps = apps }
end

function handle(ev)
	if ev.event == "start" then
		library, running, since, filter = {}, nil, nil, ""
		return nil
	elseif ev.event == "helper" and ev.data and ev.data.kind == "library" then
		local lib = ev.data
		library = lib.games or {}
		local now_running = if (lib.running or 0) ~= 0 then lib.running else nil
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
