//! Write a NADI plugin as a WebAssembly module.
//!
//! The island calls the module with an [`Event`] (the audio, the programs that are open, a file that changed, the
//! calendar, what is playing, how busy the PC is, a button that was pressed, or a tick) and the module answers with a
//! [`Reply`]: what the island is to do. A card in the hub, a pill when something is going on, rings, banners, events for
//! the calendar. The module keeps what it needs between calls (a `thread_local!` is fine), has no network and no clock
//! but `event.now`, and is cut off when a call takes longer than its budget (4 ms unless the manifest asks for less). What
//! it may see is what its manifest lists.
//!
//! ```ignore
//! use nadi_plugin::*;
//! fn handle(ev: &Event) -> Reply {
//!     Reply::card(Card::new().icon("wave").title("Hello").sub(format!("{} windows", ev.windows.len())))
//! }
//! plugin!(handle);
//! ```

pub use serde_json::{json, Value};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------------------- what the island says

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Event {
    /// "start" (the first call, and again when the plugin is switched on), "audio", "processes", "file", "calendar",
    /// "media", "system", "usage", "downloads", "notifications", "command", "action" or "tick"
    pub event: String,
    /// milliseconds since 1970
    pub now: u64,
    /// how far this PC's clock is from UTC, in minutes (+540 in Tokyo): a module has no time zone of its own, see [`civil`]
    pub tz_offset: i32,
    /// the plugin's settings (see `settings` in the manifest)
    pub settings: Value,
    /// `event == "audio"`, with the `audio` permission
    pub audio: Option<Audio>,
    /// `event == "processes"`, with the `processes` permission: the programs that have a window, the one in front first
    pub windows: Vec<Window>,
    /// `event == "file"`, with a `files` permission: which file, and what it holds now (the last 256 KB of it)
    pub path: String,
    pub text: String,
    /// `event == "calendar"`, with the `calendar` permission: the events of the island's calendar that are not over yet
    /// (an hour of grace), first ones first, at most 100. Sent when the calendar changes.
    pub events: Vec<CalEvent>,
    /// `event == "media"`, with the `media` permission: what is playing, a session for each player
    pub sessions: Vec<Session>,
    /// `event == "notifications"`, with the `notifications` permission: what other apps notified since the last time
    pub notifications: Vec<Toast>,
    /// `event == "processes"`, with the `idle` permission: seconds since the keyboard or the mouse was last used
    pub idle_s: f64,
    /// `event == "start"`, with the `storage` permission: what the module kept (null if nothing)
    pub store: Value,
    /// `event == "command"`: the answer of a command it asked for ([`Reply::run`]): its id, how it ended (0 is well; -1 it
    /// did not start; -2 it took too long) and what it printed
    pub id: String,
    pub code: i64,
    pub stdout: String,
    /// `event == "fetched"`: the answer of a request it asked for ([`Reply::fetch`]), with its `id`: the HTTP status (0: the network
    /// did not answer, and `error` says why) and what came back (up to 256 KB)
    pub status: u16,
    pub body: String,
    pub error: String,
    /// 32 random bytes as hex, on every event of a module that may reach a host (`permissions.net`): it has no randomness of its
    /// own, and a sign-in needs some
    pub rand: String,
    /// `event == "browser"`, with the `browser` permission: the pages of the browsers (about every second, when something changed)
    pub browser: Option<BrowserFeed>,
    /// `event == "browser_done"`: a button that was pressed (`op == "press"`) or a tab that was sent on (`"go"`), with its `id`
    pub op: String,
    pub ok: bool,
    /// `event == "claude"`, with the `claude` permission (every 2 s): the Claude Code sessions on this PC. A session may come
    /// twice (it has more than one process): keep the one with the later `updated`.
    pub claude: Vec<ClaudeSession>,
    /// the expanded island is open (on every event)
    pub hub: bool,
    /// `event == "system"`, with the `system` permission (every 2 s)
    pub system: Option<System>,
    /// `event == "usage"`, with the `usage` permission, while it watches (see [`Reply::watch`]): what each of the programs it
    /// watches uses (every second)
    pub usage: Vec<Usage>,
    /// `event == "downloads"`, with the `downloads` permission (every 1.5 s): what is downloading now ...
    pub observed: Vec<Download>,
    /// ... and what stopped showing up since the last time
    pub ended: Vec<Ended>,
    /// `event == "action"`: the `action` of the button that was pressed (on a card, a pill or in the plugin's settings),
    /// and what the button sent with it. A button of the plugin's [`Modal`] is the action "modal", with `args` =
    /// `{ "button", "values", "closed" }`.
    pub action: String,
    pub args: Value,
    /// `event == "tick"`: the module's own `wake_in` came due (otherwise it is the manifest's `tick`)
    pub wake: bool,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Audio {
    /// 0 to 1, how loud (on a fixed scale: quiet reads quiet)
    pub level: f32,
    /// 16 bands from about 60 Hz to 12 kHz, each 0 to 1 (the lowest first), on the scale the island's own lights use:
    /// loud music fills it
    pub bands: Vec<f32>,
    /// the same 16 bands for a meter: true dBFS from -72 to -6 mapped to 0..1, so loud and quiet both show
    pub spectrum: Vec<f32>,
    /// a beat landed since the last call
    pub beat: bool,
    /// a bass drum hit
    pub kick: bool,
    /// how hard an onset just landed (0 = none)
    pub hit: f32,
    /// "silent", "speech" or "music"
    pub kind: String,
    pub voice: f32,
    pub bpm: f32,
    pub energy: f32,
    /// -1 (left) to 1 (right)
    pub pan: f32,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Window {
    /// the program's file name, lower case (`endfield.exe`)
    pub exe: String,
    pub pid: u32,
    /// empty without the `titles` permission
    pub title: String,
    /// the one that is in front
    pub front: bool,
    /// which window (a number that stays the same while it is open)
    pub id: i64,
    /// its size on the screen, px
    pub w: i32,
    pub h: i32,
    /// a popup that has no button in the task bar (with the `titles` permission, the list has them too)
    pub tool: bool,
    /// with the `programs` permission: where the program is, what it is called, and its icon (a `data:` address, told the
    /// first time only: keep it, by `path`)
    pub path: String,
    pub name: String,
    pub icon: Option<String>,
    /// with the `programs` permission: it covers its whole screen and has no title bar (what a game does; a maximized
    /// browser or editor keeps its title bar)
    pub full: bool,
}

/// What another app notified (a Windows toast): see [`Event::notifications`].
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Toast {
    /// which one, counting up
    pub seq: u64,
    /// the app that sent it ("Discord")
    pub app: String,
    pub title: String,
    pub body: String,
}

/// One player (a music app, a browser tab with a video): what it says it is playing.
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Session {
    /// which player (pass it back to [`Reply::media`] to press a button of this one)
    pub source: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// seconds
    pub position: f64,
    pub duration: f64,
    pub can_previous: bool,
    pub can_next: bool,
    /// the player takes a jump to a place (needs `media_control` to ask)
    pub can_seek: bool,
    /// the player takes a playback speed
    pub can_rate: bool,
    /// playback speed, 1.0 is normal
    pub rate: f64,
    /// which cover picture it is (0: none). With the `media_art` permission the picture itself, a `data:` address, comes in
    /// `art` the first time and again only when it changes: keep it
    pub art_id: u64,
    pub art: Option<String>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct System {
    /// 0 to 100
    pub cpu: f32,
    pub ram: f32,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    /// 0 to 100; `None` on a PC with no graphics counters, or until they have a baseline
    pub gpu: Option<f32>,
}

/// What the browsers' pages are (see [`Event::browser`]).
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct BrowserFeed {
    /// the browsers that have the extension connected ("msedge", "chrome")
    pub browsers: Vec<String>,
    /// for each, the titles of its tabs, lower case, as the extension lists them
    pub tabs: std::collections::HashMap<String, Vec<String>>,
    /// for each, the titles of the pages the extension has read, the ones in front first
    pub open: std::collections::HashMap<String, Vec<String>>,
    /// cards whose tab went to another page of its guide: (browser, title before, title now)
    pub moves: Vec<(String, String, String)>,
    /// the pages it watches that changed (at most two at a time), as the extension described them: `browser`, `title`, `url`,
    /// `version`, `vh`, `words`, `meta`, `ld`, `video`, `guide` (`key`, `text`, `heads`), `image` (an id) and `nodes`
    pub pages: Vec<Value>,
}

/// A Claude Code session (see [`Event::claude`]).
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct ClaudeSession {
    pub id: String,
    pub pid: u32,
    /// what the session says it is: "busy", "idle", or something with "wait" in it
    pub status: String,
    /// when it waits for you: what for ("permission", "input"...), else empty
    pub waiting_for: String,
    /// ms since 1970
    pub started: u64,
    pub updated: u64,
    pub name: String,
    /// the folder it runs in
    pub cwd: String,
    /// what the conversation is about, and the prompt it is answering, when they are known
    pub title: Option<String>,
    pub prompt: Option<String>,
    /// the model's id ("claude-sonnet-5-5")
    pub model: Option<String>,
    pub context_tokens: u64,
    pub cost_usd: f64,
    pub lines_added: u64,
    pub lines_removed: u64,
    /// the newest tool calls, newest first (at most three)
    pub feed: Vec<ClaudeItem>,
    /// what the model is doing at the end of the transcript
    pub last: Option<ClaudeItem>,
    /// the window it runs in: "VS Code", "Terminal"...
    pub host: ClaudeHost,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct ClaudeItem {
    /// one of the island's line icons
    pub icon: String,
    pub text: String,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct ClaudeHost {
    pub label: String,
    /// one of the island's line icons
    pub icon: String,
    /// the program's path, for [`Reply::focus`]
    pub exe: Option<String>,
}

/// What one program uses (see [`Event::usage`]).
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Usage {
    pub pid: u32,
    /// false once the program has ended
    pub alive: bool,
    /// 0 to 100, its share of the whole CPU
    pub cpu: f32,
    pub ram_gb: f64,
    /// 0 to 100, its share of the installed memory
    pub ram: f32,
    /// 0 to 100; `None` until the counters have a baseline, or on a PC without them
    pub gpu: Option<f32>,
    pub vram_gb: Option<f64>,
    /// frames per second, the slowest 1% of frames as frames per second, and the latest frame times (ms, oldest first); `None`
    /// when the program does not present through DXGI, or Windows refused the trace (`fps_denied`)
    pub fps: Option<f64>,
    pub low_fps: Option<f64>,
    pub frame_ms: Vec<f32>,
    pub fps_denied: bool,
}

/// One download that is going on (see [`Event::observed`]).
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Download {
    /// a name that stays the same for this download
    pub id: String,
    /// "Chrome", "Steam", "qBittorrent"...
    pub source: String,
    /// "browser", "steam" or "torrent"
    pub kind: String,
    pub name: String,
    pub received: u64,
    pub total: Option<u64>,
    /// bytes per second, when the source says (else work it out from `received`)
    pub speed: Option<f64>,
}

/// A download that stopped showing up (see [`Event::ended`]).
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Ended {
    pub id: String,
    /// it is done (false: it was cancelled or paused)
    pub finished: bool,
    /// where the file is, for a finished one (pass it to [`Reply::reveal`] to show it in its folder)
    pub path: Option<String>,
}

impl Event {
    pub fn setting_str(&self, key: &str) -> String {
        match self.settings.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        }
    }

    pub fn setting_f64(&self, key: &str, default: f64) -> f64 {
        match self.settings.get(key) {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    pub fn setting_bool(&self, key: &str, default: bool) -> bool {
        self.settings.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }

    /// The local date and time of `now` (a module has no time zone of its own).
    pub fn local(&self) -> Civil {
        civil(self.now as i64, self.tz_offset)
    }
}

// ------------------------------------------------------------------------------------------------------- the time

/// A date and a time of day, as a clock on the wall would say them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Civil {
    pub year: i32,
    /// 1 to 12
    pub month: u32,
    /// 1 to 31
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 is Monday, 6 is Sunday
    pub weekday: u32,
    /// days since 1970-01-01 (a number that changes at local midnight)
    pub days: i64,
}

pub const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The local date and time of a moment (`ms` since 1970, UTC), for a clock `tz_offset_min` minutes away from UTC.
pub fn civil(ms: i64, tz_offset_min: i32) -> Civil {
    let local = ms.div_euclid(1000) + tz_offset_min as i64 * 60;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    // (days to a date: the proleptic Gregorian calendar)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
    Civil {
        year,
        month,
        day,
        hour: (secs / 3600) as u32,
        minute: ((secs % 3600) / 60) as u32,
        second: (secs % 60) as u32,
        weekday: (days + 3).rem_euclid(7) as u32,
        days,
    }
}

/// A moment inside a text, for the island to write in the viewer's own way ("Today 3:00 PM", "Fri, Oct 9"). Put it in any
/// title, line or row; it is replaced when the card is drawn, and keeps right across midnight without another call.
pub fn when(ms: i64, all_day: bool) -> String {
    if all_day {
        format!("{{when:{ms}:day}}")
    } else {
        format!("{{when:{ms}}}")
    }
}

// ----------------------------------------------------------------------------------------- what it answers with

/// A button on a card or a pill. Pressing it sends the module an `action` event.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Button {
    pub label: String,
    /// one of the island's line icons; shown instead of the label (which stays as its tooltip)
    pub icon: String,
    pub action: String,
}

impl Button {
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Button { label: label.into(), icon: String::new(), action: action.into() }
    }
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = icon.into();
        self
    }
}

/// A card: what the island draws from a few blocks. Texts are cut at 200 characters, a list at 12 rows, bars at 32.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Card {
    /// which of the plugin's cards this is, when it has several (see [`Reply::more_cards`])
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub icon: String,
    /// where it sits among the other cards (a lower number comes first; 70 if not said)
    #[serde(skip_serializing_if = "is_zero")]
    pub rank: i32,
    pub title: String,
    pub sub: String,
    /// what the closed card says at its right
    pub peek: String,
    /// a picture for its tile (a `data:` address of a png, jpeg, webp or gif), or "keep" for the one it had
    #[serde(skip_serializing_if = "String::is_empty")]
    pub image: String,
    /// buttons in the header, at the right (a player's previous, play and next): up to 3
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<Button>,
    /// a letter in a colour, with a ring, in place of the icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile: Option<Tile>,
    /// a line icon before the second line (what it is doing)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sub_icon: String,
    /// how it stands: "waiting" (it needs you), "working", "finished" or "idle". The card is marked for it, and its icon is at the right.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tone: String,
    /// small icons at the right of the header
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<Badge>,
    /// an action the module is called with when the card's icon is pressed (go to the thing the card is about)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub focus: String,
    /// what the plugin's tile says at a glance: a big word or number and a few words under it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glance: Option<Glance>,
    pub body: Vec<Value>,
}

/// What a tile says at a glance ("1" and "needs you", "144" and "fps"): the big word is cut at 12 characters, the caption at 40.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Glance {
    pub big: String,
    pub caption: String,
}

