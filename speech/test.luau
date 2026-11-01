tests = {}

local S = { model = "base", mic = true, system = true }

local function script(out)
	for _, b in out.widgets.speech.panel.body do
		if b.kind == "script" then
			return b.items
		end
	end
	return {}
end

function tests.a_call_to_listen_reaches_its_program_and_a_line_goes_out_to_who_hears()
	handle({ event = "start", now = 0, settings = S })
	local r = handle({ event = "call", now = 1, settings = S, from = "captions", what = "listen", args = { system = false } })
	eq(r.helper, { cmd = "listen", mic = true, system = false, model = "base", keep_days = 7 }, "listen, as asked")
	local o = handle({ event = "helper", now = 2, settings = S, data = { kind = "line", text = "hello there", speaker = "s1", name = "🦊" } })
	eq(o.emit.name, "speech.line", "the line goes out")
	eq(o.emit.data.text, "hello there", "its words")
	eq(o.emit.data.avatar, "🦊", "with their animal")
	check(o.widgets.speech ~= nil, "the transcript is drawn")
end

function tests.a_person_is_named_from_their_animal()
	handle({ event = "start", now = 0, settings = S })
	handle({ event = "helper", now = 1, settings = S, data = { kind = "voices", list = { { id = "s2", name = "", animal = "🐼" } } } })
	local o = handle({ event = "helper", now = 1, settings = S, data = { kind = "line", text = "hi", speaker = "s2", name = "🐼" } })
	eq(script(o)[2].avatar_action, "person:s2", "their animal opens their card")
	handle({ event = "action", now = 2, settings = S, action = "person:s2" })
	local r = handle({ event = "action", now = 3, settings = S, action = "rename", args = { text = "Kai" } })
	eq(r.helper, { cmd = "name", speaker = "s2", name = "Kai" }, "named")
	local m = handle({ event = "action", now = 4, settings = S, action = "merge:s3" })
	eq(m.helper, { cmd = "merge", into = "s2", from = "s3" }, "two made one")
end

function tests.a_line_plays_its_sound_and_is_revised()
	handle({ event = "start", now = 0, settings = S })
	handle({ event = "helper", now = 1, settings = S, data = { kind = "line", uid = "s#1", text = "helo", speaker = "s1", name = "A", session = "s", source = "system", start = 1.5, ["end"] = 3 } })
	local p = handle({ event = "action", now = 2, settings = S, action = "play:s#1" })
	eq(p.helper, { cmd = "play", session = "s", source = "system", start = 1.5, ["end"] = 3 }, "its sound played")
	check(script(p)[2].playing ~= nil, "shown playing")
	local r = handle({ event = "helper", now = 3, settings = S, data = { kind = "revised", session = "s", lines = { { uid = "s#1", text = "Hello", speaker = "s2", name = "Kai" } } } })
	eq(r.emit.name, "speech.revised", "the revision goes out")
	local l = script(r)[2]
	eq(l.text, "Hello", "its better words")
	eq(l.action, "play:s#1", "still playable")
	eq(l.fresh, 3, "marked as just made better")
end

function tests.listening_shows_and_is_told()
	handle({ event = "start", now = 0, settings = S })
	local o = handle({ event = "helper", now = 1, settings = S, data = { kind = "status", state = "listening", session = "s" } })
	eq(o.emit.name, "speech.status", "status out")
	eq(o.widgets.speech.peek.voice.state, "listening", "its voice listening")
	eq(o.emit.data.since, 1, "since when")
end

function tests.a_long_session_stays_within_what_nadi_takes()
	handle({ event = "start", now = 0, settings = S })
	local o
	for i = 1, 400 do
		o = handle({ event = "helper", now = i, settings = S, data = { kind = "line", uid = "s#" .. i, text = string.rep("word ", 30), speaker = "s1", name = "🦊", session = "s", start = i, ["end"] = i + 1, at = 1793435253345 } })
	end
	local items = 0
	for _, b in o.widgets.speech.panel.body do
		if b.kind == "script" then
			items = #b.items
		end
	end
	check(items > 20 and items < 170, "only the newest lines: " .. items)
end
