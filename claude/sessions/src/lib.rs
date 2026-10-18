//! Claude Code: the sessions running on this PC, as cards, and a pill when one needs you or has finished.
//!
//! The island reads the files Claude Code keeps and tells the module what each session is doing (`claude`, every 2 s). The module
//! decides what a session is (waiting for you, working, finished, idle), draws a card for each one that is not idle (the ones that
//! need you first), and brings a pill when a session starts waiting for you or finishes a turn that took a while. It sees which
//! window is in front (`processes`), so it does not interrupt you with a session you are looking at.
//!
//! (How much of the Claude limits is used is another plugin, `claude-usage`: it reaches Anthropic, so it sees nothing of this PC.)

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// idle again after a turn, this recently: finished, and the result waits for you
const FINISHED_MS: u64 = 15 * 60 * 1000;
/// a turn that took no more than this is a quick answer, not news
const QUICK_MS: u64 = 5_000;
/// the most cards (the island lets a module have six)
const MAX_CARDS: usize = 6;
const TILE: &str = "#e08a5a";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Waiting,
    Busy,
    Idle,
}

#[derive(Default)]
struct State {
    /// the sessions, once each (the one that changed last)
    sessions: Vec<ClaudeSession>,
    /// a finished session that was looked at: its id -> the `updated` it was put away at. It comes back only if it finishes again.
    dismissed: HashMap<String, u64>,
    /// the briefs: session id -> (how it stood, when the busy stretch began)
    seen: HashMap<String, (Kind, u64)>,
    /// the window in front: (the program's file name, lower case, and its title, lower case)
    front: Option<(String, String)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn basename(p: &str) -> String {
    p.rsplit(['\\', '/']).find(|s| !s.is_empty()).unwrap_or(p).to_string()
}

fn project_of(s: &ClaudeSession) -> String {
    basename(&s.cwd)
}

/// how the registry says a session stands (no transcript is looked at)
fn kind_of(s: &ClaudeSession) -> Kind {
    if !s.waiting_for.is_empty() || s.status.contains("wait") {
        Kind::Waiting
    } else if s.status == "busy" {
        Kind::Busy
    } else {
        Kind::Idle
    }
}

/// "waiting", "working", "finished" or "idle"
fn state_of(s: &ClaudeSession, now: u64, dismissed: &HashMap<String, u64>) -> &'static str {
    match kind_of(s) {
        Kind::Waiting => "waiting",
        Kind::Busy => "working",
        Kind::Idle if s.model.is_some() && now.saturating_sub(s.updated) <= FINISHED_MS && dismissed.get(&s.id) != Some(&s.updated) => "finished",
        Kind::Idle => "idle",
    }
}

fn context_limit(model: &str) -> u64 {
    if model.to_lowercase().contains("haiku") {
        200_000
    } else {
        1_000_000
    }
}

/// "claude-sonnet-5-5" -> "Sonnet 5.5"; "claude-haiku-4-5-20251001" -> "Haiku 4.5"
fn model_name(model: &str) -> String {
    let rest = model.trim_start_matches("claude-");
    let mut parts = rest.split('-');
    let Some(family) = parts.next().filter(|f| !f.is_empty()) else { return String::new() };
    let mut name: String = family.chars().take(1).flat_map(char::to_uppercase).chain(family.chars().skip(1)).collect();
    let version: Vec<&str> = parts.filter(|p| p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit())).collect();
    if !version.is_empty() {
        name.push(' ');
        name.push_str(&version.join("."));
    }
    name
}

fn tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else {
        format!("{}k", (n as f64 / 1000.0).round() as u64)
    }
}

fn duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        "<1m".to_string()
    }
}

/// the colour of the ring (how full the context is)
fn fill_color(p: f32) -> &'static str {
    if p > 0.85 {
        "#e87a7a"
    } else if p > 0.65 {
        "#e8b04a"
    } else {
        "#5ac88c"
    }
}

fn used(s: &ClaudeSession) -> f32 {
    let limit = context_limit(s.model.as_deref().unwrap_or(""));
    (s.context_tokens as f32 / limit as f32).min(1.0)
}

/// what the session is doing, as one line with an icon
fn activity(s: &ClaudeSession, state: &str) -> (String, String) {
    match state {
        "waiting" => {
            let w = s.waiting_for.to_lowercase();
            let text = if w.contains("perm") || w.contains("approv") {
                "Waiting for your approval"
            } else if w.contains("input") || w.contains("answer") || w.contains("question") {
                "Waiting for your answer"
            } else {
                "Waiting for you"
            };
            ("phone".into(), text.into())
        }
        "working" => s.last.as_ref().map(|l| (l.icon.clone(), l.text.clone())).unwrap_or(("sparkle".into(), "Thinking".into())),
        "finished" => ("check".into(), "Finished".into()),
        _ => ("moon".into(), "Idle".into()),
    }
}