/// A letter in a colour with a ring around it (how full something is): what a card, a pill or a brief shows in place of an icon.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Tile {
    /// one or two letters
    pub text: String,
    /// `#rrggbb`
    pub color: String,
    /// 0 to 1: how far the ring goes round (none: no ring)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ring: Option<f32>,
    /// `#rrggbb`
    pub ring_color: String,
}

impl Tile {
    pub fn new(text: impl Into<String>, color: impl Into<String>) -> Self {
        Tile { text: text.into(), color: color.into(), ring: None, ring_color: "#5ac88c".into() }
    }
    /// a ring around it, 0 to 1, in a colour
    pub fn ring(mut self, part: f32, color: impl Into<String>) -> Self {
        self.ring = Some(part);
        self.ring_color = color.into();
        self
    }
}

/// A small icon at the right of a header, with what it says when hovered.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Badge {
    pub icon: String,
    pub tip: String,
}

impl Badge {
    pub fn new(icon: impl Into<String>, tip: impl Into<String>) -> Self {
        Badge { icon: icon.into(), tip: tip.into() }
    }
}

/// A row of a pill that shows how far something is: a label, a bar (0 to 1) and its number. The bar slides when the number moves.
#[derive(Serialize, Default, Clone, Debug)]
pub struct PillRow {
    pub label: String,
    pub value: f32,
    pub text: String,
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

impl Card {
    pub fn new() -> Self {
        Card::default()
    }
    /// a name for this card among the plugin's own
    pub fn id(mut self, v: impl Into<String>) -> Self {
        self.id = v.into();
        self
    }
    /// one of the island's line icons: `wave`, `gamepad`, `sun`, `download`, `calendar`, `music`, `globe`, `clock`, `bell`...
    pub fn icon(mut self, v: impl Into<String>) -> Self {
        self.icon = v.into();
        self
    }
    pub fn rank(mut self, v: i32) -> Self {
        self.rank = v;
        self
    }
    pub fn title(mut self, v: impl Into<String>) -> Self {
        self.title = v.into();
        self
    }
    pub fn sub(mut self, v: impl Into<String>) -> Self {
        self.sub = v.into();
        self
    }
    pub fn peek(mut self, v: impl Into<String>) -> Self {
        self.peek = v.into();
        self
    }
    /// a picture for the card's tile: a `data:` address, or "keep" to say the one it had stays (so it need not be sent again)
    pub fn tile(mut self, v: impl Into<String>) -> Self {
        self.image = v.into();
        self
    }
    /// a thin bar you can press: `value` is how far, 0 to 1. A press is the action `action`, with `fraction` (0 to 1) in `args`
    pub fn seek(mut self, value: f32, action: &str) -> Self {
        self.body.push(json!({ "kind": "seek", "value": value, "action": action }));
        self
    }
    /// buttons in the card's header, at the right (up to 3): what the card's thing is controlled with
    pub fn controls(mut self, items: Vec<Button>) -> Self {
        self.controls = items;
        self
    }
    /// a seek bar with buttons at its sides (up to 2 before, 3 after): a player's jump back and forward, and its speed
    pub fn seek_with(mut self, value: f32, action: &str, before: Vec<Button>, after: Vec<Button>) -> Self {
        self.body.push(json!({ "kind": "seek", "value": value, "action": action, "before": before, "after": after }));
        self
    }
    /// a bar, 0 to 1
    pub fn progress(mut self, v: f32) -> Self {
        self.body.push(json!({ "kind": "progress", "value": v }));
        self
    }
    /// a row of meters, each 0 to 1
    pub fn bars(mut self, v: &[f32]) -> Self {
        self.body.push(json!({ "kind": "bars", "values": v }));
        self
    }
    pub fn text(mut self, v: impl Into<String>) -> Self {
        self.body.push(json!({ "kind": "text", "text": v.into() }));
        self
    }
    pub fn badge(mut self, v: impl Into<String>) -> Self {
        self.body.push(json!({ "kind": "badge", "text": v.into() }));
        self
    }
    /// label and value rows
    pub fn facts(mut self, rows: &[(&str, String)]) -> Self {
        self.body.push(json!({ "kind": "facts", "rows": rows }));
        self
    }
    /// a row each: title, a line under it, and a bar when there is a progress
    pub fn list(mut self, rows: &[(String, String, Option<f32>)]) -> Self {
        let rows: Vec<Value> = rows.iter().map(|(t, s, p)| json!({ "title": t, "sub": s, "progress": p })).collect();
        self.body.push(json!({ "kind": "list", "rows": rows }));
        self
    }
    /// buttons (at most 6); each sends the module an `action` event
    pub fn buttons(mut self, items: Vec<Button>) -> Self {
        self.body.push(json!({ "kind": "buttons", "items": items }));
        self
    }
    /// the card's second line opens with this icon, and its tone
    pub fn live(mut self, icon: impl Into<String>, tone: impl Into<String>) -> Self {
        self.sub_icon = icon.into();
        self.tone = tone.into();
        self
    }
    pub fn tile_of(mut self, tile: Tile) -> Self {
        self.tile = Some(tile);
        self
    }
    pub fn badges(mut self, items: Vec<Badge>) -> Self {
        self.badges = items;
        self
    }
    /// what the plugin's tile says at a glance (see [`Glance`]); without it the tile shows the card's title and second line
    pub fn glance(mut self, big: impl Into<String>, caption: impl Into<String>) -> Self {
        self.glance = Some(Glance { big: big.into(), caption: caption.into() });
        self
    }
    /// pressing the card's icon calls the module with this action
    pub fn focus_action(mut self, action: impl Into<String>) -> Self {
        self.focus = action.into();
        self
    }
    /// a page's main picture the browser extension sent, by its id (needs the `browser` permission)
    pub fn picture(mut self, id: u64) -> Self {
        self.body.push(json!({ "kind": "picture", "id": id }));
        self
    }
    /// a guide laid out to be read, by its key, with the page's main picture beside it (needs the `browser` permission). The reader
    /// asks the module for another section with the action `go` and `args.url`.
    pub fn reader(mut self, key: &str, image: Option<u64>) -> Self {
        self.body.push(json!({ "kind": "reader", "key": key, "image": image }));
        self
    }
    /// a paragraph that opens with a word in bold ("Prompt"); two lines at most
    pub fn labeled(mut self, label: &str, text: impl Into<String>) -> Self {
        self.body.push(json!({ "kind": "text", "label": label, "text": text.into() }));
        self
    }
    /// icon rows (at most 8): `(icon, text, now)`; the one that is `now` is lit
    pub fn lines(mut self, rows: &[(String, String, bool)]) -> Self {
        let rows: Vec<Value> = rows.iter().map(|(i, t, n)| json!({ "icon": i, "text": t, "now": n })).collect();
        self.body.push(json!({ "kind": "lines", "rows": rows }));
        self
    }
    /// a label, a bar (0 to 1) and what it says, in a colour (`#rrggbb`)
    pub fn meter(mut self, label: &str, value: f32, text: &str, color: &str) -> Self {
        self.body.push(json!({ "kind": "meter", "label": label, "value": value, "text": text, "color": color }));
        self
    }
    /// a few numbers, each with its name under it: `(value, name)`
    pub fn stats(mut self, items: &[(String, &str)]) -> Self {
        let items: Vec<Value> = items.iter().map(|(v, l)| json!({ "value": v, "label": l })).collect();
        self.body.push(json!({ "kind": "stats", "items": items }));
        self
    }
    /// a line through numbers, oldest first (at most 120): frame times, a speed. Drawn from 0 to `max` (the top of the
    /// graph, in the same unit), with a faint line at each of `marks` (at most 4)
    pub fn graph(mut self, values: &[f32], max: f32, marks: &[f32]) -> Self {
        self.body.push(json!({ "kind": "graph", "values": values, "max": max, "marks": marks }));
        self
    }
    /// a picture, as a `data:image/png;base64,...` address (png, jpeg, webp or gif, up to 200 KB)
    pub fn image(mut self, src: impl Into<String>) -> Self {
        self.body.push(json!({ "kind": "image", "src": src.into() }));
        self
    }
}

/// The pill a module offers while something is going on (what is playing, a game, a timer). The collapsed island shows
/// the pill with the highest `rank` of what the plugins offer, unless a banner or the hub has it. It is the module's to take
/// back ([`Reply::no_pill`]); it also goes when the plugin is switched off.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Pill {
    pub icon: String,
    /// a picture for its tile, or "keep" for the one it had
    #[serde(skip_serializing_if = "String::is_empty")]
    pub image: String,
    pub title: String,
    pub sub: String,
    /// a short text at its right (a time, a count)
    pub right: String,
    /// a thin bar under the text, 0 to 1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    /// a row of meters at the right, each 0 to 1
    pub bars: Vec<f32>,
    /// up to 3
    pub buttons: Vec<Button>,
    /// who wins when several plugins offer a pill, 0 to 99 (the games' is 20, the work pill's 10)
    pub rank: i32,
    /// the size at the standard width: 200 to 360 wide (260), 32 to 72 high (40, or 48 with a second line)
    #[serde(skip_serializing_if = "is_zero_f")]
    pub w: f32,
    #[serde(skip_serializing_if = "is_zero_f")]
    pub h: f32,
    /// "" or "big" (large light digits)
    pub look: String,
    /// it comes by itself and is looked at, like a banner: the island drops down for it, and its `rank` may be up to 100
    #[serde(skip_serializing_if = "is_false")]
    pub brief: bool,
    /// rows of a label, a bar and a number (at most 3), in place of the text
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<PillRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile: Option<Tile>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero_f(n: &f32) -> bool {
    *n == 0.0
}

impl Pill {
    pub fn new() -> Self {
        Pill::default()
    }
    pub fn icon(mut self, v: impl Into<String>) -> Self {
        self.icon = v.into();
        self
    }
    /// a picture for the tile: a `data:` address, or "keep" for the one it had
    pub fn image(mut self, v: impl Into<String>) -> Self {
        self.image = v.into();
        self
    }
    pub fn title(mut self, v: impl Into<String>) -> Self {
        self.title = v.into();
        self
    }
    pub fn sub(mut self, v: impl Into<String>) -> Self {
        self.sub = v.into();
        self
    }
    pub fn right(mut self, v: impl Into<String>) -> Self {
        self.right = v.into();
        self
    }
    pub fn progress(mut self, v: f32) -> Self {
        self.progress = Some(v);
        self
    }
    pub fn bars(mut self, v: &[f32]) -> Self {
        self.bars = v.to_vec();
        self
    }
    pub fn buttons(mut self, items: Vec<Button>) -> Self {
        self.buttons = items;
        self
    }
    pub fn rank(mut self, v: i32) -> Self {
        self.rank = v;
        self
    }
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.w = w;
        self.h = h;
        self
    }
    pub fn big(mut self) -> Self {
        self.look = "big".into();
        self
    }
    /// let it come by itself (see [`Pill::brief`])
    pub fn briefly(mut self) -> Self {
        self.brief = true;
        self
    }
    pub fn rows(mut self, rows: Vec<PillRow>) -> Self {
        self.rows = rows;
        self
    }
    pub fn tile_of(mut self, tile: Tile) -> Self {
        self.tile = Some(tile);
        self
    }
}

