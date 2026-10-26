# NADI plugins

The official plugins for [NADI](https://github.com/Llyme/not-another-dynamic-island): Luau scripts, and two faces.

To install, open Settings > Plugins in NADI: this repository is listed there; tick a plugin to install it.

| Plugin | Kind | What it does |
|---|---|---|
| `battery` | Luau | How full the battery is, as a liquid in a vessel; banners when it runs low, is plugged in, is full. |
| `claude` | Luau | Your Claude Code sessions (what each is doing, its context, its cost), a banner when one changes state (Allow and Deny when it asks), and your Claude limits. |
| `downloads` | Luau | What your browsers, Steam and qBittorrent are downloading. |
| `eyes` | face | Two eyes that follow the cursor, blink, have moods, and dance to the music. |
| `games` | Luau | The game you are playing, for how long, and how it runs. |
| `ics-calendar` | Luau | The date and time, a month view, banners before the events of a calendar link, and, if you like, one that says the time now and then. |
| `now-playing` | Luau | What is playing, with play, skip, seek and speed, and three bars that dance with the sound. |
| `page-reader` | Luau | A section for each page you have open, with what it says (needs the NADI browser extension). |
| `sound-light` | face | An aura of soft lights that dances to what plays. |
| `system-meters` | Luau | Bars for the CPU, the memory and the graphics card, a ring for each open, and a banner when the processor stays busy. |
| `notification-mirror` | Luau | Other apps' notifications (Discord, Viber, ...) as banners. |
| `work` | Luau | What you work on: your editor, your project's git state, your hours. |
| `audio-meter`, `game-clock`, `weather` | Luau, examples | Small examples to start from. |

Every folder with a `manifest.json` is one plugin. The format is in [native/PLUGINS.md](https://github.com/Llyme/not-another-dynamic-island/blob/main/native/PLUGINS.md).

```
powershell ./build.ps1     # dist/<id>/ for each plugin, and nadi-plugins.tar.gz (tests run when nadi-plugin is on the PATH)
nadi-plugin test battery   # a plugin's test.luau
```

Tag a version (`v1.0.0`) and the workflow attaches `nadi-plugins.tar.gz` to a release. NADI installs from it.

A plugin of your own: copy `game-clock`, change `handle`, and add a folder with its `manifest.json`.