fn title_of(s: &ClaudeSession) -> String {
    s.title.clone().filter(|t| !t.is_empty()).or_else(|| Some(s.name.clone()).filter(|n| !n.is_empty())).unwrap_or_else(|| project_of(s))
}

fn short_id(id: &str) -> String {
    format!("s{}", id.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect::<String>())
}

fn card_of(s: &ClaudeSession, now: u64, state: &'static str) -> Card {
    let (icon, text) = activity(s, state);
    let frac = used(s);
    let limit = context_limit(s.model.as_deref().unwrap_or(""));
    let mut card = Card::new()
        .id(short_id(&s.id))
        .icon("sparkle")
        .rank(if matches!(state, "waiting" | "finished") { 1 } else { 30 })
        .title(title_of(s))
        .sub(text.clone())
        .live(icon.clone(), state)
        .tile_of(Tile::new("C", TILE).ring(frac, fill_color(frac)))
        .badges(vec![Badge::new(s.host.icon.clone(), format!("{}\n{}", s.host.label, project_of(s)))]);
    if let Some(p) = s.prompt.as_deref().filter(|p| !p.is_empty()) {
        card = card.labeled("Prompt", p);
    }
    // the latest tool calls: what it is doing (lit), and what it did
    let mut rows: Vec<(String, String, bool)> = Vec::new();
    if state == "working" {
        rows.push((icon, text, true));
        rows.extend(s.feed.iter().take(2).map(|f| (f.icon.clone(), f.text.clone(), false)));
    } else {
        rows.extend(s.feed.iter().take(3).map(|f| (f.icon.clone(), f.text.clone(), false)));
    }
    if !rows.is_empty() {
        card = card.lines(&rows);
    }
    card = card.meter("Context", frac, &format!("{} / {}", tokens(s.context_tokens), tokens(limit)), fill_color(frac));
    card = card.stats(&[
        (duration(now.saturating_sub(s.started) / 1000), "Running"),
        (format!("${:.2}", s.cost_usd), "Cost"),
        (format!("+{} \u{2212}{}", s.lines_added, s.lines_removed), "Lines"),
        (s.model.as_deref().map(model_name).filter(|m| !m.is_empty()).unwrap_or_else(|| "\u{2014}".to_string()), "Model"),
    ]);
    if matches!(state, "waiting" | "finished") {
        card = card.buttons(vec![Button::new("Focus", format!("focus:{}", s.id))]);
    }
    card
}

/// the cards: the ones that need you, then the ones that finished, then the others; the most recent first within each
fn cards_of(st: &State, now: u64) -> Vec<Card> {
    let mut list: Vec<(u8, u64, Card)> = Vec::new();
    for s in &st.sessions {
        let state = state_of(s, now, &st.dismissed);
        let rank = match state {
            "waiting" => 0,
            "finished" => 1,
            "working" => 2,
            _ => continue, // (idle ones are not shown)
        };
        list.push((rank, u64::MAX - s.updated, card_of(s, now, state)));
    }
    list.sort_by_key(|(r, u, _)| (*r, *u));
    list.into_iter().take(MAX_CARDS).map(|(_, _, c)| c).collect()
}

fn reply_cards(st: &State, now: u64) -> Reply {
    let mut cards = cards_of(st, now);
    if cards.is_empty() {
        return Reply::clear().more_cards(Vec::new());
    }
    let first = cards.remove(0);
    Reply::card(first).more_cards(cards)
}

/// Is the window that holds this session the one in front? Then you can see it already. An editor holds one window for each
/// folder: only the one that shows this project counts.
fn in_front(st: &State, s: &ClaudeSession) -> bool {
    let (Some(exe), Some((front_exe, title))) = (s.host.exe.as_deref(), st.front.as_ref()) else { return false };
    if basename(exe).to_lowercase() != *front_exe {
        return false;
    }
    s.host.icon == "term" || title.contains(&project_of(s).to_lowercase())
}