/// A ring at the top right of the hub, next to the CPU and memory ones. At most 3 for a plugin.
#[derive(Serialize, Clone, Debug)]
pub struct Ring {
    /// a name of its own (letters, digits, `-`, `_`)
    pub id: String,
    /// 0 to 100
    pub pct: f32,
    /// what it says when hovered ("Disk · 61%")
    pub tip: String,
    /// `#rrggbb`
    pub color: String,
    /// in the head as soon as it shows (false: it waits in the tray of edit mode)
    pub start: bool,
}

impl Ring {
    pub fn new(id: impl Into<String>, pct: f32, tip: impl Into<String>, color: impl Into<String>) -> Self {
        Ring { id: id.into(), pct, tip: tip.into(), color: color.into(), start: true }
    }
    /// keep it in the tray until the user places it
    pub fn in_tray(mut self) -> Self {
        self.start = false;
        self
    }
}

/// A banner, or a pill that comes by itself for a moment (like a session that needs you).
#[derive(Serialize, Clone, Debug, Default)]
pub struct Say {
    pub text: String,
    pub priority: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pill: Option<SayPill>,
    /// only when nothing fills the screen: not over the hub, not while a game is on
    pub ambient: bool,
    /// the banner's title (the plugin's name if empty)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// a link that pressing the banner opens: it has to start with one the manifest names in `permissions.open`
    #[serde(skip_serializing_if = "String::is_empty")]
    pub open: String,
    /// an app whose banners pile up: too many in a row become one that counts them
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct SayPill {
    pub icon: String,
    pub right: String,
    pub look: String,
    /// how long it stays, ms (the usual if 0; 1.5 to 12 s)
    pub ms: u64,
    /// a name of its own: one with the same key replaces the one that waits, and [`Reply::cancel`] takes it away
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile: Option<Tile>,
    pub badges: Vec<Badge>,
    /// "", "waiting" or "finished": the glow, and an icon at the right
    pub tone: String,
    /// what pressing it asks of the module (an `action` event, the user's: what it answers is shown whatever do not disturb says)
    pub press: String,
}

impl Say {
    pub fn new(text: impl Into<String>, priority: i32) -> Self {
        Say { text: text.into(), priority, ..Say::default() }
    }
    /// show it as a pill instead of a banner: the text is its title
    pub fn pill(mut self, icon: impl Into<String>, right: impl Into<String>) -> Self {
        self.pill = Some(SayPill { icon: icon.into(), right: right.into(), ..SayPill::default() });
        self
    }
    /// ... with large light digits (for a time)
    pub fn big(mut self) -> Self {
        if let Some(p) = self.pill.as_mut() {
            p.look = "big".into();
        }
        self
    }
    /// ... with a name of its own, a tile, small icons at the right, how it stands, and what pressing it does
    pub fn keyed(mut self, key: impl Into<String>, tone: impl Into<String>, press: impl Into<String>) -> Self {
        if let Some(p) = self.pill.as_mut() {
            p.key = key.into();
            p.tone = tone.into();
            p.press = press.into();
        }
        self
    }
    pub fn with_tile(mut self, tile: Tile, badges: Vec<Badge>) -> Self {
        if let Some(p) = self.pill.as_mut() {
            p.tile = Some(tile);
            p.badges = badges;
        }
        self
    }
    /// ... for this long, in ms
    pub fn for_ms(mut self, ms: u64) -> Self {
        if let Some(p) = self.pill.as_mut() {
            p.ms = ms;
        }
        self
    }
    /// only when nothing fills the screen
    pub fn ambient(mut self) -> Self {
        self.ambient = true;
        self
    }
    /// a title of its own for the banner
    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }
    /// pressing the banner opens this link (see `permissions.open`)
    pub fn opens(mut self, url: impl Into<String>) -> Self {
        self.open = url.into();
        self
    }
    /// banners of this app pile up: too many in a row become one that counts them
    pub fn grouped(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }
}

