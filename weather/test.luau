tests = {}

function tests.it_asks_and_draws_the_temperature()
	local s = { latitude = 25.03, longitude = 121.56 }
	local r = handle({ event = "start", now = 0, settings = s })
	check(string.find(r.fetch[1].url, "latitude=25.0300&longitude=121.5600", 1, true) ~= nil, "the place")
	check(handle({ event = "tick", now = 60000, settings = s }).fetch == nil, "not again so soon")
	r = handle({ event = "fetched", now = 0, settings = s, id = "now", status = 200, body = '{"current":{"temperature_2m":27.6,"apparent_temperature":31.2,"relative_humidity_2m":80,"wind_speed_10m":9,"precipitation":0}}' })
	eq(r.widgets.weather.peek.text, "28°", "temperature")
	eq(r.widgets.weather.panel.body[1].rows[1][2], "80%", "humidity")
	check(handle({ event = "tick", now = 60000, settings = { latitude = 1, longitude = 2 } }).fetch ~= nil, "a new place: at once")
end