/// A pill for a session that needs you (it rings through do not disturb) or has finished.
fn brief(st: &State, s: &ClaudeSession, tone: &'static str) -> Option<Say> {
    if in_front(st, s) {
        return None;
    }
    let frac = used(s);
    Some(
        Say::new(title_of(s), if tone == "waiting" { 90 } else { 60 })
            .pill(s.host.icon.clone(), "")
            .keyed(s.id.clone(), tone, format!("focus:{}", s.id))
            .with_tile(Tile::new("C", TILE).ring(frac, fill_color(frac)), vec![Badge::new(s.host.icon.clone(), format!("{}\n{}", s.host.label, project_of(s)))])
            .ambient(),
    )
}

/// Watch the sessions go by: a pill when one starts waiting for you or finishes a turn that took a while, and the pill taken back
/// when you have answered.
fn alerts(st: &mut State, hub: bool, on: bool) -> (Vec<Say>, Vec<String>) {
    let (mut says, mut cancel) = (Vec::new(), Vec::new());
    if !on {
        st.seen.clear();
        return (says, cancel);
    }
    let sessions = st.sessions.clone();
    let mut present = Vec::new();
    for s in &sessions {
        present.push(s.id.clone());
        let kind = kind_of(s);
        let prev = st.seen.get(&s.id).copied();
        match (prev.map(|p| p.0), kind) {
            (Some(Kind::Waiting), k) if k != Kind::Waiting => cancel.push(s.id.clone()),
            (p, Kind::Waiting) if p != Some(Kind::Waiting) => {
                // (the hub shows the cards already)
                if !hub {
                    says.extend(brief(st, s, "waiting"));
                }
            }
            (Some(Kind::Busy), Kind::Idle) if s.updated.saturating_sub(prev.map_or(0, |p| p.1)) >= QUICK_MS => {
                if !hub {
                    says.extend(brief(st, s, "finished"));
                }
            }
            _ => {}
        }
        let busy_since = match prev {
            Some((Kind::Busy, t)) if kind == Kind::Busy => t,
            _ => s.updated,
        };
        st.seen.insert(s.id.clone(), (kind, busy_since));
    }
    st.seen.retain(|id, _| present.contains(id));
    (says, cancel)
}

