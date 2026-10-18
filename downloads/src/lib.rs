//! Downloads: what your browsers, Steam and qBittorrent are downloading right now, on one card.
//!
//! The island watches what those apps leave on disk and tells the module what is downloading (`observed`) and what stopped
//! showing up (`ended`: done, or given up). The module keeps the list: it smooths the speed, drops a download that stands still
//! (it comes back when it moves), and keeps a finished one on the card until you press its button.

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashMap;

const CARD_RANK: i32 = 20;
/// nothing came in for this long: it is stuck or paused, so it goes (and comes back when it moves)
const STALL_MS: u64 = 5000;
/// how many samples of the combined speed the card draws (one for each look, about every 1.5 s)
const HISTORY: usize = 30;
/// the most rows and buttons the card shows
const MAX_ACTIVE: usize = 8;
const MAX_DONE: usize = 4;

struct Item {
    id: String,
    source: String,
    kind: String,
    name: String,
    received: u64,
    total: Option<u64>,
    /// bytes per second
    speed: f64,
    done: bool,
    /// where the finished file is
    open_path: Option<String>,
    last_bytes: u64,
    last_at: u64,
    started: u64,
    /// when bytes last came in
    moved: u64,
}

#[derive(Default)]
struct State {
    items: Vec<Item>,
    /// downloads that were dropped for standing still: id -> the bytes they had. One that is still stuck is not listed
    /// again; it comes back when it moves.
    stalled: HashMap<String, u64>,
    /// the combined speed, one sample for each look
    speeds: Vec<f64>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn bytes(n: f64) -> String {
    if n < 1024.0 {
        return format!("{} B", n.round() as i64);
    }
    let mut v = n / 1024.0;
    let mut unit = 0;
    let units = ["KB", "MB", "GB", "TB"];
    while v >= 1024.0 && unit < units.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if v >= 100.0 {
        format!("{} {}", v.round() as i64, units[unit])
    } else {
        format!("{v:.1} {}", units[unit])
    }
}

fn speed_text(speed: f64) -> String {
    if speed > 1024.0 {
        format!("{}/s", bytes(speed))
    } else {
        String::new()
    }
}

/// What one look saw, and what stopped showing up, into the list.
fn merge(st: &mut State, now: u64, observed: &[Download], ended: &[Ended]) {
    for o in observed {
        let moving = o.speed.map_or(false, |s| s > 0.0);
        match st.items.iter_mut().find(|i| i.id == o.id && !i.done) {
            Some(it) => {
                if o.received > it.received || moving {
                    it.moved = now;
                }
                let dt = now.saturating_sub(it.last_at) as f64 / 1000.0;
                if dt >= 0.5 {
                    let inst = o.received.saturating_sub(it.last_bytes) as f64 / dt;
                    it.speed = if it.speed == 0.0 { inst } else { it.speed * 0.6 + inst * 0.4 };
                    it.last_bytes = o.received;
                    it.last_at = now;
                }
                if let Some(s) = o.speed {
                    it.speed = s;
                }
                it.received = o.received;
                it.total = o.total;
                it.name = o.name.clone();
            }
            None => {
                // a finished one that is still on the list is not added again
                if st.items.iter().any(|i| i.id == o.id) {
                    continue;
                }
                // one that was dropped for standing still stays off the list until it moves
                match st.stalled.get(&o.id) {
                    Some(&b) if o.received <= b && !moving => continue,
                    Some(_) => {
                        st.stalled.remove(&o.id);
                    }
                    None => {}
                }
                st.items.push(Item {
                    id: o.id.clone(),
                    source: o.source.clone(),
                    kind: o.kind.clone(),
                    name: o.name.clone(),
                    received: o.received,
                    total: o.total,
                    speed: 0.0,
                    done: false,
                    open_path: None,
                    last_bytes: o.received,
                    last_at: now,
                    started: now,
                    moved: now,
                });
            }
        }
    }
    for e in ended {
        let Some(i) = st.items.iter().position(|i| i.id == e.id && !i.done) else { continue };
        if e.finished {
            let it = &mut st.items[i];
            it.done = true;
            it.speed = 0.0;
            it.open_path = e.path.clone();
            if let Some(t) = it.total {
                it.received = t;
            }
        } else {
            st.items.remove(i);
        }
    }
    // listed and not finished, but nothing has moved for a while: stuck or paused
    if st.stalled.len() > 64 {
        st.stalled.clear();
    }
    let stalled = &mut st.stalled;
    st.items.retain(|i| {
        let stuck = !i.done && now.saturating_sub(i.moved) > STALL_MS;
        if stuck {
            stalled.insert(i.id.clone(), i.received);
        }
        !stuck
    });
    let total: f64 = st.items.iter().filter(|i| !i.done).map(|i| i.speed).sum();
    st.speeds.push(total);
    if st.speeds.len() > HISTORY {
        st.speeds.remove(0);
    }
}

/// "42% · 3.1 MB/s", or what Steam gives: its size and the speed (it writes no progress anywhere)
fn meta(it: &Item) -> String {
    let speed = speed_text(it.speed);
    let join = |a: String, b: String| match (a.is_empty(), b.is_empty()) {
        (false, false) => format!("{a} · {b}"),
        (false, true) => a,
        _ => b,
    };
    if it.kind == "steam" {
        let words = join(it.total.map(|t| bytes(t as f64)).unwrap_or_default(), speed);
        return if words.is_empty() { "Downloading".to_string() } else { words };
    }
    match it.total {
        Some(t) if t > 0 => join(format!("{}%", ((it.received as f64 / t as f64) * 100.0).floor().min(100.0) as i64), speed),
        _ => join(bytes(it.received as f64), speed),
    }
}

fn progress(it: &Item) -> Option<f32> {
    match (it.kind.as_str(), it.total) {
        // (Steam shows no progress anywhere, so neither does the card)
        ("steam", _) | (_, None) | (_, Some(0)) => None,
        (_, Some(t)) => Some((it.received as f32 / t as f32).clamp(0.0, 1.0)),
    }
}

fn cut(name: &str, n: usize) -> String {
    if name.chars().count() <= n {
        name.to_string()
    } else {
        format!("{}…", name.chars().take(n - 1).collect::<String>().trim_end())
    }
}

fn reply_for(st: &State) -> Reply {
    if st.items.is_empty() {
        return Reply::clear().with_widget("file", Blocks::new(), Blocks::new()).with_widget("speed", Blocks::new(), Blocks::new()).rest(&[]);
    }
    let mut active: Vec<&Item> = st.items.iter().filter(|i| !i.done).collect();
    active.sort_by_key(|i| i.started);
    let done: Vec<&Item> = st.items.iter().filter(|i| i.done).collect();
    let speed: f64 = active.iter().map(|i| i.speed).sum();

    let sub = match (active.len(), done.len()) {
        (0, d) => format!("{d} done"),
        (a, 0) => format!("{a} downloading"),
        (a, d) => format!("{a} downloading · {d} done"),
    };
    let mut card = Card::new().icon("download").rank(CARD_RANK).title("Downloads").sub(sub);
    card = card.peek(if active.is_empty() { format!("{} done", done.len()) } else { speed_text(speed) });

    // the combined speed over the last minute; the top of the graph is the fastest of it
    let top = st.speeds.iter().cloned().fold(0.0, f64::max);
    if top > 0.0 && !active.is_empty() {
        let values: Vec<f32> = st.speeds.iter().map(|v| *v as f32).collect();
        card = card.graph(&values, (top * 1.15).max(100_000.0) as f32, &[]);
    }
    if !active.is_empty() {
        let rows: Vec<(String, String, Option<f32>)> = active.iter().take(MAX_ACTIVE).map(|i| (format!("{} · {}", i.source, i.name), meta(i), progress(i))).collect();
        card = card.list(&rows);
    }
    if !done.is_empty() {
        let buttons: Vec<Button> = done
            .iter()
            .take(MAX_DONE)
            .map(|i| {
                let verb = if i.kind == "browser" && i.open_path.is_some() { "Show" } else { "Clear" };
                Button::new(format!("{verb} {}", cut(&i.name, 28)), format!("done:{}", i.id))
            })
            .collect();
        card = card.buttons(buttons);
    }
    // the resting island (its manifest's `rests`), while something comes down: what, at the left, and how fast, at the right
    let file = match active.first() {
        Some(i) => Blocks::new().item("", "download", &cut(&i.name, 28), &i.source),
        None => Blocks::new(),
    };
    let rate = if active.is_empty() { Blocks::new() } else { Blocks::new().text("", speed_text(speed)) };
    let on: &[&str] = if active.is_empty() { &[] } else { &["downloading"] };
    Reply::card(card).with_widget("file", file, Blocks::new()).with_widget("speed", rate, Blocks::new()).rest(on)
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        match ev.event.as_str() {
            "start" => {
                *st = State::default();
                Reply::nothing()
            }
            "downloads" => {
                merge(&mut st, ev.now, &ev.observed, &ev.ended);
                reply_for(&st)
            }
            "action" => {
                let Some(id) = ev.action.strip_prefix("done:") else { return Reply::nothing() };
                let Some(i) = st.items.iter().position(|i| i.id == id && i.done) else { return Reply::nothing() };
                let it = st.items.remove(i);
                let reply = reply_for(&st);
                // a browser's download is shown in its folder, with the file selected
                match (it.kind.as_str(), it.open_path) {
                    ("browser", Some(path)) => reply.reveal(path),
                    _ => reply,
                }
            }
            _ => Reply::nothing(),
        }
    })
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(id: &str, kind: &str, received: u64, total: Option<u64>) -> Download {
        Download { id: id.into(), source: if kind == "steam" { "Steam" } else { "Chrome" }.into(), kind: kind.into(), name: format!("{id}.bin"), received, total, speed: None }
    }

    fn look(now: u64, observed: Vec<Download>, ended: Vec<Ended>) -> Event {
        Event { event: "downloads".into(), now, observed, ended, ..Event::default() }
    }

    fn fresh() {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }

    #[test]
    fn a_download_is_on_the_card_with_its_progress_and_speed() {
        fresh();
        handle(&look(0, vec![dl("a", "browser", 0, Some(100_000_000))], vec![]));
        let r = handle(&look(2000, vec![dl("a", "browser", 20_000_000, Some(100_000_000))], vec![])).to_json();
        assert_eq!(r["card"]["title"], "Downloads");
        assert_eq!(r["card"]["sub"], "1 downloading");
        let row = &r["card"]["body"][1]["rows"][0];
        assert_eq!(row["title"], "Chrome · a.bin");
        assert_eq!(row["sub"], "20% · 9.5 MB/s");
        assert_eq!(row["progress"].as_f64().map(|p| (p * 100.0).round()), Some(20.0));
        assert_eq!(r["card"]["peek"], "9.5 MB/s");
        // the speed over the last look is drawn
        assert_eq!(r["card"]["body"][0]["kind"], "graph");
    }

    #[test]
    fn steam_says_its_size_and_speed_and_no_progress() {
        fresh();
        let mut o = dl("s", "steam", 10, Some(24_000_000_000));
        o.speed = Some(20_000_000.0);
        let r = handle(&look(0, vec![o.clone()], vec![])).to_json();
        let row = r["card"]["body"].as_array().unwrap().iter().find(|b| b["kind"] == "list").unwrap()["rows"][0].clone();
        assert_eq!(row["sub"], "22.4 GB");
        assert_eq!(row["progress"], Value::Null);
        o.received = 20;
        let r = handle(&look(1500, vec![o], vec![])).to_json();
        let row = r["card"]["body"].as_array().unwrap().iter().find(|b| b["kind"] == "list").unwrap()["rows"][0].clone();
        assert_eq!(row["sub"], "22.4 GB · 19.1 MB/s");
    }

    #[test]
    fn a_finished_download_waits_for_its_button_and_a_browser_one_shows_its_folder() {
        fresh();
        handle(&look(0, vec![dl("a", "browser", 50, Some(100))], vec![]));
        let ended = Ended { id: "a".into(), finished: true, path: Some("C:\\Users\\me\\Downloads\\a.bin".into()) };
        let r = handle(&look(1500, vec![], vec![ended])).to_json();
        assert_eq!(r["card"]["sub"], "1 done");
        assert_eq!(r["card"]["peek"], "1 done");
        let b = &r["card"]["body"][0]["items"][0];
        assert_eq!(b["label"], "Show a.bin");
        assert_eq!(b["action"], "done:a");
        // it is not added again while it is on the card
        let r = handle(&look(3000, vec![dl("a", "browser", 100, Some(100))], vec![])).to_json();
        assert_eq!(r["card"]["sub"], "1 done");
        // pressed: gone, and the folder opens
        let pressed = Event { event: "action".into(), action: "done:a".into(), ..Event::default() };
        let r = handle(&pressed).to_json();
        assert_eq!(r["reveal"], "C:\\Users\\me\\Downloads\\a.bin");
        assert_eq!(r["card"], Value::Null);
    }

    #[test]
    fn one_that_was_given_up_goes_and_a_torrent_is_only_put_away() {
        fresh();
        handle(&look(0, vec![dl("a", "browser", 50, Some(100)), dl("t", "torrent", 5, Some(10))], vec![]));
        let r = handle(&look(1500, vec![dl("t", "torrent", 6, Some(10))], vec![Ended { id: "a".into(), finished: false, path: None }])).to_json();
        assert_eq!(r["card"]["body"].as_array().unwrap().iter().find(|b| b["kind"] == "list").unwrap()["rows"].as_array().unwrap().len(), 1);
        handle(&look(3000, vec![dl("t", "torrent", 6, Some(10))], vec![Ended { id: "t".into(), finished: true, path: Some("C:\\t".into()) }]));
        let r = handle(&Event { event: "action".into(), action: "done:t".into(), ..Event::default() }).to_json();
        assert!(r.get("reveal").is_none());
    }

    #[test]
    fn a_download_that_stands_still_goes_and_comes_back_when_it_moves() {
        fresh();
        handle(&look(0, vec![dl("a", "browser", 50, Some(100))], vec![]));
        // still the same bytes, 6 seconds on: it is stuck
        let r = handle(&look(6000, vec![dl("a", "browser", 50, Some(100))], vec![])).to_json();
        assert_eq!(r["card"], Value::Null);
        // seen again with the same bytes: not listed
        let r = handle(&look(7500, vec![dl("a", "browser", 50, Some(100))], vec![])).to_json();
        assert_eq!(r["card"], Value::Null);
        // it moved
        let r = handle(&look(9000, vec![dl("a", "browser", 80, Some(100))], vec![])).to_json();
        assert_eq!(r["card"]["sub"], "1 downloading");
    }

    #[test]
    fn sizes_are_written_short() {
        assert_eq!(bytes(512.0), "512 B");
        assert_eq!(bytes(1536.0), "1.5 KB");
        assert_eq!(bytes(150.0 * 1024.0 * 1024.0), "150 MB");
        assert_eq!(cut("a-very-long-name-for-a-file-that-keeps-going.zip", 12), "a-very-long…");
    }
}
