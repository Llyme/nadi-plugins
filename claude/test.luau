-- The tests of Claude: `nadi-plugin test claude`. Each test runs against a plugin of its own.

local T0 = 1800000000000

local function session(id: string, status: string, updated: number, more: { [string]: any }?)
	local s = {
		id = id,
		pid = 100,
		status = status,
		started = T0 - 3600000,
		updated = updated,
		cwd = "C:\\Work\\nadi",
		title = "Fix the thing",
		model = "claude-sonnet-5-5",
		context_tokens = 90000,
		cost_usd = 1.5,
		lines_added = 12,
		lines_removed = 3,
		feed = { { icon = "term", text = "Running cargo build" }, { icon = "pen", text = "Editing lib.rs" } },
		last = { icon = "pen", text = "Editing lib.rs" },
		host = { label = "VS Code", icon = "code", exe = "C:\\Program Files\\Microsoft VS Code\\Code.exe" },
		files = {},
		todos = {},
		errors = 0,
	}
	for k, v in more or {} do
		s[k] = v
	end
	return s
end

local function rich(s)
	s.branch = "fix/oauth"
	s.mode = "plan"
	s.effort = "high"
	s.turn_started = T0 - 134000
	s.thinking_ms = 12000
	s.todos = { { status = "done", text = "Read the sensor" }, { status = "now", text = "Show every session" }, { status = "next", text = "Tests" } }
	s.files = { { name = "lib.rs", added = 84, removed = 31 }, { name = "hub.css", added = 142, removed = 58 } }
	s.commands = 12
	s.agents = 1
	s.reply = "The tiles are half the size now."
	return s
end

local function look(now: number, list, settings: any?)
	return handle({ event = "claude", now = now, claude = list, settings = settings or {} })
end

local function press(action: string, text: string?)
	return handle({ event = "action", now = T0, action = action, args = { text = text or "" }, settings = {} })
end

local function kinds(section)
	local out = {}
	for _, b in section.body do
		table.insert(out, b.kind)
	end
	return table.concat(out, ",")
end

local function find(section, kind: string)
	for _, b in section.body do
		if b.kind == kind then
			return b
		end
	end
	return nil
end

tests = {}