/// the sessions once each: of two with the same id, the one that changed last
fn once_each(list: &[ClaudeSession]) -> Vec<ClaudeSession> {
    let mut best: Vec<ClaudeSession> = Vec::new();
    for s in list {
        match best.iter().position(|b| b.id == s.id) {
            Some(i) if s.updated > best[i].updated => best[i] = s.clone(),
            Some(_) => {}
            None => best.push(s.clone()),
        }
    }
    best
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        match ev.event.as_str() {
            "start" => {
                *st = State::default();
                Reply::nothing()
            }
            // (which window is in front)
            "processes" => {
                st.front = ev.windows.iter().find(|w| w.front).map(|w| (w.exe.to_lowercase(), w.title.to_lowercase()));
                Reply::nothing()
            }
            "claude" => {
                st.sessions = once_each(&ev.claude);
                let (says, cancel) = alerts(&mut st, ev.hub, ev.setting_bool("alerts", true));
                let mut reply = reply_cards(&st, ev.now);
                // the resting island (the bundle's `rests`): on while a session needs you, with who at the left
                let waiting = st.sessions.iter().filter(|s| state_of(s, ev.now, &st.dismissed) == "waiting").count();
                let needs = match waiting {
                    0 => Blocks::new(),
                    1 => Blocks::new().item("", "sparkle", "Needs you", ""),
                    n => Blocks::new().item("", "sparkle", &format!("{n} need you"), ""),
                };
                reply = reply.with_widget("needs", needs, Blocks::new()).rest(if waiting > 0 { &["needs"] } else { &[] });
                for s in says {
                    reply = reply.say_with(s);
                }
                if !cancel.is_empty() {
                    let keys: Vec<&str> = cancel.iter().map(String::as_str).collect();
                    reply = reply.cancel(&keys);
                }
                reply
            }
            "action" => {
                let Some(id) = ev.action.strip_prefix("focus:") else { return Reply::nothing() };
                let Some(s) = st.sessions.iter().find(|s| s.id == id).cloned() else { return Reply::nothing() };
                // (a finished session that was looked at: its card goes until it finishes something new)
                if state_of(&s, ev.now, &st.dismissed) == "finished" {
                    st.dismissed.insert(s.id.clone(), s.updated);
                }
                reply_cards(&st, ev.now).focus(s.host.exe.as_deref(), Some(&project_of(&s)))
            }
            _ => Reply::nothing(),
        }
    })
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000_000;

    fn session(id: &str, status: &str, updated: u64) -> ClaudeSession {
        ClaudeSession {
            id: id.into(),
            pid: 100,
            status: status.into(),
            started: T0 - 3_600_000,
            updated,
            cwd: "C:\\Work\\nadi".into(),
            title: Some("Fix the thing".into()),
            prompt: Some("please fix it".into()),
            model: Some("claude-sonnet-5-5".into()),
            context_tokens: 90_000,
            cost_usd: 1.5,
            lines_added: 12,
            lines_removed: 3,
            feed: vec![ClaudeItem { icon: "term".into(), text: "Running cargo build".into() }, ClaudeItem { icon: "pen".into(), text: "Editing lib.rs".into() }],
            last: Some(ClaudeItem { icon: "pen".into(), text: "Editing lib.rs".into() }),
            host: ClaudeHost { label: "VS Code".into(), icon: "code".into(), exe: Some("C:\\Program Files\\Microsoft VS Code\\Code.exe".into()) },
            ..ClaudeSession::default()
        }
    }

    fn look(now: u64, list: Vec<ClaudeSession>) -> Event {
        Event { event: "claude".into(), now, claude: list, settings: json!({}), ..Event::default() }
    }

    fn fresh() {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }

    #[test]
    fn a_working_session_has_a_card_with_what_it_does_and_an_idle_one_has_none() {
        fresh();
        let r = handle(&look(T0, vec![session("a", "busy", T0 - 1000), session("b", "idle", T0 - 5_000_000)])).to_json();
        let c = &r["card"];
        assert_eq!(c["title"], "Fix the thing");
        assert_eq!(c["tone"], "working");
        assert_eq!((c["sub"].as_str(), c["sub_icon"].as_str()), (Some("Editing lib.rs"), Some("pen")));
        assert_eq!(c["tile"]["text"], "C");
        assert_eq!(c["badges"][0]["icon"], "code");
        assert_eq!(c["badges"][0]["tip"], "VS Code\nnadi");
        let body = c["body"].as_array().unwrap();
        assert_eq!(body[0]["label"], "Prompt");
        // what it is doing is lit, then two of what it did
        assert_eq!(body[1]["rows"].as_array().unwrap().len(), 3);
        assert_eq!(body[1]["rows"][0]["now"], true);
        assert_eq!(body[2]["text"], "90k / 1.0M");
        assert_eq!(body[3]["items"][0], json!({ "value": "1h 0m", "label": "Running" }));
        assert_eq!(body[3]["items"][1]["value"], "$1.50");
        assert_eq!(body[3]["items"][3]["value"], "Sonnet 5.5");
        assert!(r["cards_more"].as_array().unwrap().is_empty());
        // nothing busy, nothing waiting: no card at all
        let r = handle(&look(T0, vec![session("b", "idle", T0 - 5_000_000)])).to_json();
        assert_eq!(r["card"], Value::Null);
    }

    #[test]
    fn the_ones_that_need_you_lead_then_the_finished_then_the_others() {
        fresh();
        let mut waiting = session("w", "busy", T0 - 9000);
        waiting.waiting_for = "permission".into();
        waiting.title = Some("Needs you".into());
        let finished = session("f", "idle", T0 - 60_000);
        let mut working = session("k", "busy", T0 - 1000);
        working.title = Some("Busy".into());
        let r = handle(&look(T0, vec![working, finished, waiting])).to_json();
        assert_eq!(r["card"]["title"], "Needs you");
        assert_eq!(r["card"]["sub"], "Waiting for your approval");
        assert_eq!(r["card"]["body"].as_array().unwrap().last().unwrap()["items"][0]["action"], "focus:w");
        assert_eq!(r["cards_more"][0]["tone"], "finished");
        assert_eq!(r["cards_more"][1]["tone"], "working");
        assert_eq!(r["cards_more"][0]["id"], "sf");
    }

    #[test]
    fn a_finished_session_goes_when_you_have_looked_at_it_and_comes_back_when_it_finishes_again() {
        fresh();
        let s = session("f", "idle", T0 - 60_000);
        handle(&look(T0, vec![s.clone()]));
        let r = handle(&Event { event: "action".into(), action: "focus:f".into(), now: T0, ..Event::default() }).to_json();
        assert_eq!(r["card"], Value::Null);
        assert_eq!(r["focus"]["title"], "nadi");
        assert!(r["focus"]["exe"].as_str().unwrap().ends_with("Code.exe"));
        assert_eq!(handle(&look(T0 + 2000, vec![s])).to_json()["card"], Value::Null);
        let again = session("f", "idle", T0 + 10_000);
        assert_eq!(handle(&look(T0 + 12_000, vec![again])).to_json()["card"]["tone"], "finished");
        // and it is idle after a quarter of an hour
        assert_eq!(handle(&look(T0 + 10_000 + FINISHED_MS + 1, vec![session("f", "idle", T0 + 10_000)])).to_json()["card"], Value::Null);
    }

    #[test]
    fn a_session_that_starts_waiting_brings_a_pill_and_answering_it_takes_the_pill_back() {
        fresh();
        let mut s = session("w", "busy", T0);
        handle(&look(T0, vec![s.clone()]));
        s.waiting_for = "permission".into();
        s.updated = T0 + 4000;
        let r = handle(&look(T0 + 5000, vec![s.clone()])).to_json();
        let say = &r["say"][0];
        assert_eq!(say["text"], "Fix the thing");
        assert_eq!(say["priority"], 90);
        assert_eq!(say["ambient"], true);
        assert_eq!(say["pill"]["key"], "w");
        assert_eq!(say["pill"]["tone"], "waiting");
        assert_eq!(say["pill"]["press"], "focus:w");
        assert_eq!(say["pill"]["tile"]["text"], "C");
        // told again while it waits: no second pill
        assert_eq!(handle(&look(T0 + 7000, vec![s.clone()])).to_json()["say"], json!([]));
        // answered: the pill is taken back
        s.waiting_for.clear();
        s.updated = T0 + 9000;
        let r = handle(&look(T0 + 10_000, vec![s])).to_json();
        assert_eq!(r["cancel"], json!(["w"]));
    }

    #[test]
    fn a_turn_that_took_a_while_brings_a_finished_pill_and_a_quick_one_does_not() {
        fresh();
        let busy = session("a", "busy", T0);
        handle(&look(T0, vec![busy.clone()]));
        let r = handle(&look(T0 + 12_000, vec![session("a", "idle", T0 + 11_000)])).to_json();
        assert_eq!(r["say"][0]["pill"]["tone"], "finished");
        assert_eq!(r["say"][0]["priority"], 60);
        // a quick answer
        fresh();
        handle(&look(T0, vec![session("a", "busy", T0)]));
        let r = handle(&look(T0 + 4000, vec![session("a", "idle", T0 + 3000)])).to_json();
        assert_eq!(r["say"], json!([]));
    }

    #[test]
    fn no_pill_over_the_hub_with_the_setting_off_or_for_the_window_in_front() {
        let waiting = |updated: u64| {
            let mut s = session("w", "busy", updated);
            s.waiting_for = "input".into();
            s
        };
        // over the hub
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        let mut ev = look(T0 + 2000, vec![waiting(T0 + 1000)]);
        ev.hub = true;
        assert_eq!(handle(&ev).to_json()["say"], json!([]));
        // the setting off
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        let mut ev = look(T0 + 2000, vec![waiting(T0 + 1000)]);
        ev.settings = json!({ "alerts": false });
        assert_eq!(handle(&ev).to_json()["say"], json!([]));
        // its window in front, on its project
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        handle(&Event { event: "processes".into(), windows: vec![Window { exe: "code.exe".into(), title: "lib.rs - nadi - Visual Studio Code".into(), front: true, ..Window::default() }], ..Event::default() });
        assert_eq!(handle(&look(T0 + 2000, vec![waiting(T0 + 1000)])).to_json()["say"], json!([]));
        // ... but another folder of the same editor is not it
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        handle(&Event { event: "processes".into(), windows: vec![Window { exe: "code.exe".into(), title: "other - Visual Studio Code".into(), front: true, ..Window::default() }], ..Event::default() });
        assert_eq!(handle(&look(T0 + 2000, vec![waiting(T0 + 1000)])).to_json()["say"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_session_with_two_processes_is_one_card() {
        fresh();
        let a = session("a", "busy", T0 - 5000);
        let b = session("a", "busy", T0 - 1000);
        let r = handle(&look(T0, vec![a, b])).to_json();
        assert!(r["cards_more"].as_array().unwrap().is_empty());
    }

    #[test]
    fn models_have_names_and_the_context_a_size() {
        assert_eq!(model_name("claude-sonnet-5-5"), "Sonnet 5.5");
        assert_eq!(model_name("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(model_name("claude-fable-5-1"), "Fable 5.1");
        assert_eq!(model_name(""), "");
        assert_eq!((context_limit("claude-haiku-4-5"), context_limit("claude-sonnet-5-5")), (200_000, 1_000_000));
    }
}
