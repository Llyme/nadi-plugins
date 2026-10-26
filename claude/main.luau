--!strict
-- Claude: your Claude Code sessions, a banner when one changes state, and how much of your Claude limits is used.
--
-- Sessions. NADI reads the files Claude Code keeps and tells the plugin what each session is doing (`claude`, every 2 s), for every
-- session whose process is still there. The plugin decides what a session is (waiting for you, working, finished, idle) and draws a
-- section for each in its panel, the ones that need you first. A session stays as long as it runs: it goes when its source is gone
-- (the editor was closed, the terminal exited), never because it was looked at. The Claude desktop app is listed too while it runs.
-- A banner comes when a session changes state: it starts working, needs you (with Allow and Deny on it when answering is set up),
-- wants a reply, is done, stopped, went quiet, opened or closed. Each can be switched off; several at once are one banner. It sees
-- which window is in front (`processes`), so it does not interrupt you with a session you are looking at.
--
-- Usage. It signs in to Claude on its own, the way the Claude Code CLI does (OAuth with PKCE: the browser opens, the page shows a
-- code, the code is pasted back). Its tokens are its own, kept in its sealed store and renewed by the plugin, so the numbers come
-- whether or not Claude Code runs. Claude Code's own login is never read: a refresh token is good for one use, so sharing it would
-- break the CLI's login.

-- ---------------------------------------------------------------------------------------------------------------- sessions

local FINISHED_MS = 15 * 60000 -- idle again after a turn, this recently: finished, and the result waits for you
local STARTED_MS = 3000 -- a session that works this long has started: a quick answer is not news
local ASK_MS = 2500 -- a request the host does not hold is said after this long (better to wait for the buttons)
local QUIET_MS = 5 * 60000
local MAX_SESSIONS = 24
local TILE, AMBER, GREEN, RED, GREY = "#d97757", "#e8b04a", "#5ac88c", "#e87a7a", "#6d6d75"

local sessions = {} -- once each (the one that changed last)
local seen = {} -- what the banners know of each session, by id
local primed = false -- the first look was had: what is there already is not news
local front: { exe: string, title: string }? = nil
local desktop: { exe: string, title: string }? = nil

local function basename(p: string): string
	return string.match(p or "", "([^\\/]+)[\\/]*$") or (p or "")
end

local function project_of(s): string
	return basename(s.cwd or "")
end

local function kind_of(s): string
	if (s.waiting_for or "") ~= "" or string.find(s.status or "", "wait", 1, true) then
		return "waiting"
	elseif s.status == "busy" then
		return "busy"
	end
	return "idle"
end

-- "waiting", "working", "finished" or "idle"
local function state_of(s, now: number): string
	local k = kind_of(s)
	if k == "waiting" then
		return "waiting"
	elseif k == "busy" then
		return "working"
	elseif s.model and now - (s.updated or 0) <= FINISHED_MS then
		return "finished"
	end
	return "idle"
end

local function context_limit(model: string?): number
	return if string.find(string.lower(model or ""), "haiku", 1, true) then 200000 else 1000000
end

-- "claude-sonnet-5-5" -> "Sonnet 5.5"; "claude-haiku-4-5-20251001" -> "Haiku 4.5"
local function model_name(model: string): string
	local rest = string.gsub(model, "^claude%-", "")
	local parts = string.split(rest, "-")
	local family = parts[1] or ""
	if family == "" then
		return ""
	end
	local name = string.upper(string.sub(family, 1, 1)) .. string.sub(family, 2)
	local version = {}
	for i = 2, #parts do
		if #parts[i] <= 2 and string.match(parts[i], "^%d+$") then
			table.insert(version, parts[i])
		end
	end
	if #version > 0 then
		name ..= " " .. table.concat(version, ".")
	end
	return name
end

local function tokens(n: number): string
	if n >= 1000000 then
		return string.format("%.1fM", n / 1e6)
	end
	return math.floor(n / 1000 + 0.5) .. "k"
end

local function fill_color(p: number): string
	return if p > 0.85 then RED elseif p > 0.65 then AMBER else GREEN
end

local function used(s): number
	return math.min(1, (s.context_tokens or 0) / context_limit(s.model))
end

-- what the session is doing, as an icon and a line
local function activity(s, state: string): (string, string)
	if state == "waiting" then
		local w = string.lower(s.waiting_for or "")
		if string.find(w, "perm", 1, true) or string.find(w, "approv", 1, true) then
			return "phone", "Waiting for your approval"
		elseif string.find(w, "input", 1, true) or string.find(w, "answer", 1, true) or string.find(w, "question", 1, true) then
			return "phone", "Waiting for your answer"
		end
		return "phone", "Waiting for you"
	elseif state == "working" then
		if s.last then
			return s.last.icon, s.last.text
		end
		return "sparkle", "Thinking"
	elseif state == "finished" then
		return "check", "Finished"
	end
	return "moon", "Idle"
