-- The tests of Picture: `nadi-plugin test picture`.

local function act(a: string, args: any?)
	return handle({ event = "action", now = 0, settings = {}, action = a, args = args or {} })
end

local function find(body, kind)
	for _, b in body do
		if b.kind == kind then
			return b
		end
	end
	return nil
end

tests = {}

function tests.empty_it_asks_for_a_picture()
	local w = handle({ event = "start", now = 0, settings = {}, store = nil }).widgets.picture
	eq(w.peek.icon, "image", "a picture icon")
	eq(find(w.panel.body, "drop").args.paste, true, "pressed: pasted")
end

function tests.pasted_shown_and_kept()
	handle({ event = "start", now = 0, settings = {}, store = nil })
	eq(act("add", {}), nil, "nothing on the clipboard: nothing")
	act("add", { file = "paste-1.png", w = 800, h = 600 })
	local out = act("add", { file = "paste-2.png", w = 800, h = 600 })
	local w = out.widgets.picture
	eq(w.peek.image, "own:paste-2.png", "the newest shown")
	eq(find(w.panel.body, "view").image, "own:paste-2.png", "looked at")
	eq(#find(w.panel.body, "view").album.items, 2, "both in the row")
	eq(w.wheel, "next", "the wheel steps through them")
	eq(#out.store.pics, 2, "kept")
	eq(act("next", { up = false }).widgets.picture.peek.image, "own:paste-1.png", "the wheel: the next")
end

function tests.removed_and_see_through()
	handle({ event = "start", now = 0, settings = {}, store = { pics = { { file = "paste-1.png", w = 1, h = 1 }, { file = "paste-2.png", w = 1, h = 1 } }, shown = 2 } })
	eq(act("see").widgets.picture.see_through, true, "see-through")
	local out = act("remove:paste-2.png")
	eq(out.forget, "paste-2.png", "its file let go of")
	eq(out.widgets.picture.peek.image, "own:paste-1.png", "the other shown")
end
