--!strict
-- Notification Mirror: other apps' Windows notifications (Discord, Viber, ...) as banners. Pressing a banner opens the app it came
-- from. It reads the `notifications` sensor. Viber draws a popup of its own instead of a toast, with no text to read, so for Viber
-- it only notices that the popup came (a small tool window titled "ViberPC") and says there is a new message.

local popups = {} -- the Viber popups at the last look
local primed = false

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

function handle(ev)
	if ev.event == "start" then
		popups, primed = {}, false
	elseif ev.event == "notifications" then
		local says = {}
		for _, t in ev.notifications or {} do
			local title, body = words(t)
			local open, group = link_for(t.app)
			if body ~= "" then
				table.insert(says, { text = body, title = title, priority = 0, open = open, group = if group ~= "" then group else nil })
			end
		end
		return if #says > 0 then { say = says } else nil
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
			return { say = { { text = "New message", title = "Viber", priority = 0, open = "viber://chats", group = "Viber" } } }
		end
	end
	return nil
end