end

local function title_of(s): string
	if s.title and s.title ~= "" then
		return s.title
	elseif s.name and s.name ~= "" then
		return s.name
	end
	return project_of(s)
end

-- "2m 14s", "45s", "1h 03m"
local function clock_dur(secs: number): string
	secs = math.max(0, math.floor(secs))
	local h, m, sec = secs // 3600, (secs % 3600) // 60, secs % 60
	if h > 0 then
		return string.format("%dh %02dm", h, m)
	elseif m > 0 then
		return string.format("%dm %02ds", m, sec)
	end
	return sec .. "s"
end

local function ago(ms: number): string
	local m = ms // 60000
	if m < 1 then
		return "just now"
	elseif m < 60 then
		return m .. " min ago"
	end
	return (m // 60) .. "h ago"
end

local function mode_chip(mode: string)
	if mode == "" then
		return nil
	elseif mode == "plan" then
		return { icon = "shield", text = "Plan", tone = "hot" }
	elseif mode == "auto" or mode == "acceptEdits" or mode == "accept-edits" or mode == "autoAccept" then
		return { icon = "shield", text = "Auto-accept", tone = "hot" }
	elseif mode == "bypassPermissions" or mode == "bypass" then
		return { icon = "shield", text = "Bypass", tone = "hot" }
	elseif mode == "normal" or mode == "default" then
		return { icon = "shield", text = "Default", tone = "" }
	end
	return { icon = "shield", text = string.upper(string.sub(mode, 1, 1)) .. string.sub(mode, 2), tone = "" }
end

local function wants(tool: string): string
	if tool == "Bash" or tool == "PowerShell" then
		return "Wants to run"
	elseif tool == "Edit" or tool == "Write" or tool == "MultiEdit" or tool == "NotebookEdit" then
		return "Wants to change"
	elseif tool == "WebFetch" or tool == "WebSearch" then
		return "Wants to look up"
	end
	return "Wants to use"
end

local function plural(n: number, word: string): string
	return n .. " " .. word .. (if n == 1 then "" else "s")
end

local function stats(s)
	return {
		kind = "stats",
		items = {
			{ value = tostring(#(s.files or {})), label = "Files" },
			{ value = tostring(s.commands or 0), label = "Commands" },
			{ value = tostring(s.agents or 0), label = "Agents" },
			{ value = string.format("$%.2f", s.cost_usd or 0), label = "Cost" },
		},
	}
end

-- One session, as a section of the panel.
local function section_of(s, now: number, state: string)
	local icon, text = activity(s, state)
	local frac = used(s)
	local ctx = { kind = "meter", label = "Context", part = frac, text = tokens(s.context_tokens or 0) .. " / " .. tokens(context_limit(s.model)), color = fill_color(frac) }
	local body: { any } = {}
	local chips = {}
	if (s.branch or "") ~= "" then
		table.insert(chips, { icon = "branch", text = s.branch, tone = "" })
	end
	local mc = mode_chip(s.mode or "")
	if mc then
		table.insert(chips, mc)
	end
	if s.model then
		local m = model_name(s.model)
		if m ~= "" then
			table.insert(chips, { icon = "sparkle", text = if (s.effort or "") ~= "" then m .. " · " .. s.effort else m, tone = "" })
		end
	end
	if #chips > 0 then
		table.insert(body, { kind = "chips", items = chips })
	end
	local turn = if (s.turn_started or 0) > 0 then (now - s.turn_started) // 1000 else 0
	local plan = {}
	for i, t in s.todos or {} do
		if i > 8 then
			break
		end
		table.insert(plan, { state = t.status, text = t.text })
	end
	if state == "working" then
		table.insert(body, {
			kind = "strip",
			text = if turn > 0 then "Working for " .. clock_dur(turn) else "Working",
			side = if (s.thinking_ms or 0) >= 1000 then "Thought " .. (s.thinking_ms // 1000) .. "s" else "",
			tone = "working",
		})
		if #plan > 0 then
			table.insert(body, { kind = "checklist", title = "Plan", items = plan })
		end
		local rows = { { icon = icon, text = text, now = true } }
		for i, f in s.feed or {} do
			if i > 2 then
				break
			end
			table.insert(rows, { icon = f.icon, text = f.text, now = false })
		end
		table.insert(body, { kind = "lines", rows = rows })
		table.insert(body, stats(s))
		table.insert(body, ctx)
	elseif state == "waiting" then
		local ask = if s.asking and s.asking.kind == "permission" then s.asking else nil
		local call = ask or s.pending
		local detail = if call then call.detail or "" else ""
		if detail ~= "" or ask then
			local waited = (if ask then ask.age_ms or 0 else now - (s.updated or 0)) // 1000
			local block: any = {
				kind = "ask",
				label = wants(if call then call.tool or "" else ""),
				side = if waited >= 5 then "waiting " .. clock_dur(waited) else "",
				icon = if call then call.icon or "term" else "term",
				text = detail,
				buttons = {},
			}
			if ask then
				block.buttons = { { label = "Allow", action = "allow:" .. ask.id, tone = "ok" }, { label = "Deny", action = "deny:" .. ask.id, tone = "no" } }
				block.field = "Tell Claude what to do instead…"
				block.action = "denywith:" .. ask.id
			end
			table.insert(body, block)
		end
		if #plan > 0 then
			table.insert(body, { kind = "checklist", title = "Plan", items = plan })
		end
		table.insert(body, stats(s))
	elseif state == "finished" then
		local took = if (s.turn_started or 0) > 0 then clock_dur(((s.updated or 0) - s.turn_started) // 1000) else ""
		table.insert(body, { kind = "strip", text = if took == "" then "Done" else "Done in " .. took, side = ago(now - (s.updated or 0)), tone = "finished" })
		if (s.reply or "") ~= "" then
			table.insert(body, { kind = "text", label = "Result", text = s.reply })
		end
		local files = s.files or {}
		if #files > 0 then
			local rows = {}
			for i, f in files do
				if i > 6 then
					break
				end
				table.insert(rows, { name = f.name, added = f.added, removed = f.removed })
			end
			table.insert(body, { kind = "files", title = "Changed · " .. #files, rows = rows })
		end
		table.insert(body, {
			kind = "stats",
			items = {
				{ value = "+" .. (s.lines_added or 0), label = "Added" },
				{ value = "−" .. (s.lines_removed or 0), label = "Removed" },
				{ value = tostring(s.errors or 0), label = "Errors" },
				{ value = string.format("$%.2f", s.cost_usd or 0), label = "Cost" },
			},
		})
		table.insert(body, ctx)
		if s.asking and s.asking.kind == "stop" then
			table.insert(body, { kind = "field", placeholder = "Reply to Claude…", hint = "while the turn is open", action = "reply:" .. s.asking.id })
		end
	else
		table.insert(body, { kind = "strip", text = "Quiet", side = "last " .. ago(now - (s.updated or 0)), tone = "idle" })
		table.insert(body, ctx)
	end
	local host = s.host or {}
	return {
		kind = "section",
		id = s.id,
		title = title_of(s),
		sub = text,
		live = { icon = icon, tone = state },
		tile = { text = "C", color = TILE, ring = frac, ring_color = fill_color(frac) },
		badge = { icon = host.icon or "term", tip = (host.label or "") .. "\n" .. project_of(s) },
		body = body,
		press = "focus:" .. s.id,
	}
end

local RANK = { waiting = 0, finished = 1, working = 2, idle = 3 }

local function front_is(exe: string, title: string): boolean
	return front ~= nil and string.lower(basename(exe)) == front.exe and string.lower(title) == front.title
end

-- the sessions widget: at rest how they stand (the one that needs you first), open a section for each
local function sessions_widget(now: number)
	local list = {}
	for _, s in sessions do
		table.insert(list, { s = s, state = state_of(s, now) })
	end
	table.sort(list, function(a, b)
		if RANK[a.state] ~= RANK[b.state] then
			return RANK[a.state] < RANK[b.state]
		end
		return (a.s.updated or 0) > (b.s.updated or 0)
	end)
	local body = {}
	local waiting, working = 0, 0
	for i, x in list do
		if i > MAX_SESSIONS then
			break
		end
		if x.state == "waiting" then
			waiting += 1
		elseif x.state == "working" then
			working += 1
		end
		table.insert(body, section_of(x.s, now, x.state))
	end
	if desktop then
		local d = desktop
		table.insert(body, {
			kind = "section",
			id = "desktop",
			title = "Claude Desktop",
			sub = if front_is(d.exe, d.title) then "In front" else "Running",
			live = { icon = "sparkle", tone = "idle" },
			tile = { text = "C", color = TILE },
			body = {},
			press = "focus:desktop",
		})
	end
	if #body == 0 then
		return {}
	end
	local top = list[1]
	local tone = if top then top.state else "idle"
	local text = if waiting > 1 then waiting .. " need you"
		elseif waiting == 1 then "Needs you"
		elseif working > 1 then working .. " working"
		elseif working == 1 then "Working"
		elseif #body == 1 then (if top then title_of(top.s) else "Claude Desktop")
		else plural(#body, "session")
	local label = if top then title_of(top.s) .. " · " .. select(2, activity(top.s, top.state)) else "Claude Desktop"
	return {
		peek = { live = { icon = "claude", tone = tone }, color = if waiting > 0 then AMBER else TILE, text = text, label = label, dot = waiting > 0 or nil },
		panel = { title = "Claude", sub = if waiting > 0 then text else plural(#body, "session"), tile = { icon = "claude", color = TILE }, body = body },
		tip = label,
	}
end

-- Is the window that holds this session the one in front? Then you can see it already. An editor holds one window for each folder:
-- only the one that shows this project counts.
local function in_front(s): boolean
	local host = s.host or {}
	if not host.exe or not front then
		return false
	end
	if string.lower(basename(host.exe)) ~= front.exe then
		return false
	end
	return host.icon == "term" or string.find(front.title, string.lower(project_of(s)), 1, true) ~= nil
end

-- a banner: the circle is the state, the ring is full unless the session works (then a part of it sweeps round) or rests
local function tile_for(tone: string)
	local color, ring = GREY, nil
	if tone == "working" then
		color, ring = TILE, 0.25
	elseif tone == "permission" or tone == "reply" or tone == "waiting" then
		color, ring = AMBER, 1
	elseif tone == "finished" then
		color, ring = GREEN, 1
	elseif tone == "stopped" then
		color, ring = RED, 1
	end
	return { text = "C", color = color, ring = ring, ring_color = if ring then color else nil }
end

local function banner(key: string, title: string, tone: string, sub: string, ms: number, prio: number, press: string, buttons: { any }?)
	local pill: any = { icon = "sparkle", right = "", key = key, tone = tone, press = press, sub = sub, tile = tile_for(tone), ms = ms }
	if buttons and #buttons > 0 then
		pill.buttons = buttons
	end
	return { text = title, priority = prio, ambient = true, pill = pill }
end

local function clip(text: string, n: number): string
	local t = string.gsub(text, "%s+", " ")
	t = string.match(t, "^%s*(.-)%s*$") or t
	if utf8.len(t) and (utf8.len(t) :: number) <= n then
		return t
	end
	local cut = utf8.offset(t, n + 1) or (#t + 1)
	return (string.gsub(string.sub(t, 1, cut - 1), "%s+$", "")) .. "…"
end

local function needs_you(s, ask)
	local id = s.id
	local permission = ask ~= nil or s.waiting_for == "permission"
	local call = ask or s.pending
	local detail = if call then call.detail or "" else ""
	if permission then
		local sub = if detail ~= "" then wants(call.tool or "") .. " " .. clip(detail, 80) else "Needs your answer"
		if ask then
			return banner(id, title_of(s), "permission", sub, 12000, 90, "focus:" .. id, {
				{ label = "Allow", action = "allow:" .. ask.id, tone = "ok", icon = "check" },
				{ label = "Deny", action = "deny:" .. ask.id, tone = "no", icon = "x" },
			})
		end
		return banner(id, title_of(s), "permission", sub, 8000, 90, "focus:" .. id)
	end
	local sub = if detail ~= "" then "Asks: " .. clip(detail, 80) else "Wants a reply"
	return banner(id, title_of(s), "reply", sub, 12000, 90, "focus:" .. id)
end

local function held_reply(s)
	local sub = if (s.reply or "") == "" then "Done · wants a reply" else "Says: " .. clip(s.reply, 80)
	return banner(s.id, title_of(s), "reply", sub, 12000, 90, "focus:" .. s.id)
end

local function moment(s, tone: string, sub: string, ms: number, prio: number)
	return { id = s.id, title = title_of(s), tone = tone, sub = sub, ms = ms, prio = prio }
end

-- "2 done · 1 started working": several sessions at once, as one banner
local function merged(moments)
	local function count(tone: string): number
		local n = 0
		for _, m in moments do
			if m.tone == tone then
				n += 1
			end
		end
		return n
	end
	local parts = {}
	for _, p in { { "stopped", "stopped" }, { "finished", "done" }, { "working", "started working" }, { "quiet", "quiet" }, { "info", "new" }, { "closed", "closed" } } do
		local n = count(p[1])
		if n > 0 then
			table.insert(parts, n .. " " .. p[2])
		end
	end
	local tone = "info"
	for _, t in { "stopped", "finished", "working", "info", "quiet", "closed" } do
		if count(t) > 0 then
			tone = t
			break
		end
	end
	local ms, prio = 0, 0
	for _, m in moments do
		ms = math.max(ms, m.ms)
		prio = math.max(prio, m.prio)
	end
	return banner("claude-many", #moments .. " sessions", tone, table.concat(parts, " · "), ms, prio, "focus:" .. moments[1].id)
end

local function on(settings, key: string, default: boolean): boolean
	local v = settings[key]
	if v == nil then
		return default
	end
	return v == true
end

-- Watch the sessions go by, and say what changed. Several things at once are one banner; what needs you never is (its buttons are
-- its own). What was said of a session that waited is taken back when it has been answered.
local function alerts(now: number, settings)
	local says, cancel = {}, {}
	if not on(settings, "alerts", true) then
		seen = {}
		primed = false
		return says, cancel
	end
	local cfg = {
		start = on(settings, "b_start", true),
		needs = on(settings, "b_needs", true),
		done = on(settings, "b_done", true),
		stop = on(settings, "b_stop", true),
		quiet = on(settings, "b_quiet", false),
		open = on(settings, "b_open", false),
		skip_ms = (settings.skip or 10) * 1000,
	}
	local announce = primed
	local moments = {}
	local present = {}
	for _, s in sessions do
		present[s.id] = true
		local kind = kind_of(s)
		local sn = seen[s.id] or {}
		local prev = sn.kind
		local visible = announce and not in_front(s)
		local function turn_start(): number
			return if (s.turn_started or 0) > 0 then s.turn_started else s.updated or 0
		end
		if prev == nil then
			sn.since = turn_start()
			sn.errors = s.errors or 0
			sn.started = true
			sn.quiet = true
			if cfg.open and visible then
				table.insert(moments, moment(s, "info", "New session · " .. ((s.host or {}).label or ""), 3000, 30))
			end
		end
		if prev == "waiting" and kind ~= "waiting" then
			table.insert(cancel, s.id)
			sn.asked = false
		end
		if kind ~= "idle" and prev == "idle" then
			sn.since = turn_start()
			sn.errors = s.errors or 0
			sn.started = false
			sn.quiet = false
		end
		if kind == "waiting" then
			local ask = if s.asking and s.asking.kind == "permission" then s.asking else nil
			if not sn.asked and (ask or now - (s.updated or 0) >= ASK_MS) then
				sn.asked = true
				if cfg.needs and visible then
					table.insert(says, needs_you(s, ask))
				end
			end
		elseif kind == "busy" then
			if not sn.started and now - sn.since >= STARTED_MS then
				sn.started = true
				if cfg.start and visible then
					table.insert(moments, moment(s, "working", "Started working · " .. ((s.host or {}).label or ""), 3000, 40))
				end
			end
		else
			if prev == "busy" or prev == "waiting" then
				local updated = s.updated or 0
				local took = if (s.turn_started or 0) > 0 and s.turn_started <= updated then updated - s.turn_started else math.max(0, updated - sn.since)
				sn.quiet = false
				if took >= cfg.skip_ms and visible then
					local errors = math.max(0, (s.errors or 0) - (sn.errors or 0))
					if s.asking and s.asking.kind == "stop" then
						if cfg.needs then
							table.insert(says, held_reply(s))
						end
					elseif errors > 0 and cfg.stop then
						table.insert(moments, moment(s, "stopped", "Stopped · " .. plural(errors, "error"), 6000, 60))
					elseif cfg.done then
						local sub = "Done in " .. clock_dur(took // 1000)
						local files = #(s.files or {})
						if files > 0 then
							sub ..= " · " .. plural(files, "file") .. " changed"
						end
						table.insert(moments, moment(s, "finished", sub, 5000, 60))
					end
				end
			elseif not sn.quiet and now - (s.updated or 0) >= QUIET_MS then
				sn.quiet = true
				if cfg.quiet and visible and s.model then
					table.insert(moments, moment(s, "quiet", "Quiet for " .. ((now - s.updated) // 60000) .. " min", 3000, 30))
				end
			end
		end
		sn.kind = kind
		sn.last = s
		seen[s.id] = sn
	end
	for id, sn in seen do
		if not present[id] then
			seen[id] = nil
			if sn.kind == "waiting" then
				table.insert(cancel, id)
			end
			if cfg.open and announce and sn.last then
				table.insert(moments, moment(sn.last, "closed", "Session closed", 3000, 30))
			end
		end
	end
	if #moments == 1 then
		local m = moments[1]
		table.insert(says, banner(m.id, m.title, m.tone, m.sub, m.ms, m.prio, "focus:" .. m.id))
	elseif #moments > 1 then
		table.insert(says, merged(moments))
	end
	primed = true
	return says, cancel
end

-- the Claude desktop app's window (`claude.exe` with a window: the command line tool has none, and lives in `.local` or `.claude`)
local function is_desktop(w): boolean
	local path = string.lower(w.path or "")
	return w.exe == "claude.exe" and not w.tool and not string.find(path, ".claude", 1, true) and not string.find(path, "\\.local\\", 1, true)
end

local function once_each(list)
	local best, at = {}, {}
	for _, s in list or {} do
		local i = at[s.id]
		if i == nil then
			table.insert(best, s)
			at[s.id] = #best
		elseif (s.updated or 0) > (best[i].updated or 0) then
			best[i] = s
		end
	end
	return best
end

-- ------------------------------------------------------------------------------------------------------------------- usage

local USAGE_URL = "https://api.anthropic.com/api/oauth/usage"
local TOKEN_URL = "https://console.anthropic.com/v1/oauth/token"
local AUTHORIZE_URL = "https://claude.ai/oauth/authorize"
local REDIRECT_URL = "https://console.anthropic.com/oauth/code/callback"
local SCOPE = "org:create_api_key user:profile user:inference"
local CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e" -- the public OAuth client of the Claude Code CLI
local BETA = "oauth-2025-04-20"
local USER_AGENT = "claude-code/2.1.0" -- Cloudflare in front of both hosts blocks default library names
local OAUTH_AGENT = "nadi/1.1" -- the token endpoint rate-limits that name for everyone, so the sign-in says who it is
local POLL_MS = 120000 -- asking every 5 s got it rate-limited
local POLL_EMPTY_MS = 20000
local MANUAL_COOLDOWN_MS = 60000
local RENEW_BEFORE_MS = 60000
local FIVE, SEVEN, WARM, FULL = "#d97757", "#e8d5b5", "#e6a15a", "#e5675c"

local login: { access: string, refresh: string, expires_ms: number }? = nil
local snap: any = {} -- { available, error, needs_login, five = { pct, resets_at }, seven }
local pending: { verifier: string, state: string }? = nil
local last_poll: number? = nil
local last_manual: number? = nil
local inflight: string? = nil
local renewed = false
local last_pct: { any } = {}

local function reset_in(resets_at: string?, now: number): string
	local at = if resets_at then nadi.iso(resets_at) else nil
	if not at then
		return ""
	end
	local mins = math.max(0, (at - now) // 60000)
	if mins < 60 then
		return mins .. "m"
	end
	local hours = mins // 60
	if hours < 24 then
		return hours .. "h " .. (mins % 60) .. "m"
	end
	return (hours // 24) .. "d " .. (hours % 24) .. "h"
end

local function bar_color(pct: number, own: string): string
	return if pct >= 100 then FULL elseif pct >= 80 then WARM else own
end

local function token_request(out, id: string, body)
	out.fetch = { { id = id, method = "POST", url = TOKEN_URL, headers = { ["Content-Type"] = "application/json", Accept = "application/json", ["User-Agent"] = OAUTH_AGENT }, body = nadi.json.encode(body) } }
end

local function usage_request(out, access: string)
	out.fetch = { { id = "usage", method = "GET", url = USAGE_URL, headers = { Authorization = "Bearer " .. access, ["anthropic-beta"] = BETA, Accept = "application/json", ["User-Agent"] = USER_AGENT } } }
end

local function keep(): any
	return if login then { login = login } else { login = false }
end

local function failed(msg: string, needs: boolean)
	snap = { available = true, error = msg, needs_login = needs, five = snap.five, seven = snap.seven }
end

local function renew(out)
	local t = login
	if not t or t.refresh == "" then
		failed("The sign-in has no way to renew itself. Sign in again.", true)
		return
	end
	inflight = "renew"
	token_request(out, "renew", { grant_type = "refresh_token", refresh_token = t.refresh, client_id = CLIENT_ID })
end

local function refresh(now: number, out)
	last_poll = now
	local t = login
	if not t then
		snap = { needs_login = true }
		return
	end
	renewed = false
	if t.expires_ms ~= 0 and t.expires_ms < now + RENEW_BEFORE_MS then
		renew(out)
	else
		inflight = "usage"
		usage_request(out, t.access)
	end
end

local function tokens_of(data, keep_refresh: string, now: number)
	if type(data) ~= "table" or type(data.access_token) ~= "string" then
		return nil
	end
	local expires_in = data.expires_in or 0
	return { access = data.access_token, refresh = if type(data.refresh_token) == "string" then data.refresh_token else keep_refresh, expires_ms = if expires_in > 0 then now + expires_in * 1000 else 0 }
end

local function window_of(usage, key: string)
	local w = usage[key]
	if type(w) ~= "table" then
		return nil
	end
	return { pct = w.utilization, resets_at = w.resets_at }
end

-- When a number moved (rounded: a jitter under a percent is not news), a banner slides the bars from the old numbers to the new.
local function on_numbers(out)
	if snap.error or not snap.available then
		return
	end
	local new = { snap.five and snap.five.pct, snap.seven and snap.seven.pct }
	local old = last_pct
	last_pct = new
	local function moved(o, n)
		return o ~= nil and n ~= nil and math.floor(o + 0.5) ~= math.floor(n + 0.5)
	end
	if not (moved(old[1], new[1]) or moved(old[2], new[2])) then
		return
	end
	local function row(label: string, from, to)
		return { label = label, from = (from or 0) / 100, value = math.clamp((to or 0) / 100, 0, 1), text = if to then math.floor(to + 0.5) .. "%" else "" }
	end
	out.say = out.say or {}
	table.insert(out.say, { text = "Claude usage", priority = 30, ambient = true, pill = { icon = "sparkle", key = "claude-usage", tone = "info", ms = 3900, rows = { row("5h", old[1], new[1]), row("7d", old[2], new[2]) } } })
end

local function answered(ev, out)
	local data = nadi.json.decode(ev.body or "") or {}
	local function net(): string
		return if (ev.error or "") ~= "" then ev.error else "HTTP " .. tostring(ev.status)
	end
	local waited = inflight
	inflight = nil
	if waited == "usage" and ev.id == "usage" then
		if ev.status == 200 then
			local usage = data
			if data.five_hour == nil and data.seven_day == nil then
				usage = data.usage or data.data or data
			end
			snap = { available = true, five = window_of(usage, "five_hour"), seven = window_of(usage, "seven_day") }
			on_numbers(out)
		elseif ev.status == 401 and not renewed then
			renewed = true
			renew(out)
		elseif ev.status == 401 then
			failed("The sign-in was refused. Sign in again.", true)
		elseif ev.status == 429 then
			failed("Rate limited by the usage endpoint. Try again in a bit.", false)
		else
			failed(net(), false)
		end
	elseif waited == "renew" and ev.id == "renew" then
		if ev.status == 200 then
			local fresh = tokens_of(data, if login then login.refresh else "", ev.now)
			if fresh then
				login = fresh
				inflight = "usage"
				usage_request(out, fresh.access)
				out.store = keep()
			else
				failed("The sign-in returned no access token.", false)
			end
		elseif ev.status == 400 or ev.status == 401 then
			failed("The sign-in ran out. Sign in again.", true)
		elseif ev.status == 429 then
			failed("Claude is rate limiting the sign-in renewal.", false)
		else
			failed(net(), false)
		end
	elseif waited == "exchange" and ev.id == "exchange" then
		if ev.status == 200 then
			local t = tokens_of(data, "", ev.now)
			if t then
				login = t
				pending = nil
				snap = {}
				out.store = keep()
				refresh(ev.now, out)
			else
				failed("The sign-in returned no access token.", false)
			end
		elseif ev.status == 400 or ev.status == 401 then
			failed("Claude did not accept that code. Start the sign-in again and paste the new one.", true)
		elseif ev.status == 429 then
			failed("Claude is rate limiting the sign-in. Try again in a minute.", false)
		else
			failed(net(), false)
		end
	else
		-- (an answer that is not the one it waited for)
		inflight = waited
	end
end

local function usage_widget(now: number, wanted: boolean)
	if not wanted or not snap.available then
		return {}
	end
	local bars, rows, tip = {}, {}, {}
	for _, w in { { "5h", FIVE, snap.five }, { "7d", SEVEN, snap.seven } } do
		local win = w[3]
		if win and win.pct then
			local back = reset_in(win.resets_at, now)
			local color = bar_color(win.pct, w[2])
			local pct = math.floor(win.pct + 0.5) .. "%"
			table.insert(bars, { part = math.clamp(win.pct / 100, 0, 1), color = color })
			table.insert(rows, { label = w[1], part = math.clamp(win.pct / 100, 0, 1), color = color, text = pct, sub = if back ~= "" then "Back in " .. back else "" })
			table.insert(tip, w[1] .. " " .. pct)
		end
	end
	if #bars == 0 then
		return {}
	end
	local foot = if snap.error then "Not up to date: " .. snap.error else "Press to refresh"
	return {
		peek = { bars = bars, label = "Claude · " .. table.concat(tip, " · ") },
		panel = { title = "Claude Usage", sub = foot, tile = { icon = "claude", color = FIVE }, body = { { kind = "bars", items = rows } }, press = "usage_refresh" },
		tip = "Claude · " .. table.concat(tip, " · "),
	}
end

local function note_of(wanted: boolean): any
	if not wanted then
		return false
	elseif snap.error then
		return "Usage: " .. snap.error
	elseif snap.needs_login then
		return "Usage: sign in below to see how much of your limits is used."
	end
	return false
end

-- ---------------------------------------------------------------------------------------------------------------- handle

function handle(ev)
	local now = ev.now
	local settings = ev.settings or {}
	local wanted = settings.usage ~= false
	local out: any = {}
	local draw_sessions = false
	local e = ev.event
	if e == "start" then
		sessions, seen, primed, front, desktop = {}, {}, false, nil, nil
		snap, pending, last_poll, last_manual, inflight, renewed, last_pct = {}, nil, nil, nil, nil, false, {}
		local l = ev.store and ev.store.login
		login = if type(l) == "table" and type(l.access) == "string" and type(l.refresh) == "string" then { access = l.access, refresh = l.refresh, expires_ms = l.expires_ms or 0 } else nil
		if wanted then
			refresh(now, out)
		end
	elseif e == "processes" then
		front = nil
		desktop = nil
		for _, w in ev.windows or {} do
			if w.front and not front then
				front = { exe = string.lower(w.exe or ""), title = string.lower(w.title or "") }
			end
			if not desktop and is_desktop(w) then
				desktop = { exe = if (w.path or "") ~= "" then w.path else w.exe, title = w.title or "" }
			end
		end
		draw_sessions = true
	elseif e == "claude" then
		sessions = once_each(ev.claude)
		local says, cancel = alerts(now, settings)
		if #says > 0 then
			out.say = says
		end
		if #cancel > 0 then
			out.cancel = cancel
		end
		draw_sessions = true
	elseif e == "fetched" then
		answered(ev, out)
	elseif e == "tick" then
		if not wanted then
			snap, last_pct, inflight = {}, {}, nil
		else
			local every = if snap.available then POLL_MS else POLL_EMPTY_MS
			if inflight == nil and (last_poll == nil or now - last_poll >= every) then
				refresh(now, out)
			end
		end
	elseif e == "action" then
		local a = ev.action or ""
		local text = (ev.args and ev.args.text) or ""
		local id = string.match(a, "^allow:(.+)$")
		if id then
			return { answer = { id = id, behavior = "allow", text = "" } }
		end
		id = string.match(a, "^deny:(.+)$")
		if id then
			return { answer = { id = id, behavior = "deny", text = "" } }
		end
		id = string.match(a, "^denywith:(.+)$")
		if id then
			return { answer = { id = id, behavior = "deny", text = text } }
		end
		id = string.match(a, "^reply:(.+)$")
		if id then
			return { answer = { id = id, behavior = "reply", text = text } }
		end
		local wait = settings.wait or 20
		if a == "answer_on" then
			return { claude_hooks = { action = "install", permission = true, stop = settings.replies == true, wait_s = wait } }
		elseif a == "answer_off" then
			return { claude_hooks = { action = "remove", permission = true, stop = true, wait_s = wait } }
		end
		id = string.match(a, "^focus:(.+)$")
		if id == "desktop" then
			return if desktop then { focus = { exe = desktop.exe } } else nil
		elseif id then
			for _, s in sessions do
				if s.id == id then
					return { focus = { exe = (s.host or {}).exe, title = project_of(s) } }
				end
			end
			return nil
		end
		if a == "usage_login" then
			-- Sign in: the browser opens on Claude's page, which shows a code to paste back
			local verifier = nadi.b64url(nadi.random(32))
			local state = nadi.b64url(nadi.random(32))
			pending = { verifier = verifier, state = state }
			local q = {
				"code=true",
				"client_id=" .. CLIENT_ID,
				"response_type=code",
				"redirect_uri=" .. nadi.urlencode(REDIRECT_URL),
				"scope=" .. nadi.urlencode(SCOPE),
				"code_challenge=" .. nadi.b64url(nadi.sha256(verifier)),
				"code_challenge_method=S256",
				"state=" .. state,
			}
			out.open = AUTHORIZE_URL .. "?" .. table.concat(q, "&")
		elseif a == "usage_finish" then
			-- what the page shows is `code#state`: only the code goes back
			local code = string.match(string.match(text, "^%s*(.-)%s*$") or "", "^([^#]*)") or ""
			code = string.match(code, "^%s*(.-)%s*$") or ""
			if code == "" then
				login, pending, last_pct = nil, nil, {}
				snap = { needs_login = true }
				out.store = keep()
			elseif pending then
				inflight = "exchange"
				token_request(out, "exchange", { grant_type = "authorization_code", client_id = CLIENT_ID, code = code, redirect_uri = REDIRECT_URL, code_verifier = pending.verifier, state = pending.state })
			else
				out.note = "Usage: start the sign-in first."
			end
		elseif a == "usage_refresh" then
			if (last_manual == nil or now - last_manual >= MANUAL_COOLDOWN_MS) and inflight == nil then
				last_manual = now
				refresh(now, out)
			end
		else
			return nil
		end
	else
		return nil
	end
	out.widgets = { usage = usage_widget(now, wanted) }
	if draw_sessions then
		out.widgets.sessions = sessions_widget(now)
	end
	if out.note == nil then
		out.note = note_of(wanted)
	end
	return out
end