/// An event for the island's calendar (see [`Reply::events`]), or one the calendar holds (see [`Event::events`]). Times are
/// milliseconds since 1970.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct CalEvent {
    pub uid: String,
    pub summary: String,
    pub start_ms: i64,
    /// when it ends (the same as `start_ms` for an event with no length)
    pub end_ms: i64,
    pub all_day: bool,
}

impl CalEvent {
    pub fn new(uid: impl Into<String>, summary: impl Into<String>, start_ms: i64, end_ms: i64) -> Self {
        CalEvent { uid: uid.into(), summary: summary.into(), start_ms, end_ms, all_day: false }
    }
}

// ------------------------------------------------------------------------------------------------ widgets and panes

/// What a widget or a pane shows: a stack of blocks the island draws (a module sends no markup). A widget is listed in the
/// manifest (`"widgets": [{ "id", "name", "place", "rank", "width", "interactive" }]`) and answered with
/// [`Reply::widget`]; a pane is a view that replaces the cards, opened and closed by the module ([`Reply::pane`]). A press on
/// a widget, or on a block of a pane that names an id, comes back as an `action` event: `action == "widget"` with
/// `args == { "id" }`, or `action == "pane"` with `args == { "id" }`.
#[derive(Default, Clone, Debug)]
pub struct Blocks(Vec<Value>);

