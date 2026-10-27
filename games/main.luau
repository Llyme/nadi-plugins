--!strict
-- Games: the game you are playing, for how long, and how it runs.
--
-- A game is a program whose window covers its whole screen and has no title bar (NADI tells which windows do: `full`). It stays
-- until its process ends, however often you switch away from it. While one runs, the plugin watches it (`usage`): the CPU, the
-- memory, the graphics card and the video memory it uses, and its frames per second. While a game runs, ambient banners wait
-- (`quiet`). Its icon is never handled here: the widget names the program ("exe:<path>") and the shell draws its icon.

local MAX_GAMES = 4 -- NADI lets a plugin watch four programs
local GRAPH_MAX_MS = 50 -- the graph reads 0 to 50 ms, with a line at 60 and at 30 frames per second
local GRAPH_MARKS = { 16.7, 33.3 }
local VRAM_FULL_GB = 12
local TINT = "#3ea66f"

-- programs that cover the screen and are not games
local IGNORE = {
	["explorer.exe"] = true,
	["shellexperiencehost.exe"] = true,
	["searchhost.exe"] = true,
	["searchapp.exe"] = true,
	["startmenuexperiencehost.exe"] = true,
	["applicationframehost.exe"] = true,
	["textinputhost.exe"] = true,
	["lockapp.exe"] = true,
	["systemsettings.exe"] = true,
	["screenclippinghost.exe"] = true,
	["gamebar.exe"] = true,
	["widgets.exe"] = true,
	["nadi.exe"] = true,
}
-- video players, browsers (F11, a fullscreen video), presentations and remote desktops cover the screen too (parts of the name)
local NOT_GAMES = {
	"potplayer", "vlc", "mpv", "mpc-hc", "mpc-be", "wmplayer", "kmplayer", "gomplayer", "smplayer", "video.ui", "moviesandtv",
	"chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "zoom", "teams", "powerpnt", "acrord", "mstsc", "vmconnect",
	"obs64", "obs32",
}

local games = {} -- the one most recently in front first

local function is_not_game(exe: string): boolean
	local e = string.lower(exe)
	if IGNORE[e] then
		return true
	end
	for _, n in NOT_GAMES do
		if string.find(e, n, 1, true) then
			return true
		end
	end
	return false
end

-- a name from the file when the program has none: `deep_rock-galactic.exe` is "Deep Rock Galactic"
local function name_from_path(path: string): string
	local file = string.match(path, "([^\\/]+)$") or path
	local stem = string.gsub(file, "%.[eE][xX][eE]$", "")
	local words = {}
	for w in string.gmatch(stem, "[^_%-]+") do
		table.insert(words, string.upper(string.sub(w, 1, 1)) .. string.sub(w, 2))
	end
	return table.concat(words, " ")
end

local function duration(ms: number): string
	local s = ms // 1000
	local h, m = s // 3600, (s % 3600) // 60
	if h > 0 then
		return h .. "h " .. m .. "m"
	elseif m > 0 then
		return m .. "m"
	end
	return "<1m"
end

local function pct(v: number): string
	return math.floor(v + 0.5) .. "%"
end

local function round(v: number): number
	return math.floor(v + 0.5)
end

-- one game, as a section of the panel
local function section_of(g, now: number)
	local body: { any } = {}
	local u = g.usage
	if u then
		if u.fps then
			table.insert(body, {
				kind = "stats",
				items = {
					{ value = tostring(round(u.fps)), label = "FPS" },
					{ value = tostring(round(u.low_fps or u.fps)), label = "1% low" },
					{ value = string.format("%.1f ms", 1000 / math.max(u.fps, 1)), label = "Frame time" },
				},
			})
			if u.frame_ms and #u.frame_ms > 1 then
				local ms = {}
				for _, v in u.frame_ms do
					table.insert(ms, round(v * 10) / 10)
				end
				table.insert(body, { kind = "graph", values = ms, max = GRAPH_MAX_MS, marks = GRAPH_MARKS })
			end
		end
		local rows = {}
		if u.gpu then
			table.insert(rows, { title = "GPU", sub = pct(u.gpu), progress = u.gpu / 100 })
		end
		if u.vram_gb then
			table.insert(rows, { title = "VRAM", sub = string.format("%.1f GB", u.vram_gb), progress = math.min(1, u.vram_gb / VRAM_FULL_GB) })
		end
		table.insert(rows, { title = "CPU", sub = pct(u.cpu or 0), progress = (u.cpu or 0) / 100 })
		table.insert(rows, { title = "RAM", sub = string.format("%.1f GB", u.ram_gb or 0), progress = (u.ram or 0) / 100 })
		table.insert(body, { kind = "list", rows = rows })
	end
	return {
		kind = "section",
		id = "g" .. g.pid,
		title = g.name,
		sub = "Playing for " .. duration(now - g.since),
		tile = { image = "exe:" .. g.path, icon = "gamepad", color = TINT },
		body = body,
		press = "focus:" .. g.pid,
	}
