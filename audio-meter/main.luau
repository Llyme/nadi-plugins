--!strict
-- Audio Meter: what is playing, as bands. It hears levels and bands, never the sound itself, and cannot reach the network. The
-- bars rise at once and fall slowly, so the meter reads as a meter.

local bars = table.create(16, 0)

function handle(ev)
	if ev.event ~= "audio" or not ev.audio then
		return nil
	end
	local a = ev.audio
	for i = 1, 16 do
		local now = math.clamp((a.spectrum or {})[i] or 0, 0, 1)
		bars[i] = if now > bars[i] then now else bars[i] * 0.82 + now * 0.18
	end
	local kind = if a.kind == "music" then "Music" elseif a.kind == "speech" then "Voices" else "Silence"
	local sub = kind
	if ev.settings.show_bpm ~= false and (a.bpm or 0) > 40 and a.kind == "music" then
		sub = kind .. ", " .. math.floor(a.bpm + 0.5) .. " bpm"
	end
	local level = math.floor((a.level or 0) * 100 + 0.5) .. "%"
	local shown = table.clone(bars)
	return {
		widgets = {
			meter = {
				peek = { viz = true, text = level, label = sub },
				panel = { body = { { kind = "spectrum", values = shown } } },
				tip = sub,
			},
		},
	}
end