/// one day of a month grid
#[derive(Clone, Debug)]
pub struct Cell {
    /// what it says ("14")
    pub t: String,
    /// what a press sends back
    pub id: String,
    /// "", "today", "sel" or "other" (a day of the next or the last month)
    pub state: &'static str,
    /// 0 to 3 dots under it (things on that day)
    pub dots: u32,
}

/// one row of a list: a title, a line under it, and a time at its right
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub title: String,
    pub sub: String,
    pub time: String,
    pub past: bool,
}

impl Blocks {
    pub fn new() -> Self {
        Blocks::default()
    }
    /// a line of text; `kind`: "", "title", "big", "small", "muted", "note" or "warn"
    pub fn text(mut self, kind: &str, t: impl Into<String>) -> Self {
        self.0.push(json!({ "text": t.into(), "kind": kind }));
        self
    }
    /// the time of day, as the island's clock says it (the island keeps it ticking: this costs no call)
    pub fn clock_time(mut self, h24: bool) -> Self {
        self.0.push(json!({ "clock": "time", "h24": h24 }));
        self
    }
    /// a small picture or an icon, with a line of words (and one under it): what a song is, a download. `tile`: a `data:` raster
    /// picture, `"keep"` for the one this widget showed last, or "" for the `icon` instead.
    pub fn item(mut self, tile: &str, icon: &str, text: &str, sub: &str) -> Self {
        self.0.push(json!({ "item": { "tile": tile, "icon": icon, "text": text, "sub": sub } }));
        self
    }
    /// a round button with an icon ("play", "pause", "next"...): a press comes back as an `action` with `id` as `press`
    pub fn icon_button(mut self, icon: &str, press: &str, label: &str) -> Self {
        self.0.push(json!({ "icon_button": { "icon": icon, "press": press, "label": label } }));
        self
    }
    /// today's date, as the island's clock says it
    pub fn clock_date(mut self) -> Self {
        self.0.push(json!({ "clock": "date" }));
        self
    }
    /// a few thin bars stacked, each `(label, part 0 to 1, colour "#rrggbb", words at its right)`; the words show in a tooltip
    pub fn bars(mut self, items: &[(&str, f32, &str, &str)]) -> Self {
        self.0.push(json!({ "bars": items.iter().map(|(l, p, c, t)| json!({ "label": l, "part": p, "color": c, "text": t })).collect::<Vec<_>>() }));
        self
    }
    /// the same meters as circles, each a small ring with its label inside (`(label, part 0 to 1, colour "#rrggbb", words)`; the
    /// words show in a tooltip)
    pub fn rings(mut self, items: &[(&str, f32, &str, &str)]) -> Self {
        self.0.push(json!({ "rings": items.iter().map(|(l, p, c, t)| json!({ "label": l, "part": p, "color": c, "text": t })).collect::<Vec<_>>() }));
        self
    }
    /// the same bars, each with a few words under it (in a tooltip; a widget shows the bars only)
    pub fn bars_with_sub(mut self, items: &[(&str, f32, &str, &str, &str)]) -> Self {
        self.0.push(json!({ "bars": items.iter().map(|(l, p, c, t, s)| json!({ "label": l, "part": p, "color": c, "text": t, "sub": s })).collect::<Vec<_>>() }));
        self
    }
    /// a thin bar, 0 to 1, with a few words at its right
    pub fn bar(mut self, part: f32, text: &str) -> Self {
        self.0.push(json!({ "bar": part, "text": text }));
        self
    }
    /// a title with a button at each side (the ids they send; "" for none), and an id for a press on the title
    pub fn head(mut self, title: &str, prev: &str, next: &str, press: &str) -> Self {
        self.0.push(json!({ "head": title, "prev": prev, "next": next, "press": press }));
        self
    }
    /// a grid of up to seven columns (a month): the column labels, and the cells, row by row
    pub fn grid(mut self, labels: &[&str], cells: &[Cell]) -> Self {
        let cells: Vec<Value> = cells.iter().map(|c| json!({ "t": c.t, "id": c.id, "state": c.state, "dots": c.dots })).collect();
        self.0.push(json!({ "grid": { "cols": labels.len().clamp(1, 7), "labels": labels, "cells": cells } }));
        self
    }
    /// rows of a list; `empty` is said when there are none
    pub fn rows(mut self, rows: &[Row], empty: &str) -> Self {
        let rows: Vec<Value> = rows.iter().map(|r| json!({ "title": r.title, "sub": r.sub, "time": r.time, "past": r.past })).collect();
        self.0.push(json!({ "rows": rows, "empty": empty }));
        self
    }
    /// up to four buttons: (id, label)
    pub fn buttons(mut self, items: &[(&str, &str)]) -> Self {
        self.0.push(json!({ "buttons": items.iter().map(|(i, l)| json!({ "id": i, "label": l })).collect::<Vec<_>>() }));
        self
    }
    fn to_json(&self) -> Value {
        Value::Array(self.0.clone())
    }
}