end

local function draw(now: number)
	local watch = {}
	for _, g in games do
		table.insert(watch, g.pid)
	end
	local front = games[1]
	if not front then
		-- (no game: idle, so NADI can hide it)
		return { widgets = { game = { idle = true, peek = { icon = "gamepad", text = "No game" }, tip = "No game" } }, watch = {}, quiet = false }
	end
	local body = {}
	for _, g in games do
		table.insert(body, section_of(g, now))
	end
	local u = front.usage
	local text = if u and u.fps then round(u.fps) .. " fps" elseif u and u.gpu then "GPU " .. pct(u.gpu) else duration(now - front.since)
	local bars = nil
	if u then
		bars = { { part = (u.gpu or 0) / 100, color = "#8adb6e" }, { part = (u.cpu or 0) / 100, color = "#5ac8e6" }, { part = (u.ram or 0) / 100, color = "#b47ee6" } }
	end
	local more = if #games > 1 then " · +" .. (#games - 1) else ""
	return {
		widgets = {
			game = {
				peek = { image = "exe:" .. front.path, icon = "gamepad", color = TINT, text = text, label = front.name .. " · " .. text .. more, bars = bars },
				panel = { title = front.name, sub = "Playing for " .. duration(now - front.since) .. more, tile = { image = "exe:" .. front.path, icon = "gamepad", color = TINT }, body = body },
				tip = front.name .. " · " .. text,
			},
		},
		watch = watch,
		quiet = true,
	}
end

-- the windows that cover their screen join; the one in front goes first
local function see_windows(ev)
	local seen, at = {}, {}
	for _, w in ev.windows or {} do
		if w.full and not w.tool and (w.path or "") ~= "" and not is_not_game(w.exe or "") and not at[w.pid] then
			at[w.pid] = true
			table.insert(seen, w)
		end
	end
	for i = #seen, 1, -1 do
		local w = seen[i]
		local found = nil
		for j, g in games do
			if g.pid == w.pid then
				found = j
				break
			end
		end
		if found then
			table.insert(games, 1, table.remove(games, found))
		else
			table.insert(games, 1, { pid = w.pid, name = if (w.name or "") ~= "" then w.name else name_from_path(w.path), path = w.path, since = ev.now })
		end
	end
	while #games > MAX_GAMES do
		table.remove(games)
	end
end

function handle(ev)
	if ev.event == "start" then
		games = {}
		return nil
	elseif ev.event == "processes" then
		see_windows(ev)
		return draw(ev.now)
	elseif ev.event == "usage" then
		for _, u in ev.usage or {} do
			for _, g in games do
				if g.pid == u.pid then
					g.usage = u
				end
			end
		end
		-- a game lasts until its process ends
		local kept = {}
		for _, g in games do
			local gone = false
			for _, u in ev.usage or {} do
				if u.pid == g.pid and not u.alive then
					gone = true
				end
			end
			if not gone then
				table.insert(kept, g)
			end
		end
		games = kept
		return draw(ev.now)
	elseif ev.event == "action" then
		local pid = tonumber(string.match(ev.action or "", "^focus:(%d+)$"))
		for _, g in games do
			if g.pid == pid then
				return { focus = { exe = g.path } }
			end
		end
	end
	return nil
end
