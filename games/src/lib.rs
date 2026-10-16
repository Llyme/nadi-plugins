//! Games: the game you are playing, for how long, and how it runs.
//!
//! A game is a program whose window covers its whole screen and has no title bar (the island tells which windows do: `full`).
//! It stays until its process ends, however often you switch away from it. While one runs, the module watches it (`usage`):
//! the CPU, the memory, the graphics card and the video memory it uses, and its frames per second.
//!
//! The collapsed island shows the game in front, with the numbers; the hub has a card for each game, up to four.

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// the games' card leads the hub's list; the pill sits below what is playing
const CARD_RANK: i32 = 10;
const PILL_RANK: i32 = 20;
/// how many games it follows at once (the island lets a module watch four programs)
const MAX_GAMES: usize = 4;
/// the card's graph reads 0 to 50 ms, with a line at 60 and at 30 frames per second
const GRAPH_MAX_MS: f32 = 50.0;
const GRAPH_MARKS: [f32; 2] = [16.7, 33.3];
/// what a full bar of video memory is, GB (the island does not know the card)
const VRAM_FULL_GB: f64 = 12.0;

/// programs that cover the screen and are not games
const IGNORE_EXE: &[&str] = &[
    "explorer.exe", "shellexperiencehost.exe", "searchhost.exe", "searchapp.exe", "startmenuexperiencehost.exe",
    "applicationframehost.exe", "textinputhost.exe", "lockapp.exe", "systemsettings.exe", "screenclippinghost.exe",
    "gamebar.exe", "widgets.exe",
];

/// Fullscreen video players, browsers (F11, a fullscreen video), presentations and remote desktops cover the screen too.
/// Matched as parts of the program's file name.
const NOT_GAMES: &[&str] = &[
    "potplayer", "vlc", "mpv", "mpc-hc", "mpc-be", "wmplayer", "kmplayer", "gomplayer", "smplayer", "video.ui", "moviesandtv",
    "chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "zoom", "teams", "powerpnt", "acrord", "mstsc", "vmconnect",
    "obs64", "obs32",
];

fn is_not_game(exe: &str) -> bool {
    let exe = exe.to_lowercase();
    IGNORE_EXE.contains(&exe.as_str()) || NOT_GAMES.iter().any(|n| exe.contains(n))
}