// ------------------------------------------------------------------------------------------------ a window of its own

/// A small window the island opens for the plugin: you can move it, and it says whose it is. The module describes it (words,
/// a field, a few choices, up to three buttons) and the island draws it. A button that was pressed comes back as an `action`
/// event with `action == "modal"` and `args` = `{ "button": id, "values": { name: what it held }, "closed": bool }`. A button made
/// with [`Modal::stay`] keeps the window open for the module to answer with the next step (a reply with `Reply::modal` again
/// replaces the window); any other button closes it. A module has one at a time.
#[derive(Default, Clone, Debug)]
pub struct Modal {
    title: String,
    width: Option<u32>,
    at: Option<Value>,
    busy: String,
    blocks: Vec<Value>,
    buttons: Vec<Value>,
}

impl Modal {
    pub fn new(title: impl Into<String>) -> Self {
        Modal { title: title.into(), ..Modal::default() }
    }
    /// 280 to 520 px
    pub fn width(mut self, px: u32) -> Self {
        self.width = Some(px);
        self
    }
    /// Where it opens the first time: "center" (the default), "island", "cursor", an edge or a corner ("top", "bottom-right"...).
    /// After you have moved it, it opens where you left it.
    pub fn at(mut self, place: impl Into<String>) -> Self {
        self.at = Some(Value::String(place.into()));
        self
    }
    /// Where its middle goes, as a part of the screen it opens on (0 to 1 each way).
    pub fn at_part(mut self, x: f64, y: f64) -> Self {
        self.at = Some(json!({ "x": x, "y": y }));
        self
    }
    /// A line to say while something is being done (the buttons wait).
    pub fn busy(mut self, text: impl Into<String>) -> Self {
        self.busy = text.into();
        self
    }
    /// a paragraph
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.blocks.push(json!({ "text": t.into() }));
        self
    }
    /// a heading, "muted", "note" or "warn": a paragraph of that kind
    pub fn text_of(mut self, kind: &str, t: impl Into<String>) -> Self {
        self.blocks.push(json!({ "text": t.into(), "kind": kind }));
        self
    }
    /// a title and what it means, under it, for each (what a thing may see, what a word means)
    pub fn facts(mut self, items: &[(&str, &str)]) -> Self {
        self.blocks.push(json!({ "facts": items.iter().map(|(t, x)| json!({ "title": t, "text": x })).collect::<Vec<_>>() }));
        self
    }
    /// one line of text; what it holds comes back in `values[name]`
    pub fn field(mut self, name: &str, value: &str, placeholder: &str) -> Self {
        self.blocks.push(json!({ "field": name, "value": value, "placeholder": placeholder }));
        self
    }
    /// a few options, one picked (`values[name]` is its id)
    pub fn choice(mut self, name: &str, options: &[(&str, &str)], picked: &str) -> Self {
        self.blocks.push(json!({ "choice": name, "options": options.iter().map(|(i, l)| json!({ "id": i, "label": l })).collect::<Vec<_>>(), "value": picked }));
        self
    }
    /// a list of rows (id, label, a note at the right), one picked (`values[name]` is its id)
    pub fn list(mut self, name: &str, rows: &[(&str, &str, &str)], picked: &str) -> Self {
        self.blocks.push(json!({ "list": name, "items": rows.iter().map(|(i, l, s)| json!({ "id": i, "label": l, "sub": s })).collect::<Vec<_>>(), "value": picked }));
        self
    }
    /// a bar: 0 to 1, or `None` for one that is moving, with a line under it
    pub fn progress(mut self, part: Option<f32>, text: &str) -> Self {
        self.blocks.push(json!({ "progress": part, "text": text }));
        self
    }
    /// a button (at most three): pressing it closes the window
    pub fn button(mut self, id: &str, label: &str) -> Self {
        self.buttons.push(json!({ "id": id, "label": label }));
        self
    }
    /// the main button
    pub fn primary(mut self, id: &str, label: &str) -> Self {
        self.buttons.push(json!({ "id": id, "label": label, "primary": true }));
        self
    }
    /// a button that asks for care (it deletes, it resets)
    pub fn danger(mut self, id: &str, label: &str) -> Self {
        self.buttons.push(json!({ "id": id, "label": label, "danger": true }));
        self
    }
    /// a button that leaves the window open: the module answers with the next step
    pub fn stay(mut self, id: &str, label: &str, primary: bool) -> Self {
        self.buttons.push(json!({ "id": id, "label": label, "primary": primary, "stay": true }));
        self
    }
    fn to_json(&self) -> Value {
        json!({ "title": self.title, "width": self.width, "at": self.at.clone().unwrap_or(Value::Null), "busy": self.busy, "blocks": self.blocks, "buttons": self.buttons })
    }
}

/// What a call answers. Everything is optional: what a reply does not mention stays as it was.
#[derive(Default, Clone, Debug)]
pub struct Reply {
    card: Option<Option<Card>>,
    pill: Option<Option<Pill>>,
    rings: Option<Vec<Ring>>,
    say: Vec<Say>,
    events: Option<Option<Vec<CalEvent>>>,
    quiet: Option<bool>,
    note: Option<Option<String>>,
    wake_ms: Option<u64>,
    media: Option<(String, String, f64)>,
    modal: Option<Option<Modal>>,
    run: Vec<(String, Vec<String>)>,
    store: Option<Value>,
    more: Option<Vec<Card>>,
    widgets: Vec<(String, Blocks, Blocks)>,
    pane: Option<Option<(String, Blocks, bool)>>,
    watch: Option<Vec<u32>>,
    reveal: Option<String>,
    fetch: Vec<Value>,
    open: Option<String>,
    focus: Option<(Option<String>, Option<String>)>,
    cancel: Vec<String>,
    browser: Option<Value>,
    rest: Option<Vec<String>>,
}

