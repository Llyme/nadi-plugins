--!strict
-- Picture: pictures you paste into it (a game's map, a guide, a recipe). At rest: the one shown, small (none yet: a picture icon).
-- Opened: it, as large as the panel lets it be, to zoom (the wheel) and pan (dragging); a double-click fits it again. Over its corner:
-- paste (a new one, from the clipboard) and see-through (popped out as a window over a game, the game shows through it). Over its
-- foot, while the mouse is on it, a row of all of them: pressed, shown; right-clicked, removed. The wheel over it at rest
-- steps through them.
--
-- The shell keeps a pasted picture as a PNG in this plugin's folder ("paste-<when>.png") and tells it the file's name; this plugin
-- keeps the list (its store) and names each "own:<file>". Removed, the file is let go of (`forget`).

type Pic = { file: string, w: number, h: number }

local pics: { Pic } = {}
local shown = 1
local see = false

local function kept()
	return { pics = pics, shown = shown, see = see }
end

local function find(file: string): number?
	for i, p in pics do
		if p.file == file then
			return i
		end
	end
	return nil
end

local function draw()
	local p = pics[shown]
	if not p then
		return {
			widgets = {
				picture = {
					peek = { icon = "image" },
					panel = { width = 380, body = { { kind = "drop", text = "Ctrl + C an image, then press here", tip = "Paste", action = "add", args = { paste = true } } } },
					tip = "Paste a Picture",
				},
			},
			store = kept(),
		}
	end
	local view = {
		kind = "view",
		image = "own:" .. p.file,
		id = p.file,
		buttons = {
			{ icon = "clipboard", tip = "Paste", action = "add", args = { paste = true } },
			{ icon = "eye", tip = "See-Through", on = see, action = "see" },
		},
	}
	local body: { any } = { view }
	local items = {}
	for i, q in pics do
		table.insert(items, {
			image = "own:" .. q.file,
			on = i == shown,
			action = "show",
			args = { i = i },
			menu = {
				{ icon = "trash", label = "Remove", action = "remove:" .. q.file, hold = true },
			},
		})
	end
	-- (over its foot, while the mouse is on it)
	view.album = { items = items, add = { icon = "clipboard", tip = "Paste", action = "add", args = { paste = true } } }
	return {
		widgets = {
			picture = {
				peek = { image = "own:" .. p.file },
				panel = { width = 544, body = body },
				wheel = if #pics > 1 then "next" else nil,
				see_through = see,
			},
		},
		store = kept(),
	}
end

function handle(ev)
	if ev.event == "start" then
		local s = ev.store
		if type(s) == "table" and type(s.pics) == "table" then
			pics = s.pics
			shown = math.clamp(tonumber(s.shown) or 1, 1, math.max(#pics, 1))
			see = s.see == true
		end
		return draw()
	elseif ev.event == "tick" then
		return draw()
	elseif ev.event ~= "action" then
		return nil
	end
	local a: string = ev.action
	local args = ev.args or {}
	if a == "add" then
		-- (nothing on the clipboard that is a picture: no file)
		if type(args.file) ~= "string" then
			return nil
		end
		table.insert(pics, { file = args.file, w = tonumber(args.w) or 0, h = tonumber(args.h) or 0 })
		shown = #pics
	elseif a == "show" then
		shown = math.clamp(tonumber(args.i) or shown, 1, math.max(#pics, 1))
	elseif a == "next" then
		if #pics > 1 then
			local step = if args.up then -1 else 1
			shown = (shown - 1 + step) % #pics + 1
		end
	elseif a == "see" then
		see = not see
	elseif string.sub(a, 1, 7) == "remove:" then
		local file = string.sub(a, 8)
		local i = find(file)
		if i then
			table.remove(pics, i)
			if shown > i or shown > #pics then
				shown = math.max(shown - 1, 1)
			end
			local out = draw()
			out.forget = file
			return out
		end
	end
	return draw()
end
