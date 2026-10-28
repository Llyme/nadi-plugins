--!strict
-- Notification Mirror: other apps' Windows notifications (Discord, Viber, ...) as banners. Pressing a banner opens the app it came
-- from. It reads the `notifications` sensor. Viber draws a popup of its own instead of a toast, with no text to read, so for Viber
-- it only notices that the popup came (a small tool window titled "ViberPC") and says there is a new message.
--
-- Its widget, a bell, is where the banners come out (drag it to move them); its panel lists the last ones. A dot shows on the bell
-- until its panel is opened.

local popups = {} -- the Viber popups at the last look
local primed = false
local recent = {} -- the last notifications, newest last: { title, body, app, at }
local unseen = false

-- what pressing a banner opens, and the name a pile of them goes under
local function link_for(app: string): (string, string)
	local a = string.lower(app or "")
	if string.find(a, "discord", 1, true) then
		return "discord://", "Discord"
	elseif string.find(a, "viber", 1, true) then
		return "viber://chats", ""
	end
	return "", ""
end

-- the title (the app's name if there is none), and the body (the app's name if only a title came)
local function words(t): (string, string)
	local title = if (t.title or "") == "" then t.app or "" else t.title
	local body
	if (t.title or "") == "" then
		body = t.body or ""
	elseif (t.body or "") == "" then
		body = t.app or ""
	else
		body = t.body
	end
	return title, body
end

local function ago(ms: number): string
	local m = ms // 60000
	if m < 1 then
		return "Now"
	elseif m < 60 then
		return m .. "m"
	end
	return (m // 60) .. "h"
end

local function remember(title: string, body: string, app: string, now: number)
	table.insert(recent, { title = title, body = body, app = app, at = now })
	while #recent > 8 do
		table.remove(recent, 1)
	end
	unseen = true
end

-- the bell, and its panel of the last ones
local function widget(now: number)
	local rows = {}
	for i = #recent, 1, -1 do
		local n = recent[i]
		table.insert(rows, { title = n.title, sub = n.body, right = ago(now - n.at) })
	end
	-- (nothing to show: no panel, so it does not open)
	local body = if #rows > 0
		then { { kind = "list", rows = rows }, { kind = "buttons", items = { { label = "Clear", action = "clear" } } } }
		else nil
	return {
		peek = { icon = "bell" },
		glow = if unseen then { color = "#e8b04a" } else nil,
		panel = if body then { body = body } else nil,
		tip = "Notifications",
	}
end

function handle(ev)
	if ev.event == "start" then
		popups, primed, recent, unseen = {}, false, {}, false
		return { widgets = { inbox = widget(ev.now) } }
	elseif ev.event == "action" then
		if ev.action == "clear" then
			recent, unseen = {}, false
			return { widgets = { inbox = widget(ev.now) } }
		end
		return nil
	elseif ev.event == "open" then
		if ev.widget == "inbox" and ev.open == true then
			unseen = false
		end
		return { widgets = { inbox = widget(ev.now) } }
	elseif ev.event == "notifications" then
		local says = {}
		for _, t in ev.notifications or {} do
			local title, body = words(t)
			local open, group = link_for(t.app)
			if body ~= "" then
				remember(title, body, t.app or "", ev.now)
				table.insert(says, { text = body, title = title, priority = 0, open = open, group = if group ~= "" then group else nil })
			end
		end
		return if #says > 0 then { say = says, widgets = { inbox = widget(ev.now) } } else nil
	elseif ev.event == "processes" then
		local now, fresh = {}, false
		if ev.settings.viber ~= false then
			for _, w in ev.windows or {} do
				if w.exe == "viber.exe" and w.title == "ViberPC" and w.w > 50 and w.w < 600 then
					now[w.id] = true
					if primed and not popups[w.id] then
						fresh = true
					end
				end
			end
		end
		popups, primed = now, true
		if fresh then
			remember("Viber", "New message", "Viber", ev.now)
			return { say = { { text = "New message", title = "Viber", priority = 0, open = "viber://chats", group = "Viber" } }, widgets = { inbox = widget(ev.now) } }
		end
	end
	return nil
end