impl Reply {
    /// Which of the resting islands of the manifest (`rests`) are on now: while one is, and the user has not turned it off, the
    /// collapsed island shows it. An empty list says none is. The last one told stands until the next.
    pub fn rest(mut self, ids: &[&str]) -> Self {
        self.rest = Some(ids.iter().map(|s| s.to_string()).collect());
        self
    }
    /// Watch these programs (their process ids, at most 4): each second the module is told what they use, in an `event ==
    /// "usage"`. An empty list stops it. The last one asked for is kept. Needs the `usage` permission.
    pub fn watch(mut self, pids: &[u32]) -> Self {
        self.watch = Some(pids.to_vec());
        self
    }
    /// Ask the host for an address (up to 2 in a reply) and be told what came back, in an `event == "fetched"` with this `id`.
    /// Only a host the manifest names in `permissions.net` is reached, over https; a redirect is not followed. `method` is "GET"
    /// or "POST" (with a `body`).
    pub fn fetch(mut self, id: &str, method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> Self {
        let h: serde_json::Map<String, Value> = headers.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        self.fetch.push(json!({ "id": id, "method": method, "url": url, "headers": h, "body": body }));
        self
    }
    /// Open a link now (the browser, an app): it has to start with one of the prefixes the manifest names in `permissions.open`.
    pub fn open(mut self, url: impl Into<String>) -> Self {
        self.open = Some(url.into());
        self
    }
    /// Bring a window to the front: the program (its path, as a window told it) and a word of its title. Needs `programs`.
    pub fn focus(mut self, exe: Option<&str>, title: Option<&str>) -> Self {
        self.focus = Some((exe.map(str::to_string), title.map(str::to_string)));
        self
    }
    /// Take back the briefs that have these keys ([`Say::pill`] with [`SayPill::key`]).
    pub fn cancel(mut self, keys: &[&str]) -> Self {
        self.cancel = keys.iter().map(|k| k.to_string()).collect();
        self
    }
    /// What it asks of the browser extension (needs `browser`): `config` (`{ hosts, hostWords, titleWords, safe, images }`: the sites
    /// that are never read, the buttons that may be pressed, whether pictures are sent), `watch` (the pages to be told of, at most 8:
    /// `(browser, title)`), a button to `press` and a tab to send on (`go`); the last two are answered by a `browser_done` event with
    /// their `id`. Without a `config` the extensions read nothing.
    pub fn browser(mut self, ask: Value) -> Self {
        self.browser = Some(ask);
        self
    }
    /// Show a finished download in its folder: `path` is the [`Ended::path`] the module was told of. Needs the `downloads`
    /// permission.
    pub fn reveal(mut self, path: impl Into<String>) -> Self {
        self.reveal = Some(path.into());
        self
    }
    /// Open a window for the plugin (or show the next step of the one it has). See [`Modal`].
    pub fn modal(m: Modal) -> Self {
        Reply { modal: Some(Some(m)), ..Reply::default() }
    }
    /// also open this window
    pub fn with_modal(mut self, m: Modal) -> Self {
        self.modal = Some(Some(m));
        self
    }
    /// close the window it has open
    pub fn close_modal() -> Self {
        Reply { modal: Some(None), ..Reply::default() }
    }
    /// What a widget of the manifest shows now (and its tooltip). A widget keeps what it showed until it is answered again.
    pub fn widget(id: &str, show: Blocks, tip: Blocks) -> Self {
        Reply::default().with_widget(id, show, tip)
    }
    /// also draw this widget
    pub fn with_widget(mut self, id: &str, show: Blocks, tip: Blocks) -> Self {
        self.widgets.push((id.to_string(), show, tip));
        self
    }
    /// Show a pane: a view that replaces the cards. `open`: bring it up now (the user asked for it); otherwise it only
    /// changes what it shows if it is open.
    pub fn pane(title: &str, show: Blocks, open: bool) -> Self {
        Reply::default().with_pane(title, show, open)
    }
    /// also show this pane
    pub fn with_pane(mut self, title: &str, show: Blocks, open: bool) -> Self {
        self.pane = Some(Some((title.to_string(), show, open)));
        self
    }
    /// take the pane away (the cards are back)
    pub fn close_pane() -> Self {
        Reply { pane: Some(None), ..Reply::default() }
    }
    /// Have the host run a command (up to 4 in a reply) and tell you what it printed, in an `event == "command"` with this
    /// `id`. The command line has to fit one of the shapes the manifest lists in `permissions.run`: `["git", "-C", dir,
    /// "status"]` fits `"git -C * status"`.
    pub fn run(mut self, id: &str, line: &[&str]) -> Self {
        self.run.push((id.to_string(), line.iter().map(|t| t.to_string()).collect()));
        self
    }
    /// Keep this (at most 64 KB as text) for the next time: it comes back in `Event::store` with the next start. Needs the
    /// `storage` permission.
    pub fn store(mut self, value: Value) -> Self {
        self.store = Some(value);
        self
    }
    /// The plugin's cards after the first: they replace the ones it had (an empty list takes them away). Give each its own
    /// [`Card::id`].
    pub fn more_cards(mut self, cards: Vec<Card>) -> Self {
        self.more = Some(cards);
        self
    }
    /// nothing new: the island keeps what it shows
    pub fn nothing() -> Self {
        Reply::default()
    }
    /// show this card
    pub fn card(c: Card) -> Self {
        Reply { card: Some(Some(c)), ..Reply::default() }
    }
    /// take the card away
    pub fn clear() -> Self {
        Reply { card: Some(None), ..Reply::default() }
    }
    /// also show this card
    pub fn with_card(mut self, c: Card) -> Self {
        self.card = Some(Some(c));
        self
    }
    /// also take the card away
    pub fn without_card(mut self) -> Self {
        self.card = Some(None);
        self
    }
    /// Offer this pill: the collapsed island shows it while it has the highest rank of the pills on offer.
    pub fn pill(mut self, p: Pill) -> Self {
        self.pill = Some(Some(p));
        self
    }
    /// Take the pill back.
    pub fn no_pill(mut self) -> Self {
        self.pill = Some(None);
        self
    }
    /// Show these rings in the hub (what was there before is replaced; an empty list takes them away).
    pub fn rings(mut self, rings: Vec<Ring>) -> Self {
        self.rings = Some(rings);
        self
    }
    /// also drop a banner. `priority` is 1 to 100 (a plain notification is 0, a session that needs you is 90); the
    /// island shows one banner at a time, the highest first, and the user can mute the plugin or turn on do not disturb.
    pub fn say(mut self, text: impl Into<String>, priority: i32) -> Self {
        self.say.push(Say::new(text, priority));
        self
    }
    /// also drop a banner or a pill with more to say about it (see [`Say`]). What a module says as the answer to a button
    /// the user pressed is always shown.
    pub fn say_with(mut self, say: Say) -> Self {
        self.say.push(say);
        self
    }

    /// Bring these events to the island's calendar (it keeps them until the next call that says `events` again, or the
    /// plugin is switched off). The calendar view, and any plugin that works from the calendar, will have them.
    pub fn events(mut self, events: Vec<CalEvent>) -> Self {
        self.events = Some(Some(events));
        self
    }

    /// Take the events this plugin brought away.
    pub fn no_events(mut self) -> Self {
        self.events = Some(None);
        self
    }

    /// Ask the other plugins to hold their peace (something fills the screen), or say it is over. Banners that are
    /// `ambient` are not shown meanwhile.
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = Some(quiet);
        self
    }

    /// A line about how the plugin is doing, shown under its name in Settings > Plugins ("Connected", "Waiting for the game").
    pub fn note(mut self, text: impl Into<String>) -> Self {
        self.note = Some(Some(text.into()));
        self
    }

    /// Take that line away.
    pub fn no_note(mut self) -> Self {
        self.note = Some(None);
        self
    }

    /// Be called again in this many ms (an event "tick" with `wake` true). A module keeps time with it: it is not called
    /// otherwise when nothing happens. Between 50 ms and a day; the last one asked for is the one kept.
    pub fn wake_in(mut self, ms: u64) -> Self {
        self.wake_ms = Some(ms);
        self
    }

    /// Press a button of the player: "play_pause", "next", "previous" or "focus" (bring the player to the front). `source` is
    /// the [`Session::source`] of the player (empty for the one in front). Needs the `media_control` permission.
    pub fn media(mut self, action: impl Into<String>, source: impl Into<String>) -> Self {
        self.media = Some((action.into(), source.into(), 0.0));
        self
    }
    /// A control that takes a number: "seek" (to that second), "seek_by" (that many seconds forward, or back when negative) or
    /// "set_rate" (a playback speed, 0.1 to 16).
    pub fn media_to(mut self, action: impl Into<String>, source: impl Into<String>, value: f64) -> Self {
        self.media = Some((action.into(), source.into(), value));
        self
    }

