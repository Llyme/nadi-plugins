--!strict
-- Game Clock: which game is open, and for how long. It says so, once, after the time you set (a banner of priority 40). The
-- programs that count as games are a setting. It only sees which programs have a window.

local exe, since, said = "", 0, 0

local function nice(e: string): string
	local n = string.gsub(e, "%.exe$", "")
	return string.upper(string.sub(n, 1, 1)) .. string.sub(n, 2)
end

local function span(ms: number): string
	local m = ms // 60000
	return if m >= 60 then (m // 60) .. "h " .. (m % 60) .. "m" else m .. "m"
end

function handle(ev)
	if ev.event ~= "processes" then
		return nil
	end
	local games = {}
	for g in string.gmatch(string.lower(ev.settings.games or ""), "[^,]+") do
		local t = string.match(g, "^%s*(.-)%s*$")
		if t and t ~= "" then
			games[t] = true
		end
	end
	local remind = math.max(1, ev.settings.remind_min or 120) * 60000
	local w = nil
	for _, x in ev.windows or {} do
		if games[x.exe] then
			w = x
			break
		end
	end
	if not w then
		exe, since, said = "", 0, 0
		return { widgets = { clock = {} } }
	end
	if exe ~= w.exe then
		exe, since, said = w.exe, ev.now, 0
	end
	local played = math.max(0, ev.now - since)
	local out: any = {
		widgets = {
			clock = {
				peek = { icon = "gamepad", text = span(played), label = nice(w.exe) .. ", " .. span(played) },
				panel = {
					title = nice(w.exe),
					sub = "Playing for " .. span(played),
					tile = { icon = "gamepad" },
					body = { { kind = "facts", rows = { { "Session", span(played) }, { "Reminder", "after " .. span(remind) } } } },
				},
				tip = "Playing for " .. span(played),
			},
		},
	}
	local due = played // remind
	if due > said then
		said = due
		out.say = { { text = "You have been playing " .. nice(w.exe) .. " for " .. span(played), priority = 40 } }
	end
	return out
end
