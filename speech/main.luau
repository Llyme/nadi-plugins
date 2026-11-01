--!strict
-- Speech: writes down what is said, from the microphone (one person, or a room of them) and from what the computer plays, and tells the
-- voices apart: each person an animal until named, known again next time. Its program (bin/speech-helper.exe) does the hearing; this
-- script runs it and shows it: a dot that breathes while it listens, and the transcript in its panel (a script: each person's animal
-- and time in a gutter, their lines beside it; a line pressed plays what was said). It gives the `speech` service to other plugins:
--
--   calls:  listen { mic?, system? }   stop   toggle   sources { mic, system }   name { speaker, name }
--   events: speech.draft { id, text, speaker, name, avatar, tint, me, source }   (what is being said, so far)
--           speech.line { uid, id, text, speaker, name, avatar, tint, me, at, time, date, source, session, start, end }
--           speech.drop { id, source }   (a draft that was nothing)
--           speech.revised { session, lines = { { uid, text, speaker, name } } }   (a session gone over again, carefully)
--           speech.status { state, text?, progress?, since?, mic, system }

local MAX_LINES = 400

local ANIMALS = { "🦊", "🐼", "🐸", "🦉", "🐧", "🐙", "🦄", "🐯", "🐰", "🐻", "🐝", "🐢", "🦋", "🐳", "🐨", "🦔" }
-- a person's ground (dark, behind their animal) and their colour (light, their bar of talk)
local TINTS = { "#3a2a16", "#262a33", "#1f3324", "#33242f", "#2a2440", "#3a2020", "#203238", "#38331a" }
local BRIGHT = { "#e8a04a", "#8aa4d6", "#7cc48a", "#d68ab8", "#a58ae0", "#e07a7a", "#6cc4d0", "#d6c45a" }
local ME_TINT = "#173a27"

local KEEP = { never = -1, ["1"] = 1, ["7"] = 7, ["30"] = 30, forever = 0 }

local state = "idle" -- idle, downloading, loading, listening, error
local status_text = ""
local progress: number? = nil
local checking = false
local settings: any = {}
local src = { mic = true, system = true }

local lines: { any } = {}
local drafts: { [string]: any } = {} -- source -> what is being said now
local voices: { [string]: any } = {} -- id -> { name, animal, me }
local since: number? = nil -- when it began listening (ms)

local view = "live" -- live, sessions, past
local sessions: { any } = {}
local past: any = nil -- { name, lines }
local editing: string? = nil -- the person whose card is open
local shares = false -- how long each spoke, shown
local playing: any = nil -- { uid, since, secs, words }
local fresh: { [string]: number } = {} -- uid -> when it came back better
local tz = 0

local function listening(): boolean
	return state == "listening" or state == "downloading" or state == "loading"
end

local function num(id: string?): number
	return tonumber(string.match(id or "", "%d+") or "1") or 1
end

