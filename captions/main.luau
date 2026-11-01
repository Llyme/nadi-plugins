--!strict
-- Captions: live subtitles of what is said, from the Speech service. At rest, a pictogram (and, while it listens, the animals of who
-- spoke last); what is being said comes out as a live banner; opened, everything said as a script (each person's animal
-- and time in a gutter, their lines beside it, larger), a bar on top to listen or stop and to choose what is heard. Someone talking:
-- a live banner, with who and their words typed in as they come, there while they talk.

local MAX = 500

local lines = {}
local drafts = {} -- source -> what is being said now
local listening = false
local since: number? = nil
local src = { mic = true, system = true }
local newest = nil -- the line or draft that came last
local tz = 0

-- the talking banner stays while someone talks, and goes this long after the last word
local BANNER_MS = 2500

local function clock_of(ms: number): string
	local secs = (ms // 1000 + tz * 60) % 86400
	return string.format("%d:%02d", ((secs // 3600) + 11) % 12 + 1, secs // 60 % 60)
end

-- A banner for who is talking, live: their animal and name, their words typed in as they come; it stays while they talk.
local function banner(ev, d)
	if ev.settings and ev.settings.banner == false then
		return nil
	end
	local text = d.text or ""
	-- (its newest words, as many as a banner holds)
	if #text > 180 then
		text = string.sub(text, #text - 179)
		text = string.gsub(text, "^%S*%s", "")
	end
	if text == "" then
		return nil
	end
	local name = if d.name and string.match(d.name, "^%w") then d.name else ""
	return { { title = name, text = text, priority = 30, glow = false, pill = { key = "talking", live = true, avatar = d.avatar or "👤", tint = d.tint, ms = BANNER_MS } } }
end

local function item(l, draft: boolean)
	return {
		speaker = l.speaker,
		name = if l.name and string.match(l.name, "^%w") then l.name else "",
		avatar = l.avatar or "👤",
		tint = l.tint,
		me = l.me,
		time = if l.at and not draft then clock_of(l.at) else nil,
		text = l.text,
		draft = draft,
	}
end


-- The newest lines that fit what NADI takes of a widget (64 KB): the rest are in the session's file, and in Sessions.
local function tail(ls, per: number, budget: number)
	local from, cost = #ls + 1, 0
	while from > 1 do
		local l = ls[from - 1]
		cost += #(l.text or "") + per
		if cost > budget or #ls - from + 2 > 160 then
			break
		end
		from -= 1
	end
	return table.move(ls, from, #ls, 1, {})
end

local function draw()
	-- (at rest: a pictogram; while it listens, the animals of who spoke last, the newest first; what they say is the banner's)
	local peek = { icon = "chat" }
	if listening then
		local seen, faces = {}, {}
		for i = #lines, 1, -1 do
			local a = lines[i].avatar
			if a and not seen[a] then
				seen[a] = true
				table.insert(faces, a)
				if #faces == 3 then
					break
				end
			end
		end
		if #faces > 0 then
			peek.text = table.concat(faces, "")
		end
	end
	local bar = { { kind = "rec", on = listening, action = "toggle", tip = if listening then "Stop" else "Listen" } }
	if listening and since then
		table.insert(bar, { kind = "clock", since = since })
	end
	table.insert(bar, { kind = "grow" })
	table.insert(bar, {
		kind = "toggles",
		items = {
			{ icon = "mic", on = src.mic, action = "src:mic", tip = "Microphone" },
			{ icon = "speaker", on = src.system, action = "src:system", tip = "System Audio" },
		},
	})
	local items = {}
	for _, l in tail(lines, 160, 50000) do
		table.insert(items, item(l, false))
	end
	for _, s in { "mic", "system" } do
		local d = drafts[s]
		if d then
			table.insert(items, item(d, true))
		end
	end
	local body = { { kind = "toolbar", items = bar }, { kind = "script", items = items, size = 15 } }
	return { widgets = { live = { peek = peek, tip = "Captions", panel = { width = 460, height = 460, stick = "bottom", body = body } } } }
end

function handle(ev)
	tz = ev.tz_offset or tz
	if ev.event == "start" then
		lines, listening, drafts, newest, since = {}, false, {}, nil, nil
		local out = draw()
		out.subscribe = { "speech.line", "speech.draft", "speech.drop", "speech.status", "speech.revised" }
		return out
	elseif ev.event == "speech.draft" then
		local d = { speaker = ev.speaker, name = ev.name or "", avatar = ev.avatar, tint = ev.tint, me = ev.me, text = ev.text or "", draft = true }
		drafts[ev.source or "mic"] = d
		newest = d
		local out = draw()
		out.say = banner(ev, d)
		return out
	elseif ev.event == "speech.drop" then
		drafts[ev.source or "mic"] = nil
		return draw()
	elseif ev.event == "speech.line" then
		drafts[ev.source or "mic"] = nil
		table.insert(lines, { uid = ev.uid, speaker = ev.speaker, name = ev.name or "", avatar = ev.avatar, tint = ev.tint, me = ev.me, text = ev.text or "", at = ev.at })
		if #lines > MAX then
			table.remove(lines, 1)
		end
		newest = lines[#lines]
		local out = draw()
		out.say = banner(ev, newest)
		return out
	elseif ev.event == "speech.revised" then
		local by = {}
		for _, r in ev.lines or {} do
			if r.uid then
				by[r.uid] = r
			end
		end
		for _, l in lines do
			local r = l.uid and by[l.uid]
			if r then
				l.text, l.name = r.text or l.text, r.name or l.name
			end
		end
		return draw()
	elseif ev.event == "speech.status" then
		listening = ev.state == "listening" or ev.state == "downloading" or ev.state == "loading"
		since = ev.since
		if ev.mic ~= nil then
			src.mic, src.system = ev.mic, ev.system
		end
		local out = draw()
		-- (no longer listening: nobody is talking)
		if not listening then
			out.cancel = { "talking" }
		end
		return out
	elseif ev.event == "action" then
		if ev.action == "toggle" then
			return { call = { service = "speech", what = "toggle" } }
		elseif ev.action == "src:mic" or ev.action == "src:system" then
			local k = string.sub(ev.action, 5)
			local want = { mic = src.mic, system = src.system }
			want[k] = not want[k]
			if not want.mic and not want.system then
				return nil
			end
			return { call = { service = "speech", what = "sources", args = want } }
		end
	end
	return nil
end
