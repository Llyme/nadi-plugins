# NADI plugins

The official plugins for [NADI](https://github.com/Llyme/not-another-dynamic-island), and the Rust SDK to write your own.

To install, open Settings > Plugins in NADI, press `+`, and paste `Llyme/nadi-plugins`.

| Plugin | Kind | What it does |
|---|---|---|
| `ics-calendar` | module | The events of a calendar link: date and time in the head, a month view, the next events, a banner before one starts. |
| `time` | module | Says what time it is now and then, in a small pill. |
| `audio-meter` | module, example | Hears the sound and draws a card that moves with it. |
| `busy-meter` | module, example | A ring in the hub, a pill with buttons, a window of its own. |
| `game-clock` | module, example | Says how long a game has been on. |
| `notification-mirror` | module | The notifications of other apps (Discord, Viber, ...) as banners on the island. |
| `work` | module | What you work on: your editor, your hours and its git state, with a pill for the session you are in. |
| `now-playing` | module | A pill and a card for what is playing, with play, skip, seek and speed. |
| `eyes` | face | The two eyes of the island: they follow the cursor, blink, and move to the beat. |
| `sound-light` | face | An aura behind the island that dances to what plays, and spills a little outside it. |
| `system-meters` | module | Three thin bars for the CPU, the memory and the graphics card, with a ring for each in the tray. |
| `weather` | declarative, example | A card and a pill from a web API. |

Every folder with a `manifest.json` is one plugin. The format is in [PLUGINS.md](https://github.com/Llyme/not-another-dynamic-island/blob/main/PLUGINS.md).

```
powershell ./build.ps1     # dist/<id>/ for each plugin, and nadi-plugins.tar.gz
cargo test                 # a module is a function from an event to a reply, so its logic is an ordinary Rust test
```

Tag a version (`v1.0.0`) and the workflow attaches `nadi-plugins.tar.gz` to a release. NADI installs from it.

A plugin of your own: copy `busy-meter`, change `handle`, and add a folder with its `manifest.json`.