local function tint(id: string?): string
	local v = id and voices[id]
	if v and v.me then
		return ME_TINT
	end
	return TINTS[(num(id) - 1) % #TINTS + 1]
end

local function bright(id: string?): string
	local v = id and voices[id]
	if v and v.me then
		return "#5ac88c"
	end
	return BRIGHT[(num(id) - 1) % #BRIGHT + 1]
end

local function avatar(id: string?, name: string?): string
	local v = id and voices[id]
	if v and v.animal and v.animal ~= "" then
		return v.animal
	end
	if name and #name <= 8 and not string.match(name, "^%w") then
		return name -- (an animal already)
	end
	return "👤"
end

-- the name shown beside them: a name given, else none (their animal says who)
local function shown(id: string?): string
	local v = id and voices[id]
	if v and v.name and v.name ~= "" then
		return v.name
	end
	return ""
end

local function is_me(id: string?): boolean
	local v = id and voices[id]
	return v ~= nil and v.me == true
end

local function span(secs: number): string
	local t = math.max(0, math.floor(secs))
	if t >= 3600 then
		return string.format("%d:%02d:%02d", t // 3600, t // 60 % 60, t % 60)
	end
	return string.format("%d:%02d", t // 60, t % 60)
end

local function listen_cmd(args)
	args = args or {}
	if args.mic ~= nil then
		src.mic = args.mic
	end
	if args.system ~= nil then
		src.system = args.system
	end
	return {
		cmd = "listen",
		mic = src.mic,
		system = src.system,
		model = settings.model or "small",
		keep_days = KEEP[settings.keep_audio or "7"] or 7,
	}
end

-- what other plugins are told of a line or a draft: who, with their animal and ground
local function dressed(d)
	local o = table.clone(d)
	o.avatar = avatar(d.speaker, d.name)
	o.tint = tint(d.speaker)
	o.me = is_me(d.speaker)
	return o
end

-- ---------------------------------------------------------------------------------------------------------------------- days

local MONTHS = { "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December" }
local WEEKDAYS = { "Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday" }

local function day_of(ms: number): number
	return (ms // 1000 + tz * 60) // 86400
end

local function civil(days: number): (number, number, number)
	local z = days + 719468
	local era = z // 146097
	local doe = z - era * 146097
	local yoe = (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365
	local y = yoe + era * 400
	local doy = doe - (365 * yoe + yoe // 4 - yoe // 100)
	local mp = (5 * doy + 2) // 153
	local d = doy - (153 * mp + 2) // 5 + 1
	local m = if mp < 10 then mp + 3 else mp - 9
	return (if m <= 2 then y + 1 else y), m, d
end

local function day_name(ms: number, now: number): string
	local d, today = day_of(ms), day_of(now)
	if d == today then
		return "Today"
	elseif d == today - 1 then
		return "Yesterday"
	end
	local _, m, dd = civil(d)
	return WEEKDAYS[d % 7 + 1] .. ", " .. dd .. " " .. MONTHS[m]
end

local function clock_of(ms: number): string
	local secs = (ms // 1000 + tz * 60) % 86400
	local h, m = secs // 3600, secs // 60 % 60
	return string.format("%d:%02d %s", (h + 11) % 12 + 1, m, if h < 12 then "AM" else "PM")
end

-- ---------------------------------------------------------------------------------------------------------------------- drawing

-- the people in these lines, in the order they first spoke
local function people_of(ls)
	local seen, out = {}, {}
	for _, l in ls do
		if l.speaker and not seen[l.speaker] then
			seen[l.speaker] = true
			table.insert(out, l.speaker)
		end
	end
	return out
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

local function script_of(ls, now: number)
	local items = {}
	local day = nil
	for _, l in tail(ls, 330, 46000) do
		local at = l.at or now
		local dn = day_name(at, now)
		if dn ~= day then
			day = dn
			table.insert(items, { day = dn .. ", " .. clock_of(at) })
		end
		local can = l.uid ~= nil and l.session ~= nil and l.start ~= nil and settings.keep_audio ~= "never"
		local p = playing ~= nil and l.uid ~= nil and playing.uid == l.uid
		local nm = shown(l.speaker)
		if nm == "" and l.name and string.match(l.name, "^%w") and not (l.speaker and voices[l.speaker]) then
			nm = l.name -- (one from before voices were kept: their name as it was written)
		end
		table.insert(items, {
			speaker = l.speaker,
			name = nm,
			avatar = avatar(l.speaker, l.name),
			tint = tint(l.speaker),
			me = is_me(l.speaker),
			time = if l.at then string.gsub(clock_of(l.at), " [AP]M$", "") else nil,
			text = l.text,
			action = if can then "play:" .. l.uid else nil,
			tip = if can then "Play" else nil,
			playing = if p then { since = playing.since, secs = playing.secs, words = playing.words } else nil,
			fresh = if l.uid then fresh[l.uid] else nil,
			avatar_action = if l.speaker then "person:" .. l.speaker else nil,
			avatar_tip = if shown(l.speaker) ~= "" then shown(l.speaker) else "Name",
		})
	end
	return items
end

local function person_card(body)
	local id = editing :: string
	local v = voices[id] or {}
	table.insert(body, { kind = "person", avatar = avatar(id, nil), tint = tint(id), field = { placeholder = "Name", action = "rename", value = v.name or "" } })
	local zoo = {}
	for _, a in ANIMALS do
		table.insert(zoo, { text = a, action = "animal:" .. a, on = avatar(id, nil) == a })
	end
	table.insert(body, { kind = "picker", items = zoo })
	-- (you; and the others it may be the same person as)
	local row = { { icon = "person", action = "me", on = v.me == true, tip = "Me" } }
	local others = people_of(if view == "past" and past then past.lines else lines)
	local any = false
	for _, o in others do
		if o ~= id then
			if not any then
				table.insert(row, { icon = "merge", tip = "Merge" })
				any = true
			end
			table.insert(row, { text = avatar(o, nil), tint = tint(o), action = "merge:" .. o, tip = if shown(o) ~= "" then shown(o) else "Merge" })
		end
	end
	table.insert(body, { kind = "picker", items = row })
end

local function shares_of(ls)
	local talk, order = {}, people_of(ls)
	local most = 0
	for _, l in ls do
		if l.speaker and l.start and l["end"] then
			talk[l.speaker] = (talk[l.speaker] or 0) + (l["end"] - l.start)
			most = math.max(most, talk[l.speaker])
		end
	end
	table.sort(order, function(a, b)
		return (talk[a] or 0) > (talk[b] or 0)
	end)
	local rows = {}
	for _, id in order do
		local t = talk[id] or 0
		table.insert(rows, { avatar = avatar(id, nil), tint = tint(id), value = if most > 0 then t / most else 0, color = bright(id), text = span(t) })
	end
	return { kind = "shares", rows = rows }
end

local function status_marks(items)
	if state == "downloading" then
		table.insert(items, { kind = "icon", icon = "download", color = "#e8b04a", tip = status_text })
		table.insert(items, { kind = "meter", value = progress or 0, color = "#e8b04a", w = 70 })
	elseif state == "error" then
		table.insert(items, { kind = "icon", icon = "bang", color = "#e86a6a", tip = status_text })
	elseif checking then
		table.insert(items, { kind = "icon", icon = "sparkle", on = true, tip = "Checking" })
	end
end

-- At rest, a voice: its state (listening, going over a session, its model coming, gone wrong, or off), since when it listens (the
-- shell keeps the time going), how far its model has come, and who is talking now (their animal, shown when it is large).
local function voice_now(on: boolean)
	local st = "off"
	if state == "downloading" or state == "loading" then
		st = "loading"
	elseif state == "error" then
		st = "error"
	elseif on then
		st = "listening"
	elseif checking then
		st = "review"
	end
	local v: any = { state = st, since = if st == "listening" then since else nil, progress = progress }
	if on then
		for _, d in drafts do
			if d.speaker then
				v.who = { avatar = avatar(d.speaker, d.name), tint = tint(d.speaker) }
				break
			end
		end
	end
	return v
end

local function people_mark(ls)
	local ps = {}
	local talking = {}
	for _, d in drafts do
		if d.speaker then
			talking[d.speaker] = true
		end
	end
	for _, id in people_of(ls) do
		table.insert(ps, { avatar = avatar(id, nil), tint = tint(id), ring = talking[id] == true })
	end
	if #ps == 0 then
		return nil
	end
	return { kind = "people", items = ps, action = "shares", tip = "People" }
end

local function draw(now: number)
	local on = listening()
	local body = {}
	local bar = {}
	if view == "sessions" then
		bar = { { kind = "icon", icon = "back", action = "back", tip = "Back" }, { kind = "grow" }, { kind = "icon", icon = "folder", action = "folder", tip = "Folder" } }
		table.insert(body, { kind = "toolbar", items = bar })
		local items = {}
		local day = nil
		for _, s in sessions do
			local at = s.at or now
			local dn = day_name(at, now)
			if dn ~= day then
				day = dn
				table.insert(items, { day = dn })
			end
			local ps = {}
			for _, id in s.people or {} do
				table.insert(ps, { avatar = avatar(id, nil), tint = tint(id) })
			end
			table.insert(items, { people = ps, title = clock_of(at), right = span(s.secs or 0), sub = s.first or "", action = "open:" .. s.name })
		end
		table.insert(body, { kind = "sessions", items = items })
	else
		local ls = if view == "past" and past then past.lines else lines
		if view == "past" then
			table.insert(bar, { kind = "icon", icon = "back", action = "sessions", tip = "Back" })
			table.insert(bar, { kind = "grow" })
		else
			table.insert(bar, { kind = "rec", on = on, action = "toggle", tip = if on then "Stop" else "Listen" })
			if on and since then
				table.insert(bar, { kind = "clock", since = since })
			end
			table.insert(bar, { kind = "grow" })
		end
		local pm = people_mark(ls)
		if pm then
			table.insert(bar, pm)
		end
		status_marks(bar)
		if view == "past" then
			table.insert(bar, { kind = "icon", icon = "folder", action = "folder", tip = "Folder" })
		else
			table.insert(bar, {
				kind = "toggles",
				items = {
					{ icon = "mic", on = src.mic, action = "src:mic", tip = "Microphone" },
					{ icon = "speaker", on = src.system, action = "src:system", tip = "System Audio" },
				},
			})
			table.insert(bar, { kind = "icon", icon = "clock", action = "sessions", tip = "Sessions" })
		end
		table.insert(body, { kind = "toolbar", items = bar, shimmer = checking })
		if editing then
			person_card(body)
		end
		if shares then
			table.insert(body, shares_of(ls))
		end
		local items = script_of(ls, now)
		if view == "live" then
			for _, s in { "mic", "system" } do
				local d = drafts[s]
				if d then
					table.insert(items, {
						speaker = d.speaker,
						name = shown(d.speaker),
						avatar = avatar(d.speaker, d.name),
						tint = tint(d.speaker),
						me = is_me(d.speaker),
						text = d.text,
						draft = true,
					})
				end
			end
		end
		table.insert(body, { kind = "script", items = items })
	end
	return {
		widgets = {
			speech = {
				peek = { voice = voice_now(on) },
				-- (it begins or stops listening: it comes out of its edge for a moment)
				bump = if on then "listening" else "off",
				tip = if on then "Listening" elseif checking then "Going Over" elseif state == "error" then (if status_text ~= "" then status_text else "Speech") else "Speech",
				panel = { width = 420, height = 520, stick = "bottom", body = body },
			},
		},
	}
end

local function merge(a, b)
	for k, v in b do
		a[k] = v
	end
	return a
end

local function status_event()
	return { name = "speech.status", data = { state = state, text = status_text, progress = progress, since = since, mic = src.mic, system = src.system, checking = checking } }
end

local function revise(ls, by, now)
	for _, l in ls do
		local r = l.uid and by[l.uid]
		if r then
			if r.text and r.text ~= l.text then
				fresh[l.uid] = now
			end
			l.text, l.speaker, l.name = r.text or l.text, r.speaker or l.speaker, r.name or l.name
			if r.words ~= nil then
				l.words = r.words
			end
		end
	end
end

local function find(uid: string)
	for _, ls in { lines, if past then past.lines else {} } do
		for _, l in ls do
			if l.uid == uid then
				return l
			end
		end
	end
	return nil
end

function handle(ev)
	settings = ev.settings or settings
	tz = ev.tz_offset or tz
	local now = ev.now or 0
	if ev.event == "start" then
		state, lines, drafts, voices, editing, shares, view, past, playing, fresh = "idle", {}, {}, {}, nil, false, "live", nil, nil, {}
		src = { mic = settings.mic ~= false, system = settings.system ~= false }
		return draw(now)
	elseif ev.event == "helper" then
		local d = ev.data or {}
		if d.kind == "status" then
			if d.state == "checking" then
				checking = true
				return merge(draw(now), { emit = status_event() })
			end
			if d.checked then
				checking = false
			end
			state = d.state or state
			status_text = d.what or d.text or ""
			progress = d.progress
			if d.session then
				lines, since = {}, now
			end
			if not listening() then
				since = nil
			end
			return merge(draw(now), { emit = status_event() })
		elseif d.kind == "voices" then
			voices = {}
			for _, v in d.list or {} do
				voices[v.id] = v
			end
			return draw(now)
		elseif d.kind == "draft" then
			drafts[d.source or "mic"] = d
			return merge(draw(now), { emit = { name = "speech.draft", data = dressed(d) } })
		elseif d.kind == "drop" then
			drafts[d.source or "mic"] = nil
			return merge(draw(now), { emit = { name = "speech.drop", data = d } })
		elseif d.kind == "line" then
			drafts[d.source or "mic"] = nil
			table.insert(lines, d)
			if #lines > MAX_LINES then
				table.remove(lines, 1)
			end
			return merge(draw(now), { emit = { name = "speech.line", data = dressed(d) } })
		elseif d.kind == "revised" then
			local by = {}
			for _, r in d.lines or {} do
				if r.uid then
					by[r.uid] = r
				end
			end
			revise(lines, by, now)
			if past and past.name == d.session then
				revise(past.lines, by, now)
			end
			return merge(draw(now), { emit = { name = "speech.revised", data = d } })
		elseif d.kind == "sessions" then
			sessions = d.list or {}
			return draw(now)
		elseif d.kind == "session" then
			past = { name = d.name, lines = d.lines or {} }
			for _, l in past.lines do
				l.session = d.name
			end
			view, editing, shares = "past", nil, false
			return draw(now)
		elseif d.kind == "stopped" then
			state = "idle"
			return merge(draw(now), { emit = status_event() })
		end
		return nil
	elseif ev.event == "call" then
		local what, args = ev.what, ev.args
		if what == "listen" then
			return { helper = listen_cmd(args) }
		elseif what == "stop" then
			return { helper = { cmd = "stop" } }
		elseif what == "toggle" then
			return { helper = if state == "listening" then { cmd = "stop" } else listen_cmd(args) }
		elseif what == "sources" and type(args) == "table" then
			src.mic, src.system = args.mic ~= false, args.system ~= false
			return merge(draw(now), { helper = { cmd = "sources", mic = src.mic, system = src.system }, emit = status_event() })
		elseif what == "name" and type(args) == "table" and args.speaker and args.name then
			return { helper = { cmd = "name", speaker = args.speaker, name = args.name } }
		end
		return nil
	elseif ev.event == "action" then
		local a = ev.action or ""
		if a == "toggle" then
			return { helper = if state == "listening" then { cmd = "stop" } else listen_cmd(nil) }
		elseif a == "src:mic" or a == "src:system" then
			local k = string.sub(a, 5)
			src[k] = not src[k]
			if not src.mic and not src.system then
				src[if k == "mic" then "system" else "mic"] = true
			end
			return merge(draw(now), { helper = { cmd = "sources", mic = src.mic, system = src.system }, emit = status_event() })
		elseif a == "sessions" then
			view, editing, shares = "sessions", nil, false
			return merge(draw(now), { helper = { cmd = "sessions" } })
		elseif a == "back" then
			view, past = "live", nil
			return draw(now)
		elseif a == "folder" then
			return { helper = { cmd = "open_sessions" } }
		elseif a == "shares" then
			shares = not shares
			return draw(now)
		elseif a == "me" and editing then
			return { helper = { cmd = "me", speaker = editing, on = not is_me(editing) } }
		elseif a == "rename" and editing then
			local name = (ev.args and ev.args.text) or ""
			return { helper = { cmd = "name", speaker = editing, name = name } }
		end
		local name = string.match(a, "^open:(.+)$")
		if name then
			return { helper = { cmd = "open", session = name } }
		end
		local uid = string.match(a, "^play:(.+)$")
		if uid then
			local l = find(uid)
			if l and l.start and l["end"] then
				playing = { uid = uid, since = now, secs = (l["end"] - l.start) + 0.3, words = l.words }
				return merge(draw(now), { helper = { cmd = "play", session = l.session, source = l.source or "mic", start = l.start, ["end"] = l["end"] } })
			end
			return nil
		end
		local who = string.match(a, "^person:(.+)$")
		if who then
			editing = if editing == who then nil else who
			return draw(now)
		end
		local animal = string.match(a, "^animal:(.+)$")
		if animal and editing then
			return { helper = { cmd = "animal", speaker = editing, animal = animal } }
		end
		local from = string.match(a, "^merge:(.+)$")
		if from and editing then
			return { helper = { cmd = "merge", into = editing, from = from } }
		end
	end
	return nil
end