    /// The reply as the JSON the island reads (a module can test what it answers with this).
    pub fn to_json(&self) -> Value {
        let mut o = serde_json::Map::new();
        match &self.card {
            Some(Some(c)) => {
                o.insert("card".into(), serde_json::to_value(c).unwrap_or(Value::Null));
            }
            Some(None) => {
                o.insert("card".into(), Value::Null);
            }
            None => {}
        }
        match &self.pill {
            Some(Some(p)) => {
                o.insert("pill".into(), serde_json::to_value(p).unwrap_or(Value::Null));
            }
            Some(None) => {
                o.insert("pill".into(), Value::Null);
            }
            None => {}
        }
        if let Some(r) = &self.rings {
            o.insert("rings".into(), serde_json::to_value(r).unwrap_or(Value::Null));
        }
        match &self.events {
            Some(Some(e)) => {
                o.insert("events".into(), serde_json::to_value(e).unwrap_or(Value::Null));
            }
            Some(None) => {
                o.insert("events".into(), Value::Null);
            }
            None => {}
        }
        o.insert("say".into(), serde_json::to_value(&self.say).unwrap_or(Value::Null));
        if let Some(q) = self.quiet {
            o.insert("quiet".into(), json!(q));
        }
        if !self.widgets.is_empty() {
            let mut w = serde_json::Map::new();
            for (id, show, tip) in &self.widgets {
                w.insert(id.clone(), json!({ "blocks": show.to_json(), "tip": tip.to_json() }));
            }
            o.insert("widgets".into(), Value::Object(w));
        }
        if let Some(cards) = &self.more {
            o.insert("cards_more".into(), serde_json::to_value(cards).unwrap_or(Value::Null));
        }
        if !self.run.is_empty() {
            o.insert("run".into(), Value::Array(self.run.iter().map(|(id, line)| json!({ "id": id, "line": line })).collect()));
        }
        if let Some(v) = &self.store {
            o.insert("store".into(), v.clone());
        }
        match &self.pane {
            Some(Some((title, show, open))) => {
                o.insert("pane".into(), json!({ "title": title, "blocks": show.to_json(), "open": open }));
            }
            Some(None) => {
                o.insert("pane".into(), Value::Null);
            }
            None => {}
        }
        match &self.note {
            Some(Some(t)) => {
                o.insert("note".into(), json!(t));
            }
            Some(None) => {
                o.insert("note".into(), Value::Null);
            }
            None => {}
        }
        match &self.modal {
            Some(Some(m)) => {
                o.insert("modal".into(), m.to_json());
            }
            Some(None) => {
                o.insert("modal".into(), Value::Null);
            }
            None => {}
        }
        if let Some(ms) = self.wake_ms {
            o.insert("wake_ms".into(), json!(ms));
        }
        if let Some(w) = &self.watch {
            o.insert("watch".into(), json!(w));
        }
        if let Some(r) = &self.reveal {
            o.insert("reveal".into(), json!(r));
        }
        if let Some(r) = &self.rest {
            o.insert("rest".into(), json!(r));
        }
        if !self.fetch.is_empty() {
            o.insert("fetch".into(), Value::Array(self.fetch.clone()));
        }
        if let Some(u) = &self.open {
            o.insert("open".into(), json!(u));
        }
        if let Some((exe, title)) = &self.focus {
            o.insert("focus".into(), json!({ "exe": exe, "title": title }));
        }
        if !self.cancel.is_empty() {
            o.insert("cancel".into(), json!(self.cancel));
        }
        if let Some(b) = &self.browser {
            o.insert("browser".into(), b.clone());
        }
        if let Some((a, s, v)) = &self.media {
            o.insert("media".into(), json!({ "action": a, "source": s, "value": v }));
        }
        Value::Object(o)
    }
}

// -------------------------------------------------------------------------------------- the glue the host calls

fn leak(bytes: Vec<u8>) -> (i32, i32) {
    let b = bytes.into_boxed_slice();
    let len = b.len() as i32;
    let ptr = Box::into_raw(b) as *mut u8 as usize as i32;
    (ptr, len)
}

#[doc(hidden)]
#[no_mangle]
pub extern "C" fn nadi_alloc(len: i32) -> i32 {
    leak(vec![0u8; len.max(0) as usize]).0
}

#[doc(hidden)]
#[no_mangle]
pub extern "C" fn nadi_free(ptr: i32, len: i32) {
    if ptr != 0 && len > 0 {
        unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as usize as *mut u8, len as usize))) }
    }
}

#[doc(hidden)]
pub fn dispatch(handler: fn(&Event) -> Reply, ptr: i32, len: i32) -> i64 {
    let input = unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len.max(0) as usize) };
    let ev: Event = serde_json::from_slice(input).unwrap_or_default();
    let out = serde_json::to_vec(&handler(&ev).to_json()).unwrap_or_default();
    let (p, n) = leak(out);
    (((p as u32 as u64) << 32) | n as u32 as u64) as i64
}

/// `plugin!(handle)` where `fn handle(&Event) -> Reply`: the one function the island calls.
#[macro_export]
macro_rules! plugin {
    ($handler:path) => {
        #[no_mangle]
        pub extern "C" fn nadi_call(ptr: i32, len: i32) -> i64 {
            $crate::dispatch($handler, ptr, len)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_time_is_right_across_zones_and_leap_days() {
        // 2026-10-09 15:30:45 UTC, a Friday
        let ms = 1_791_559_845_000i64;
        let utc = civil(ms, 0);
        assert_eq!((utc.year, utc.month, utc.day, utc.hour, utc.minute, utc.second), (2026, 10, 9, 15, 30, 45));
        assert_eq!(utc.weekday, 4);
        // Tokyo is nine hours ahead: it is already the next day
        let tokyo = civil(ms, 540);
        assert_eq!((tokyo.month, tokyo.day, tokyo.hour, tokyo.weekday), (10, 10, 0, 5));
        // 2028-02-29 exists; the epoch is a Thursday
        let leap = civil(1_835_395_200_000, 0);
        assert_eq!((leap.year, leap.month, leap.day), (2028, 2, 29));
        assert_eq!(civil(0, 0).weekday, 3);
        // before 1970 and a zone behind UTC
        let back = civil(-1000, -300);
        assert_eq!((back.year, back.month, back.day, back.hour, back.minute, back.second), (1969, 12, 31, 18, 59, 59));
    }

    #[test]
    fn a_reply_says_only_what_it_was_given() {
        let r = Reply::nothing().to_json();
        assert_eq!(r, json!({ "say": [] }));
        let r = Reply::nothing().no_pill().wake_in(500).quiet(true).to_json();
        assert_eq!(r["pill"], Value::Null);
        assert!(r.as_object().unwrap().contains_key("pill"));
        assert_eq!(r["wake_ms"], 500);
        assert_eq!(r["quiet"], true);
        assert!(!r.as_object().unwrap().contains_key("card"));
        let r = Reply::nothing().say_with(Say::new("3:30 PM", 10).pill("clock", "Fri").big().ambient()).to_json();
        assert_eq!(r["say"][0]["pill"]["look"], "big");
        assert_eq!(r["say"][0]["ambient"], true);
    }

    #[test]
    fn a_moment_inside_a_text() {
        assert_eq!(when(1000, false), "{when:1000}");
        assert_eq!(when(1000, true), "{when:1000:day}");
    }
}
