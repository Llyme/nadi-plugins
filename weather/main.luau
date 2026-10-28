--!strict
-- Weather: the weather where you are, from open-meteo.com, every 15 minutes. It reaches that one host and sees nothing of this PC.

local EVERY_MS = 15 * 60000
local last: number? = nil
local place = ""

local function round(x: number): number
	return math.floor(x + 0.5)
end

local function ask(ev, out)
	local s = ev.settings
	last = ev.now
	place = tostring(s.latitude) .. "," .. tostring(s.longitude)
	out.fetch = {
		{
			id = "now",
			method = "GET",
			url = string.format(
				"https://api.open-meteo.com/v1/forecast?latitude=%.4f&longitude=%.4f&current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,precipitation",
				s.latitude or 0,
				s.longitude or 0
			),
		},
	}
end

function handle(ev)
	local out: any = {}
	if ev.event == "start" then
		ask(ev, out)
	elseif ev.event == "tick" then
		local here = tostring(ev.settings.latitude) .. "," .. tostring(ev.settings.longitude)
		if last == nil or ev.now - last >= EVERY_MS or here ~= place then
			ask(ev, out)
		end
	elseif ev.event == "fetched" then
		local data = nadi.json.decode(ev.body or "")
		local c = if type(data) == "table" then data.current else nil
		if ev.status ~= 200 or type(c) ~= "table" then
			out.note = "The weather could not be read: " .. (if (ev.error or "") ~= "" then ev.error else "HTTP " .. tostring(ev.status))
			return out
		end
		local t = round(c.temperature_2m or 0)
		out.note = false
		out.widgets = {
			weather = {
				peek = { icon = "sun", text = t .. "°", label = t .. "°C, feels like " .. round(c.apparent_temperature or 0) .. "°C" },
				panel = {
					title = t .. "°C",
					sub = "Feels like " .. round(c.apparent_temperature or 0) .. "°C",
					tile = { icon = "sun", color = "#e8b04a" },
					body = {
						{
							kind = "facts",
							rows = { { "Humidity", tostring(c.relative_humidity_2m) .. "%" }, { "Wind", tostring(c.wind_speed_10m) .. " km/h" }, { "Rain", tostring(c.precipitation) .. " mm" } },
						},
					},
				},
				tip = t .. "°C",
			},
		}
	else
		return nil
	end
	return out
end