function tests.a_working_session_says_what_it_does()
	local w = look(T0, { rich(session("a", "busy", T0 - 1000)), session("b", "idle", T0 - 5000000) }).widgets.sessions
	eq(w.peek.text, "Working", "peek")
	eq(w.peek.live, { icon = "own:claude.svg", tone = "working", image = "own:claude-code.gif" }, "live")
	local s = w.panel.body[1]
	eq(s.title, "Fix the thing", "title")
	eq(s.sub, "Editing lib.rs", "what it does")
	eq(s.live.tone, "working", "tone")
	eq(s.badge.tip, "VS Code\nnadi", "badge")
	eq(s.press, "focus:a", "press")
	eq(kinds(s), "chips,strip,checklist,lines,stats,meter", "blocks")
	eq(s.body[1].items[2], { icon = "shield", text = "Plan", tone = "hot" }, "plan is lit")
	eq(s.body[1].items[3].text, "Sonnet 5.5", "model")
	eq(s.body[1].items[4].text, "high", "effort")
	eq(s.body[2].text, "Working for 2m 14s", "turn")
	eq(s.body[2].side, "Thought 12s", "thought")
	eq(#s.body[4].rows, 3, "now and two before")
	eq(s.body[5].items[1], { value = "2", label = "Files" }, "files")
	eq(s.body[6].text, "90k / 1.0M", "context")
	eq(w.panel.body[2].live.tone, "idle", "the idle one too")
	eq(find(w.panel.body[2], "strip").text, "Quiet", "quiet")
end

function tests.a_session_that_waits_shows_what_it_asks_and_how_to_answer()
	local s = rich(session("w", "busy", T0 - 41000, { waiting_for = "permission", pending = { tool = "Bash", icon = "term", detail = "cargo build --release" } }))
	local ask = find(look(T0, { s }).widgets.sessions.panel.body[1], "ask")
	eq(ask.label, "Wants to run", "label")
	eq(ask.text, "cargo build --release", "text")
	eq(ask.side, "waiting 41s", "side")
	eq(#ask.buttons, 0, "nothing to press")
	s.asking = { id = "r1", kind = "permission", tool = "Bash", icon = "term", detail = "cargo build --release", age_ms = 41000 }
	ask = find(look(T0, { s }).widgets.sessions.panel.body[1], "ask")
	eq(ask.buttons[1], { label = "Allow", action = "allow:r1", tone = "ok" }, "allow")
	eq(ask.action, "denywith:r1", "with words")
	eq(press("allow:r1").answer, { id = "r1", behavior = "allow", text = "" }, "allowed")
	eq(press("denywith:r1", "use the other file").answer, { id = "r1", behavior = "deny", text = "use the other file" }, "denied with words")
	eq(press("answer_on").claude_hooks, { action = "install", permission = true, stop = false, wait_s = 20 }, "set up")
	eq(press("answer_off").claude_hooks.action, "remove", "removed")
end

function tests.a_finished_session_says_what_it_did()
	local f = rich(session("f", "idle", T0 - 180000))
	f.turn_started = T0 - 180000 - 362000
	local s = look(T0, { f }).widgets.sessions.panel.body[1]
	eq(s.body[2].text, "Done in 6m 02s", "took")
	eq(s.body[2].side, "3 min ago", "ago")
	eq(s.body[3], { kind = "text", label = "Result", text = "The tiles are half the size now." }, "result")
	eq(s.body[4].rows[2], { name = "hub.css", added = 142, removed = 58 }, "files")
	check(find(s, "field") == nil, "no field")
	f.asking = { id = "s1", kind = "stop" }
	eq(find(look(T0, { f }).widgets.sessions.panel.body[1], "field").action, "reply:s1", "reply field")
	eq(press("reply:s1", "now the rest").answer, { id = "s1", behavior = "reply", text = "now the rest" }, "replied")
end

function tests.the_ones_that_need_you_lead()
	local waiting = session("w", "busy", T0 - 9000, { waiting_for = "permission", title = "Needs you" })
	local finished = session("f", "idle", T0 - 60000)
	local working = session("k", "busy", T0 - 1000, { title = "Busy" })
	local w = look(T0, { working, finished, waiting }).widgets.sessions
	eq(w.panel.body[1].title, "Needs you", "first")
	eq(w.panel.body[1].sub, "Waiting for your approval", "sub")
	eq(w.panel.body[2].live.tone, "finished", "then finished")
	eq(w.panel.body[3].live.tone, "working", "then working")
	eq(w.peek.text, "Needs you", "peek")
	eq(w.glow and w.glow.style, "breathe", "it glows")
end

function tests.a_session_stays_while_it_runs_and_goes_with_its_source()
	local s = session("f", "idle", T0 - 60000)
	eq(look(T0, { s }).widgets.sessions.panel.body[1].live.tone, "finished", "finished")
	local r = press("focus:f")
	eq(r.focus.title, "nadi", "focus")
	eq(look(T0 + 16 * 60000, { s }).widgets.sessions.panel.body[1].live.tone, "idle", "idle later")
	eq((look(T0 + 20 * 60000, {}).widgets.sessions).idle, true, "gone (idle)")
end

function tests.banners_say_when_a_session_starts_and_finishes()
	look(T0, { session("a", "idle", T0 - 100000) })
	-- (a new turn: started working after 3 s)
	local r = look(T0 + 1000, { session("a", "busy", T0 + 1000, { turn_started = T0 + 1000 }) })
	check(r.say == nil, "not yet")
	r = look(T0 + 5000, { session("a", "busy", T0 + 5000, { turn_started = T0 + 1000 }) })
	eq(r.say[1].pill.sub, "Working in VS Code", "started")
	eq(r.say[1].pill.tone, "working", "tone")
	r = look(T0 + 60000, { session("a", "idle", T0 + 60000, { turn_started = T0 + 1000, files = { { name = "x" } } }) })
	eq(r.say[1].pill.sub, "Done in 59s, 1 file changed", "done")
	eq(r.say[1].pill.tile.color, "#5ac88c", "green")
end

function tests.needing_you_has_buttons_and_is_taken_back_when_answered()
	look(T0, { session("a", "idle", T0 - 100000) })
	local ask = { id = "r1", kind = "permission", tool = "Bash", detail = "rm -rf build", age_ms = 0 }
	local r = look(T0 + 1000, { session("a", "busy", T0 + 1000, { waiting_for = "permission", asking = ask }) })
	eq(r.say[1].priority, 90, "urgent")
	eq(r.say[1].pill.sub, "Wants to run rm -rf build", "sub")
	eq(r.say[1].pill.buttons[1].action, "allow:r1", "allow")
	r = look(T0 + 2000, { session("a", "busy", T0 + 2000) })
	eq(r.cancel, { "a" }, "taken back")
end

function tests.a_session_in_front_is_not_announced()
	handle({ event = "processes", now = T0, settings = {}, windows = { { exe = "code.exe", title = "lib.rs - nadi - Visual Studio Code", front = true } } })
	look(T0, { session("a", "idle", T0 - 100000) })
	local r = look(T0 + 60000, { session("a", "busy", T0 + 1000, { turn_started = T0 + 1000 }) })
	check(r.say == nil, "in front")
end

function tests.several_at_once_are_one_banner()
	look(T0, { session("a", "idle", T0 - 100000), session("b", "idle", T0 - 100000) })
	local r = look(T0 + 60000, {
		session("a", "busy", T0 + 1000, { turn_started = T0 + 1000 }),
		session("b", "busy", T0 + 1000, { turn_started = T0 + 1000 }),
	})
	eq(#r.say, 1, "one")
	eq(r.say[1].text, "2 sessions", "merged")
	eq(r.say[1].pill.sub, "2 started working", "sub")
end

function tests.model_names()
	local w = look(T0, { session("a", "busy", T0, { model = "claude-haiku-4-5-20251001", context_tokens = 180000 }) }).widgets.sessions
	local s = w.panel.body[1]
	eq(s.body[1].items[1].text, "Haiku 4.5", "haiku")
	eq(find(s, "meter").color, "#e87a7a", "nearly full")
end

-- usage

local function start(store)
	return handle({ event = "start", now = T0, settings = {}, store = store })
end

local function fetched(id: string, status: number, body, now: number?)
	return handle({ event = "fetched", now = now or T0, settings = {}, id = id, status = status, body = nadi.json.encode(body), error = "" })
end

function tests.signed_out_it_says_how_to_sign_in()
	local r = start(nil)
	eq(r.note, "Not signed in", "note")
	eq(r.widgets.sessions.peek.live.rings, nil, "no rings")
	check(r.fetch == nil, "no request")
end

function tests.signed_in_it_asks_and_draws_what_comes_back()
	local r = start({ login = { access = "ACC", refresh = "REF", expires_ms = T0 + 3600000 } })
	eq(r.fetch[1].id, "usage", "asks")
	eq(r.fetch[1].headers.Authorization, "Bearer ACC", "bearer")
	r = fetched("usage", 200, { five_hour = { utilization = 42, resets_at = "2027-01-15T12:00:00Z" }, seven_day = { utilization = 18 } })
	local w = r.widgets.sessions
	eq(#w.peek.live.rings, 2, "two rings")
	eq(w.peek.live.rings[2].part, 0.18, "7d")
	eq(w.tip.title, "No sessions", "tip")
	eq(w.tip.body[1].kind, "bars", "its tip: the limits as bars")
	eq(w.panel.body[1].kind, "bars", "the limits on top")
	eq(r.widgets.usage, {}, "no widget of its own")
	eq(r.note, false, "no note")
	check(r.say == nil, "the first numbers are not news")
	r = fetched("usage", 200, {}, T0)
	check(r.say == nil, "not waited for")
end

function tests.a_number_that_moved_says_so()
	start({ login = { access = "ACC", refresh = "REF", expires_ms = 0 } })
	fetched("usage", 200, { five_hour = { utilization = 42 }, seven_day = { utilization = 18 } })
	handle({ event = "tick", now = T0 + 200000, settings = {} })
	local r = fetched("usage", 200, { five_hour = { utilization = 47 }, seven_day = { utilization = 18 } })
	eq(r.say[1].pill.rows[1], { label = "5h", from = 0.42, value = 0.47, text = "47%" }, "moved")
end

function tests.a_token_about_to_run_out_is_renewed()
	local r = start({ login = { access = "ACC", refresh = "REF", expires_ms = T0 + 1000 } })
	eq(r.fetch[1].id, "renew", "renew")
	eq(r.fetch[1].headers["User-Agent"], "nadi/1.1", "agent")
	local body = nadi.json.decode(r.fetch[1].body)
	eq(body.refresh_token, "REF", "refresh")
	r = fetched("renew", 200, { access_token = "NEW", expires_in = 3600 })
	eq(r.store.login.access, "NEW", "kept")
	eq(r.store.login.refresh, "REF", "the old refresh stays")
	eq(r.fetch[1].headers.Authorization, "Bearer NEW", "asks again")
end

function tests.sign_in_opens_the_page_and_the_code_comes_back()
	start(nil)
	local r = press("usage_login")
	check(string.find(r.open, "https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a", 1, true) == 1, "page")
	check(string.find(r.open, "redirect_uri=https%3A%2F%2Fconsole.anthropic.com%2Foauth%2Fcode%2Fcallback", 1, true) ~= nil, "redirect")
	check(string.find(r.open, "code_challenge_method=S256", 1, true) ~= nil, "pkce")
	r = press("usage_finish", "  abc123#xyz \r\n")
	eq(r.fetch[1].id, "exchange", "exchange")
	eq(nadi.json.decode(r.fetch[1].body).code, "abc123", "code only")
	r = fetched("exchange", 200, { access_token = "A", refresh_token = "R", expires_in = 3600 })
	eq(r.store.login.access, "A", "kept")
	eq(r.fetch[1].id, "usage", "and asks")
end