/// A name from the file when the program has none: `deep_rock-galactic.exe` is "Deep Rock Galactic".
fn name_from_path(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let stem = file.strip_suffix(".exe").or(file.strip_suffix(".EXE")).unwrap_or(file);
    let mut out = String::new();
    let mut up = true;
    for ch in stem.chars() {
        if ch == '_' || ch == '-' {
            out.push(' ');
            up = true;
        } else if up {
            out.extend(ch.to_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    out
}

struct Game {
    pid: u32,
    name: String,
    path: String,
    /// when it was first seen, ms
    since: u64,
    usage: Option<Usage>,
}

#[derive(Default)]
struct State {
    /// the games running, the one most recently in front first
    games: Vec<Game>,
    /// the icon of each program, told once, by path
    icons: HashMap<String, String>,
    /// whose icon the main card and the pill hold now (a picture is sent once, then "keep")
    sent: Option<u32>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn duration(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        "<1m".to_string()
    }
}

fn pct(v: f32) -> String {
    format!("{}%", v.round() as i32)
}

/// What the card of one game says.
fn card_of(g: &Game, now: u64, rank: i32) -> Card {
    let mut card = Card::new().icon("gamepad").rank(rank).title(g.name.clone()).sub(format!("Playing for {}", duration(now.saturating_sub(g.since))));
    let Some(u) = &g.usage else { return card };
    if let Some(fps) = u.fps {
        card = card.peek(format!("{} FPS", fps.round() as i64));
        let ms = 1000.0 / fps.max(1.0);
        card = card.facts(&[
            ("FPS", format!("{}", fps.round() as i64)),
            ("1% low", format!("{}", u.low_fps.unwrap_or(fps).round() as i64)),
            ("Frame time", format!("{ms:.1} ms")),
        ]);
        if u.frame_ms.len() > 1 {
            // (to a tenth of a millisecond: the graph cannot show more, and it is sent every second)
            let ms: Vec<f32> = u.frame_ms.iter().map(|v| (v * 10.0).round() / 10.0).collect();
            card = card.graph(&ms, GRAPH_MAX_MS, &GRAPH_MARKS);
        }
    } else if let Some(gpu) = u.gpu {
        card = card.peek(format!("GPU {}", pct(gpu)));
    }
    // the meters: the game's own share of each
    let mut rows: Vec<(String, String, Option<f32>)> = Vec::new();
    if let Some(gpu) = u.gpu {
        rows.push(("GPU".into(), pct(gpu), Some(gpu / 100.0)));
    }
    if let Some(v) = u.vram_gb {
        rows.push(("VRAM".into(), format!("{v:.1} GB"), Some((v / VRAM_FULL_GB).min(1.0) as f32)));
    }
    rows.push(("CPU".into(), pct(u.cpu), Some(u.cpu / 100.0)));
    rows.push(("RAM".into(), format!("{:.1} GB", u.ram_gb), Some(u.ram / 100.0)));
    card.list(&rows)
}

fn reply_for(st: &mut State, now: u64) -> Reply {
    let watch: Vec<u32> = st.games.iter().map(|g| g.pid).collect();
    let Some(front) = st.games.first() else {
        st.sent = None;
        return Reply::clear().more_cards(Vec::new()).no_pill().quiet(false).watch(&[]);
    };
    // (the icon goes out once, then "keep")
    let icon = st.icons.get(&front.path).cloned().unwrap_or_default();
    let image = if icon.is_empty() {
        String::new()
    } else if st.sent == Some(front.pid) {
        "keep".to_string()
    } else {
        icon
    };
    st.sent = (!image.is_empty()).then_some(front.pid);
    let mut card = card_of(front, now, CARD_RANK);
    if !image.is_empty() {
        card = card.tile(image.clone());
    }
    let others: Vec<Card> = st.games.iter().skip(1).map(|g| card_of(g, now, CARD_RANK + 1).id(format!("g{}", g.pid))).collect();

    let mut right = front.usage.as_ref().and_then(|u| u.fps).map(|f| format!("{} FPS", f.round() as i64)).unwrap_or_default();
    if st.games.len() > 1 {
        right = if right.is_empty() { format!("+{}", st.games.len() - 1) } else { format!("{right} · +{}", st.games.len() - 1) };
    }
    let mut pill = Pill::new().icon("gamepad").image(image).title(front.name.clone()).right(right).rank(PILL_RANK).size(320.0, 40.0);
    if let Some(u) = &front.usage {
        let gpu = u.gpu.unwrap_or(0.0);
        pill = pill.bars(&[gpu / 100.0, u.cpu / 100.0, u.ram / 100.0]);
    }
    Reply::card(card).more_cards(others).pill(pill).quiet(true).watch(&watch)
}

/// Keep the list of games up to date with the windows on the screens: the ones that cover their screen join, the one in
/// front goes first.
fn see_windows(st: &mut State, ev: &Event) {
    for w in &ev.windows {
        if let Some(icon) = &w.icon {
            st.icons.insert(w.path.clone(), icon.clone());
        }
    }
    // (the list is in front-first order: put them in back to front, so the one in front ends up first)
    let mut seen: Vec<&Window> = Vec::new();
    for w in ev.windows.iter().filter(|w| w.full && !w.tool && !w.path.is_empty() && !is_not_game(&w.exe)) {
        if !seen.iter().any(|s| s.pid == w.pid) {
            seen.push(w);
        }
    }
    for w in seen.into_iter().rev() {
        match st.games.iter().position(|g| g.pid == w.pid) {
            Some(i) => {
                let g = st.games.remove(i);
                st.games.insert(0, g);
            }
            None => {
                let name = if w.name.is_empty() { name_from_path(&w.path) } else { w.name.clone() };
                st.games.insert(0, Game { pid: w.pid, name, path: w.path.clone(), since: ev.now, usage: None });
            }
        }
    }
    st.games.truncate(MAX_GAMES);
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        match ev.event.as_str() {
            "start" => {
                *st = State::default();
                Reply::nothing()
            }
            "processes" => {
                see_windows(&mut st, ev);
                reply_for(&mut st, ev.now)
            }
            "usage" => {
                for u in &ev.usage {
                    if let Some(g) = st.games.iter_mut().find(|g| g.pid == u.pid) {
                        g.usage = Some(u.clone());
                    }
                }
                // a game lasts until its process ends, however often you switch away from it
                for u in ev.usage.iter().filter(|u| !u.alive) {
                    st.games.retain(|g| g.pid != u.pid);
                }
                reply_for(&mut st, ev.now)
            }
            _ => Reply::nothing(),
        }
    })
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    fn win(exe: &str, pid: u32, full: bool) -> Window {
        Window { exe: exe.into(), pid, title: "t".into(), full, path: format!("C:\\Games\\{exe}"), name: String::new(), ..Window::default() }
    }

    fn processes(now: u64, windows: Vec<Window>) -> Event {
        Event { event: "processes".into(), now, windows, ..Event::default() }
    }

    fn usage(now: u64, list: Vec<Usage>) -> Event {
        Event { event: "usage".into(), now, usage: list, ..Event::default() }
    }

    fn fresh() {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }

    #[test]
    fn a_program_that_covers_its_screen_is_a_game_and_a_browser_or_a_player_is_not() {
        fresh();
        let r = handle(&processes(1000, vec![win("chrome.exe", 1, true), win("vlc.exe", 2, true), win("code.exe", 3, false), win("deep_rock-galactic.exe", 4, true)])).to_json();
        assert_eq!(r["card"]["title"], "Deep Rock Galactic");
        assert_eq!(r["pill"]["title"], "Deep Rock Galactic");
        assert_eq!(r["quiet"], true);
        assert_eq!(r["watch"], json!([4]));
        // nothing covers the screen and nothing was running: no card
        fresh();
        let r = handle(&processes(1000, vec![win("code.exe", 3, false)])).to_json();
        assert_eq!(r["card"], Value::Null);
        assert_eq!(r["pill"], Value::Null);
        assert_eq!(r["quiet"], false);
    }

    #[test]
    fn a_game_stays_while_its_window_is_not_in_the_list_and_goes_when_its_process_ends() {
        fresh();
        handle(&processes(1000, vec![win("a.exe", 4, true)]));
        // it was switched away from (or minimized): its window is not covering the screen, but the game stays
        let r = handle(&processes(3000, vec![win("code.exe", 3, false)])).to_json();
        assert_eq!(r["card"]["title"], "A");
        assert_eq!(r["card"]["sub"], "Playing for <1m");
        // it ended
        let r = handle(&usage(5000, vec![Usage { pid: 4, alive: false, ..Usage::default() }])).to_json();
        assert_eq!(r["card"], Value::Null);
        assert_eq!(r["pill"], Value::Null);
        assert_eq!(r["watch"], json!([]));
    }

    #[test]
    fn the_game_in_front_leads_and_the_others_have_cards_of_their_own() {
        fresh();
        handle(&processes(1000, vec![win("a.exe", 1, true)]));
        let r = handle(&processes(2000, vec![win("b.exe", 2, true), win("a.exe", 1, true)])).to_json();
        assert_eq!(r["card"]["title"], "B");
        assert_eq!(r["cards_more"][0]["title"], "A");
        assert_eq!(r["cards_more"][0]["id"], "g1");
        assert_eq!(r["pill"]["right"], "+1");
        assert_eq!(r["watch"], json!([2, 1]));
        // alt-tab to the first: it leads again
        let r = handle(&processes(3000, vec![win("a.exe", 1, true), win("b.exe", 2, true)])).to_json();
        assert_eq!(r["card"]["title"], "A");
    }

    #[test]
    fn what_a_game_uses_is_in_its_card_and_its_pill() {
        fresh();
        handle(&processes(0, vec![win("a.exe", 1, true)]));
        let u = Usage { pid: 1, alive: true, cpu: 12.4, ram_gb: 6.25, ram: 20.0, gpu: Some(73.0), vram_gb: Some(4.0), fps: Some(144.0), low_fps: Some(98.0), frame_ms: vec![7.0, 6.9, 7.1], ..Usage::default() };
        let r = handle(&usage(3_660_000, vec![u])).to_json();
        let c = &r["card"];
        assert_eq!(c["sub"], "Playing for 1h 1m");
        assert_eq!(c["peek"], "144 FPS");
        assert_eq!(c["body"][0]["rows"][1], json!(["1% low", "98"]));
        assert_eq!(c["body"][1]["kind"], "graph");
        assert_eq!(c["body"][1]["max"], 50.0);
        let rows = c["body"][2]["rows"].as_array().unwrap();
        assert_eq!(rows.iter().map(|r| r["title"].as_str().unwrap()).collect::<Vec<_>>(), ["GPU", "VRAM", "CPU", "RAM"]);
        assert_eq!(rows[0]["sub"], "73%");
        assert_eq!(rows[3]["sub"], "6.2 GB");
        assert_eq!(r["pill"]["right"], "144 FPS");
        assert_eq!(r["pill"]["bars"].as_array().unwrap().len(), 3);
        // without frames the card says the graphics card; without a graphics counter, it has no such rows
        let u = Usage { pid: 1, alive: true, cpu: 5.0, ram_gb: 1.0, ram: 3.0, gpu: Some(40.0), ..Usage::default() };
        let r = handle(&usage(3_661_000, vec![u])).to_json();
        assert_eq!(r["card"]["peek"], "GPU 40%");
        let u = Usage { pid: 1, alive: true, cpu: 5.0, ram_gb: 1.0, ram: 3.0, ..Usage::default() };
        let r = handle(&usage(3_662_000, vec![u])).to_json();
        assert_eq!(r["card"]["body"][0]["rows"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn the_icon_is_sent_once_then_kept() {
        fresh();
        let mut w = win("a.exe", 1, true);
        w.icon = Some("data:image/png;base64,AAAA".into());
        let r = handle(&processes(0, vec![w.clone()])).to_json();
        assert_eq!(r["card"]["image"], "data:image/png;base64,AAAA");
        assert_eq!(r["pill"]["image"], "data:image/png;base64,AAAA");
        w.icon = None;
        let r = handle(&processes(2000, vec![w])).to_json();
        assert_eq!(r["card"]["image"], "keep");
        assert_eq!(r["pill"]["image"], "keep");
    }

    #[test]
    fn a_name_from_a_file() {
        assert_eq!(name_from_path("C:\\Games\\deep_rock-galactic.exe"), "Deep Rock Galactic");
        assert_eq!(name_from_path("hades2.exe"), "Hades2");
    }
}
