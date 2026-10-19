//! Claude Code: the sessions running on this PC, as cards, and a pill when one needs you or has finished.
//!
//! The island reads the files Claude Code keeps and tells the module what each session is doing (`claude`, every 2 s), for every
//! session whose process is still there. The module decides what a session is (waiting for you, working, finished, idle) and draws
//! a card for each one, the ones that need you first. A card stays as long as its session runs: it goes only when its source is
//! gone (the editor was closed, the terminal exited), never because it was looked at or has been quiet. The Claude desktop app has a
//! card too while it runs. The module also brings a banner when a session changes state: it starts working, needs you (with Allow and
//! Deny on it, when answering is set up), wants a reply, is done, stopped, went quiet, opened or closed. Each moment can be switched
//! off; several at once are one banner. It sees which window is in front (`processes`), so it does not interrupt you with a session
//! you are looking at.
//!
//! (How much of the Claude limits is used is another plugin, `claude-usage`: it reaches Anthropic, so it sees nothing of this PC.)

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// idle again after a turn, this recently: finished, and the result waits for you
const FINISHED_MS: u64 = 15 * 60 * 1000;
/// a session that works this long has started: a quick answer is not news
const STARTED_MS: u64 = 3_000;
/// a request for a permission that the island does not hold is said after this long (the island holds it a moment after the session
/// says it waits: with the buttons, it is better to wait for them)
const ASK_MS: u64 = 2_500;
/// idle this long after a turn: gone quiet
const QUIET_MS: u64 = 5 * 60 * 1000;
/// the most cards (the island lets a module have this many)
const MAX_CARDS: usize = 24;
const TILE: &str = "#d97757";
const AMBER: &str = "#e8b04a";
const GREEN: &str = "#5ac88c";
const RED: &str = "#e87a7a";
const GREY: &str = "#6d6d75";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Waiting,
    Busy,
    Idle,
}

/// What the banners know of a session from the last look.
#[derive(Default)]
struct Seen {
    kind: Option<Kind>,
    /// when its turn began (ms), kept while it works and waits
    since: u64,
    /// how many errors it had when the turn began
    errors: u32,
    /// "started working" has been said for this turn
    started: bool,
    /// "needs you" has been said for this wait
    asked: bool,
    /// "gone quiet" has been said for this rest
    quiet: bool,
    /// what it was, for when it is gone
    last: Option<ClaudeSession>,
}

#[derive(Default)]
struct State {
    /// the sessions, once each (the one that changed last)
    sessions: Vec<ClaudeSession>,
    /// what the banners know of each session
    seen: HashMap<String, Seen>,
    /// the first look has been had: what is there already is not news
    primed: bool,
    /// the window in front: (the program's file name, lower case, and its title, lower case)
    front: Option<(String, String)>,
    /// the Claude desktop app, while it runs: (its program, the title of its window)
    desktop: Option<(String, String)>,
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
fn state_of(s: &ClaudeSession, now: u64) -> &'static str {
    match kind_of(s) {
        Kind::Waiting => "waiting",
        Kind::Busy => "working",
        Kind::Idle if s.model.is_some() && now.saturating_sub(s.updated) <= FINISHED_MS => "finished",
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

/// "2m 14s", "45s", "1h 03m"
fn clock_dur(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// "just now", "3 min ago", "2h ago"
fn ago(ms: u64) -> String {
    let m = ms / 60_000;
    if m < 1 {
        "just now".to_string()
    } else if m < 60 {
        format!("{m} min ago")
    } else {
        format!("{}h ago", m / 60)
    }
}

/// how the session is set to ask, as a chip: `(icon, text, tone)`; a mode that holds back (plan) or lets things through (auto) is lit
fn mode_chip(mode: &str) -> Option<(&'static str, String, &'static str)> {
    match mode {
        "" => None,
        "plan" => Some(("shield", "Plan".into(), "hot")),
        "auto" | "acceptEdits" | "accept-edits" | "autoAccept" => Some(("shield", "Auto-accept".into(), "hot")),
        "bypassPermissions" | "bypass" => Some(("shield", "Bypass".into(), "hot")),
        "normal" | "default" => Some(("shield", "Default".into(), "")),
        other => Some(("shield", other.chars().take(1).flat_map(char::to_uppercase).chain(other.chars().skip(1)).collect(), "")),
    }
}

/// what it asks to do, as the first words of the strip: "Wants to run", "Wants to change"
fn wants(tool: &str) -> &'static str {
    match tool {
        "Bash" | "PowerShell" => "Wants to run",
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => "Wants to change",
        "WebFetch" | "WebSearch" => "Wants to look up",
        _ => "Wants to use",
    }
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
    // the chips: where it works, how it is set to ask, which model and how hard it thinks
    let mut chips: Vec<(&str, String, &str)> = Vec::new();
    if !s.branch.is_empty() {
        chips.push(("branch", s.branch.clone(), ""));
    }
    chips.extend(mode_chip(&s.mode));
    if let Some(m) = s.model.as_deref().map(model_name).filter(|m| !m.is_empty()) {
        chips.push(("sparkle", if s.effort.is_empty() { m } else { format!("{m} \u{b7} {}", s.effort) }, ""));
    }
    if !chips.is_empty() {
        card = card.chips(&chips);
    }
    let turn = if s.turn_started > 0 { now.saturating_sub(s.turn_started) / 1000 } else { 0 };
    let plan: Vec<(&str, String)> = s.todos.iter().take(8).map(|t| (t.status.as_str(), t.text.clone())).collect();
    let ctx = format!("{} / {}", tokens(s.context_tokens), tokens(limit));
    match state {
        "working" => {
            let thought = if s.thinking_ms >= 1000 { format!("Thought {}s", s.thinking_ms / 1000) } else { String::new() };
            card = card.strip(if turn > 0 { format!("Working for {}", clock_dur(turn)) } else { "Working".to_string() }, thought, "working");
            if !plan.is_empty() {
                card = card.checklist("Plan", &plan);
            }
            // the latest tool calls: what it is doing (lit), and what it did
            let mut rows: Vec<(String, String, bool)> = vec![(icon, text, true)];
            rows.extend(s.feed.iter().take(2).map(|f| (f.icon.clone(), f.text.clone(), false)));
            card = card.lines(&rows);
            card = card.stats(&[
                (s.files.len().to_string(), "Files"),
                (s.commands.to_string(), "Commands"),
                (s.agents.to_string(), "Agents"),
                (format!("${:.2}", s.cost_usd), "Cost"),
            ]);
            card = card.meter("Context", frac, &ctx, fill_color(frac));
        }
        "waiting" => {
            // what it asks for: the call that has no result yet (and, when the island holds the request, the way to answer it)
            let ask = s.asking.as_ref().filter(|a| a.kind == "permission");
            let (tool, detail, call_icon) = match (ask, &s.pending) {
                (Some(a), _) => (a.tool.as_str(), a.detail.clone(), a.icon.as_str()),
                (None, Some(p)) => (p.tool.as_str(), p.detail.clone(), p.icon.as_str()),
                (None, None) => ("", String::new(), "term"),
            };
            if !detail.is_empty() || ask.is_some() {
                let waited = ask.map_or_else(|| now.saturating_sub(s.updated), |a| a.age_ms) / 1000;
                let side = if waited >= 5 { format!("waiting {}", clock_dur(waited)) } else { String::new() };
                match ask {
                    Some(a) => {
                        card = card.ask(
                            wants(tool),
                            &side,
                            call_icon,
                            detail,
                            &[("Allow", &format!("allow:{}", a.id), "ok"), ("Deny", &format!("deny:{}", a.id), "no")],
                            "Tell Claude what to do instead\u{2026}",
                            &format!("denywith:{}", a.id),
                        )
                    }
                    None => card = card.ask(wants(tool), &side, call_icon, detail, &[], "", ""),
                }
            }
            if !plan.is_empty() {
                card = card.checklist("Plan", &plan);
            }
            card = card.stats(&[
                (s.files.len().to_string(), "Files"),
                (s.commands.to_string(), "Commands"),
                (s.agents.to_string(), "Agents"),
                (format!("${:.2}", s.cost_usd), "Cost"),
            ]);
        }
        "finished" => {
            let took = if s.turn_started > 0 { clock_dur(s.updated.saturating_sub(s.turn_started) / 1000) } else { String::new() };
            card = card.strip(if took.is_empty() { "Done".to_string() } else { format!("Done in {took}") }, ago(now.saturating_sub(s.updated)), "finished");
            if !s.reply.is_empty() {
                card = card.labeled("Result", s.reply.clone());
            }
            let changed: Vec<(String, u64, u64)> = s.files.iter().take(6).map(|f| (f.name.clone(), f.added, f.removed)).collect();
            if !changed.is_empty() {
                card = card.files(&format!("Changed \u{b7} {}", s.files.len()), &changed);
            }
            card = card.stats(&[
                (format!("+{}", s.lines_added), "Added"),
                (format!("\u{2212}{}", s.lines_removed), "Removed"),
                (s.errors.to_string(), "Errors"),
                (format!("${:.2}", s.cost_usd), "Cost"),
            ]);
            card = card.meter("Context", frac, &ctx, fill_color(frac));
            // (a turn that is held open for a reply)
            if let Some(a) = s.asking.as_ref().filter(|a| a.kind == "stop") {
                card = card.field("Reply to Claude\u{2026}", "while the turn is open", &format!("reply:{}", a.id));
            }
        }
        _ => {
            card = card.strip("Quiet", format!("last {}", ago(now.saturating_sub(s.updated))), "idle");
            card = card.meter("Context", frac, &ctx, fill_color(frac));
        }
    }
    // (it stays while it runs: its window is one press on its circle away)
    card.focus_action(format!("focus:{}", s.id))
}

/// the card of the Claude desktop app, which runs (a chat has no session of its own to tell about)
fn desktop_card(front: bool) -> Card {
    Card::new()
        .id("desktop")
        .icon("sparkle")
        .rank(40)
        .title("Claude Desktop")
        .sub(if front { "In front" } else { "Running" })
        .live("sparkle", "idle")
        .tile_of(Tile::new("C", TILE))
        .badges(vec![Badge::new("sparkle", "Claude Desktop")])
        .focus_action("focus:desktop")
}

/// the cards: one for each session that runs (the ones that need you first, then the ones that finished, then the working ones, then the
/// idle ones; the most recent first within each), and the desktop app last
fn cards_of(st: &State, now: u64) -> Vec<Card> {
    let mut list: Vec<(u8, u64, Card)> = Vec::new();
    for s in &st.sessions {
        let state = state_of(s, now);
        let rank = match state {
            "waiting" => 0,
            "finished" => 1,
            "working" => 2,
            _ => 3,
        };
        list.push((rank, u64::MAX - s.updated, card_of(s, now, state)));
    }
    list.sort_by_key(|(r, u, _)| (*r, *u));
    let mut cards: Vec<Card> = list.into_iter().map(|(_, _, c)| c).collect();
    cards.truncate(MAX_CARDS - usize::from(st.desktop.is_some()));
    if let Some((exe, title)) = &st.desktop {
        let front = st.front.as_ref().is_some_and(|(f, t)| basename(exe).to_lowercase() == *f && *t == title.to_lowercase());
        cards.push(desktop_card(front));
    }
    cards
}

fn reply_cards(st: &State, now: u64) -> Reply {
    let mut cards = cards_of(st, now);
    if cards.is_empty() {
        return Reply::clear().more_cards(Vec::new());
    }
    // (what its tile says: how many need you, else how many there are)
    let waiting = st.sessions.iter().filter(|s| state_of(s, now) == "waiting").count();
    let (big, caption) = if waiting > 0 { (waiting, "needs you") } else if cards.len() == 1 { (1, "session") } else { (cards.len(), "sessions") };
    let first = cards.remove(0).glance(big.to_string(), caption);
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

/// What the person asked of the banners (the settings).
struct Cfg {
    on: bool,
    start: bool,
    needs: bool,
    done: bool,
    stop: bool,
    quiet: bool,
    open: bool,
    /// a turn that took less than this is not announced, ms
    skip_ms: u64,
}

impl Cfg {
    fn of(ev: &Event) -> Cfg {
        Cfg {
            on: ev.setting_bool("alerts", true),
            start: ev.setting_bool("b_start", true),
            needs: ev.setting_bool("b_needs", true),
            done: ev.setting_bool("b_done", true),
            stop: ev.setting_bool("b_stop", true),
            quiet: ev.setting_bool("b_quiet", false),
            open: ev.setting_bool("b_open", false),
            skip_ms: ev.settings["skip"].as_u64().unwrap_or(10) * 1000,
        }
    }
}

/// One thing that happened to a session, to be said in a banner.
struct Moment {
    id: String,
    title: String,
    /// "working", "finished", "stopped", "quiet", "info" or "closed"
    tone: &'static str,
    sub: String,
    ms: u64,
    prio: i32,
}

impl Moment {
    fn new(s: &ClaudeSession, tone: &'static str, sub: String, ms: u64, prio: i32) -> Moment {
        Moment { id: s.id.clone(), title: title_of(s), tone, sub, ms, prio }
    }
}

/// the circle of a banner: its colour is the state, and the ring around it is full unless the session works (then a part of it
/// sweeps round) or does nothing (then there is none)
fn tile_for(tone: &str) -> Tile {
    let (color, ring) = match tone {
        "working" => (TILE, Some(0.25)),
        "permission" | "reply" | "waiting" => (AMBER, Some(1.0)),
        "finished" => (GREEN, Some(1.0)),
        "stopped" => (RED, Some(1.0)),
        _ => (GREY, None),
    };
    match ring {
        Some(r) => Tile::new("C", color).ring(r, color),
        None => Tile::new("C", color),
    }
}

/// a banner: the circle, the title, what changed under it, and the state (or buttons that answer) at the right
fn banner(key: &str, title: String, tone: &'static str, sub: String, ms: u64, prio: i32, press: String, buttons: &[(&str, &str, &str)]) -> Say {
    let mut say = Say::new(title, prio).pill("sparkle", "").keyed(key, tone, press).with_sub(sub).with_tile(tile_for(tone), Vec::new()).for_ms(ms).ambient();
    if !buttons.is_empty() {
        say = say.with_buttons(buttons);
    }
    say
}

fn say_of(m: &Moment) -> Say {
    banner(&m.id, m.title.clone(), m.tone, m.sub.clone(), m.ms, m.prio, format!("focus:{}", m.id), &[])
}

fn clip(text: &str, n: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= n {
        text
    } else {
        format!("{}\u{2026}", text.chars().take(n).collect::<String>().trim_end())
    }
}

/// a session waits on you: for a permission (with Allow and Deny when the island holds the request), or for a few words
fn needs_you(s: &ClaudeSession, ask: Option<&ClaudeAsking>) -> Say {
    let id = s.id.as_str();
    let permission = ask.is_some() || s.waiting_for == "permission";
    let call = match (ask, &s.pending) {
        (Some(a), _) => Some((a.tool.as_str(), a.detail.as_str())),
        (None, Some(p)) => Some((p.tool.as_str(), p.detail.as_str())),
        (None, None) => None,
    };
    if permission {
        let sub = match call {
            Some((tool, detail)) if !detail.is_empty() => format!("{} {}", wants(tool), clip(detail, 80)),
            _ => "Needs your answer".to_string(),
        };
        return match ask {
            Some(a) => banner(id, title_of(s), "permission", sub, 12_000, 90, format!("focus:{id}"), &[("Allow", &format!("allow:{}", a.id), "ok"), ("Deny", &format!("deny:{}", a.id), "no")]),
            None => banner(id, title_of(s), "permission", sub, 8_000, 90, format!("focus:{id}"), &[]),
        };
    }
    let sub = match call {
        Some((_, detail)) if !detail.is_empty() => format!("Asks: {}", clip(detail, 80)),
        _ => "Wants a reply".to_string(),
    };
    banner(id, title_of(s), "reply", sub, 12_000, 90, format!("focus:{id}"), &[])
}

/// a turn that is held open for you to reply (the Stop hook): what it said
fn held_reply(s: &ClaudeSession) -> Say {
    let sub = if s.reply.is_empty() { "Done \u{b7} wants a reply".to_string() } else { format!("Says: {}", clip(&s.reply, 80)) };
    banner(&s.id, title_of(s), "reply", sub, 12_000, 90, format!("focus:{}", s.id), &[])
}

/// "2 done \u{b7} 1 started working": what several sessions did at once, as one banner
fn merged(moments: &[Moment]) -> Say {
    let count = |tone: &str| moments.iter().filter(|m| m.tone == tone).count();
    let mut parts = Vec::new();
    for (tone, word) in [("stopped", "stopped"), ("finished", "done"), ("working", "started working"), ("quiet", "quiet"), ("info", "new"), ("closed", "closed")] {
        let n = count(tone);
        if n > 0 {
            parts.push(format!("{n} {word}"));
        }
    }
    let tone = ["stopped", "finished", "working", "info", "quiet", "closed"].into_iter().find(|t| count(t) > 0).unwrap_or("info");
    let first = &moments[0];
    banner(
        "claude-many",
        format!("{} sessions", moments.len()),
        tone,
        parts.join(" \u{b7} "),
        moments.iter().map(|m| m.ms).max().unwrap_or(5000),
        moments.iter().map(|m| m.prio).max().unwrap_or(40),
        format!("focus:{}", first.id),
        &[],
    )
}

/// when the turn of a session began
fn turn_start(s: &ClaudeSession) -> u64 {
    if s.turn_started > 0 {
        s.turn_started
    } else {
        s.updated
    }
}

/// Watch the sessions go by, and say what changed: it starts working, needs you, is done, stopped, went quiet, opened or closed
/// (each of which can be switched off). Several things at once are one banner; what needs you never is, its buttons are its own.
/// What was said of a session that waited is taken back when it has been answered.
fn alerts(st: &mut State, now: u64, hub: bool, cfg: &Cfg) -> (Vec<Say>, Vec<String>) {
    let (mut says, mut cancel) = (Vec::new(), Vec::new());
    if !cfg.on {
        st.seen.clear();
        st.primed = false;
        return (says, cancel);
    }
    let sessions = st.sessions.clone();
    // (the hub shows the cards already; what is there at the first look is not news)
    let announce = st.primed && !hub;
    let mut moments: Vec<Moment> = Vec::new();
    let mut present = Vec::new();
    for s in &sessions {
        present.push(s.id.clone());
        let kind = kind_of(s);
        let mut seen = st.seen.remove(&s.id).unwrap_or_default();
        let prev = seen.kind;
        let visible = announce && !in_front(st, s);
        if prev.is_none() {
            seen.since = turn_start(s);
            seen.errors = s.errors;
            seen.started = true;
            seen.quiet = true;
            if cfg.open && visible {
                moments.push(Moment::new(s, "info", format!("New session \u{b7} {}", s.host.label), 3000, 30));
            }
        }
        if prev == Some(Kind::Waiting) && kind != Kind::Waiting {
            cancel.push(s.id.clone());
            seen.asked = false;
        }
        // a new turn
        if kind != Kind::Idle && prev == Some(Kind::Idle) {
            seen.since = turn_start(s);
            seen.errors = s.errors;
            seen.started = false;
            seen.quiet = false;
        }
        match kind {
            Kind::Waiting => {
                let ask = s.asking.as_ref().filter(|a| a.kind == "permission");
                if !seen.asked && (ask.is_some() || now.saturating_sub(s.updated) >= ASK_MS) {
                    seen.asked = true;
                    if cfg.needs && visible {
                        says.push(needs_you(s, ask));
                    }
                }
            }
            Kind::Busy => {
                if !seen.started && now.saturating_sub(seen.since) >= STARTED_MS {
                    seen.started = true;
                    if cfg.start && visible {
                        moments.push(Moment::new(s, "working", format!("Started working \u{b7} {}", s.host.label), 3000, 40));
                    }
                }
            }
            Kind::Idle => {
                if matches!(prev, Some(Kind::Busy | Kind::Waiting)) {
                    // its turn ended
                    let took = if s.turn_started > 0 && s.turn_started <= s.updated { s.updated - s.turn_started } else { s.updated.saturating_sub(seen.since) };
                    seen.quiet = false;
                    if took >= cfg.skip_ms && visible {
                        let errors = s.errors.saturating_sub(seen.errors);
                        let held = s.asking.as_ref().is_some_and(|a| a.kind == "stop");
                        if held {
                            if cfg.needs {
                                says.push(held_reply(s));
                            }
                        } else if errors > 0 && cfg.stop {
                            let sub = format!("Stopped \u{b7} {errors} error{}", if errors == 1 { "" } else { "s" });
                            moments.push(Moment::new(s, "stopped", sub, 6000, 60));
                        } else if cfg.done {
                            let files = s.files.len();
                            let mut sub = format!("Done in {}", clock_dur(took / 1000));
                            if files > 0 {
                                sub.push_str(&format!(" \u{b7} {files} file{} changed", if files == 1 { "" } else { "s" }));
                            }
                            moments.push(Moment::new(s, "finished", sub, 5000, 60));
                        }
                    }
                } else if !seen.quiet && now.saturating_sub(s.updated) >= QUIET_MS {
                    seen.quiet = true;
                    if cfg.quiet && visible && s.model.is_some() {
                        moments.push(Moment::new(s, "quiet", format!("Quiet for {} min", now.saturating_sub(s.updated) / 60_000), 3000, 30));
                    }
                }
            }
        }
        seen.kind = Some(kind);
        seen.last = Some(s.clone());
        st.seen.insert(s.id.clone(), seen);
    }
    // the sessions that are gone
    let gone: Vec<String> = st.seen.keys().filter(|id| !present.contains(id)).cloned().collect();
    for id in gone {
        let Some(seen) = st.seen.remove(&id) else { continue };
        if seen.kind == Some(Kind::Waiting) {
            cancel.push(id.clone());
        }
        if let (true, true, Some(last)) = (cfg.open, announce, seen.last.as_ref()) {
            moments.push(Moment::new(last, "closed", "Session closed".to_string(), 3000, 30));
        }
    }
    match moments.len() {
        0 => {}
        1 => says.push(say_of(&moments[0])),
        _ => says.push(merged(&moments)),
    }
    st.primed = true;
    (says, cancel)
}

/// Is this window the Claude desktop app's? (`claude.exe` with a window: the command line tool has none of its own, and it lives in a
/// `.local` or a `.claude` folder)
fn is_desktop(w: &Window) -> bool {
    let path = w.path.to_lowercase();
    w.exe == "claude.exe" && !w.tool && !path.contains(".claude") && !path.contains("\\.local\\")
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
            // (which window is in front, and whether the desktop app runs: its card is there while it does)
            "processes" => {
                st.front = ev.windows.iter().find(|w| w.front).map(|w| (w.exe.to_lowercase(), w.title.to_lowercase()));
                let before = st.desktop.clone();
                st.desktop = ev.windows.iter().find(|w| is_desktop(w)).map(|w| (if w.path.is_empty() { w.exe.clone() } else { w.path.clone() }, w.title.clone()));
                if before == st.desktop {
                    return Reply::nothing();
                }
                reply_cards(&st, ev.now)
            }
            "claude" => {
                st.sessions = once_each(&ev.claude);
                let (says, cancel) = alerts(&mut st, ev.now, ev.hub, &Cfg::of(ev));
                let mut reply = reply_cards(&st, ev.now);
                // the resting island (the bundle's `rests`): on while a session needs you, with who at the left
                let waiting = st.sessions.iter().filter(|s| state_of(s, ev.now) == "waiting").count();
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
                // answering what a session asks (the island holds the request; the hooks of Claude Code wait on it)
                let said = ev.args["text"].as_str().unwrap_or("").to_string();
                if let Some(id) = ev.action.strip_prefix("allow:") {
                    return Reply::nothing().answer(id, "allow", "");
                }
                if let Some(id) = ev.action.strip_prefix("deny:") {
                    return Reply::nothing().answer(id, "deny", "");
                }
                if let Some(id) = ev.action.strip_prefix("denywith:") {
                    return Reply::nothing().answer(id, "deny", said);
                }
                if let Some(id) = ev.action.strip_prefix("reply:") {
                    return Reply::nothing().answer(id, "reply", said);
                }
                // (set up, or take away, what lets the island answer: the person pressed the button for it)
                let wait = ev.settings["wait"].as_u64().unwrap_or(20);
                match ev.action.as_str() {
                    "answer_on" => return Reply::nothing().claude_hooks("install", true, ev.setting_bool("replies", false), wait),
                    "answer_off" => return Reply::nothing().claude_hooks("remove", true, true, wait),
                    _ => {}
                }
                let Some(id) = ev.action.strip_prefix("focus:") else { return Reply::nothing() };
                if id == "desktop" {
                    let Some((exe, _)) = st.desktop.clone() else { return Reply::nothing() };
                    return reply_cards(&st, ev.now).focus(Some(&exe), None);
                }
                let Some(s) = st.sessions.iter().find(|s| s.id == id).cloned() else { return Reply::nothing() };
                // (the card stays: it goes when the session does)
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

    /// the body of a card by the kind of its blocks
    fn blocks<'a>(card: &'a Value) -> Vec<(&'a str, &'a Value)> {
        card["body"].as_array().unwrap().iter().map(|b| (b["kind"].as_str().unwrap(), b)).collect()
    }

    fn rich(mut s: ClaudeSession) -> ClaudeSession {
        s.branch = "fix/oauth".into();
        s.mode = "plan".into();
        s.effort = "high".into();
        s.turn_started = T0 - 134_000;
        s.thinking_ms = 12_000;
        s.todos = vec![
            ClaudeTodo { status: "done".into(), text: "Read the sensor".into() },
            ClaudeTodo { status: "now".into(), text: "Show every session".into() },
            ClaudeTodo { status: "next".into(), text: "Tests".into() },
        ];
        s.files = vec![ClaudeFile { name: "lib.rs".into(), added: 84, removed: 31 }, ClaudeFile { name: "hub.css".into(), added: 142, removed: 58 }];
        s.commands = 12;
        s.agents = 1;
        s.reply = "The tiles are half the size now.".into();
        s
    }

    #[test]
    fn a_working_session_has_a_card_with_what_it_does() {
        fresh();
        let r = handle(&look(T0, vec![rich(session("a", "busy", T0 - 1000)), session("b", "idle", T0 - 5_000_000)])).to_json();
        let c = &r["card"];
        assert_eq!(c["title"], "Fix the thing");
        assert_eq!(c["tone"], "working");
        assert_eq!((c["sub"].as_str(), c["sub_icon"].as_str()), (Some("Editing lib.rs"), Some("pen")));
        assert_eq!(c["tile"]["text"], "C");
        assert_eq!(c["badges"][0]["icon"], "code");
        assert_eq!(c["badges"][0]["tip"], "VS Code\nnadi");
        assert_eq!(c["focus"], "focus:a");
        let b = blocks(c);
        // the chips: the branch, how it asks (plan is lit), the model and how hard it thinks
        assert_eq!(b[0].0, "chips");
        assert_eq!(b[0].1["items"][0]["text"], "fix/oauth");
        assert_eq!((b[0].1["items"][1]["text"].as_str(), b[0].1["items"][1]["tone"].as_str()), (Some("Plan"), Some("hot")));
        assert_eq!(b[0].1["items"][2]["text"], "Sonnet 5.5 \u{b7} high");
        // how long this turn has taken, and how long it thought
        assert_eq!((b[1].0, b[1].1["text"].as_str(), b[1].1["side"].as_str(), b[1].1["tone"].as_str()), ("strip", Some("Working for 2m 14s"), Some("Thought 12s"), Some("working")));
        // the plan, with its step lit, and what it is doing (lit), then two of what it did
        assert_eq!(b[2].0, "checklist");
        assert_eq!(b[2].1["items"][1]["state"], "now");
        assert_eq!(b[3].0, "lines");
        assert_eq!(b[3].1["rows"].as_array().unwrap().len(), 3);
        assert_eq!(b[3].1["rows"][0]["now"], true);
        assert_eq!(b[4].1["items"][0], json!({ "value": "2", "label": "Files" }));
        assert_eq!(b[4].1["items"][1], json!({ "value": "12", "label": "Commands" }));
        assert_eq!(b[5].1["text"], "90k / 1.0M");
        // the idle one is there too, small
        assert_eq!(r["cards_more"][0]["tone"], "idle");
        let idle = blocks(&r["cards_more"][0]);
        assert_eq!(idle.last().unwrap().0, "meter");
        assert_eq!(idle[idle.len() - 2].1["text"], "Quiet");
        // nothing busy, nothing waiting: the session is idle, and it has its card all the same
        let r = handle(&look(T0, vec![session("b", "idle", T0 - 5_000_000)])).to_json();
        assert_eq!(r["card"]["tone"], "idle");
        assert_eq!(r["card"]["sub"], "Idle");
    }

    #[test]
    fn a_session_that_waits_shows_what_it_asks_and_how_to_answer() {
        fresh();
        let mut w = rich(session("w", "busy", T0 - 41_000));
        w.waiting_for = "permission".into();
        w.pending = Some(ClaudePending { tool: "Bash".into(), icon: "term".into(), detail: "cargo build --release".into() });
        // (the island does not hold the request: what it asks is shown, and nothing to press)
        let r = handle(&look(T0, vec![w.clone()])).to_json();
        let b = blocks(&r["card"]);
        let ask = b.iter().find(|x| x.0 == "ask").unwrap().1;
        assert_eq!((ask["label"].as_str(), ask["text"].as_str(), ask["side"].as_str()), (Some("Wants to run"), Some("cargo build --release"), Some("waiting 41s")));
        assert!(ask["buttons"].as_array().unwrap().is_empty());
        // (the island holds it: Allow, Deny, and a few words)
        w.asking = Some(ClaudeAsking { id: "r1".into(), kind: "permission".into(), tool: "Bash".into(), icon: "term".into(), detail: "cargo build --release".into(), age_ms: 41_000 });
        let r = handle(&look(T0, vec![w])).to_json();
        let ask = blocks(&r["card"]).into_iter().find(|x| x.0 == "ask").unwrap().1.clone();
        assert_eq!(ask["buttons"][0], json!({ "label": "Allow", "action": "allow:r1", "tone": "ok" }));
        assert_eq!(ask["buttons"][1]["action"], "deny:r1");
        assert_eq!(ask["action"], "denywith:r1");
        // each answer reaches the island as an answer
        let press = |action: &str, text: &str| handle(&Event { event: "action".into(), action: action.into(), args: json!({ "text": text }), now: T0, ..Event::default() }).to_json();
        assert_eq!(press("allow:r1", "")["answer"], json!({ "id": "r1", "behavior": "allow", "text": "" }));
        assert_eq!(press("deny:r1", "")["answer"]["behavior"], "deny");
        assert_eq!(press("denywith:r1", "use the other file")["answer"], json!({ "id": "r1", "behavior": "deny", "text": "use the other file" }));
        // the person sets up what lets the island answer
        assert_eq!(press("answer_on", "")["claude_hooks"], json!({ "action": "install", "permission": true, "stop": false, "wait_s": 20 }));
        assert_eq!(press("answer_off", "")["claude_hooks"]["action"], "remove");
    }

    #[test]
    fn a_session_that_finished_says_what_it_did_and_can_be_replied_to_while_the_turn_is_open() {
        fresh();
        let mut f = rich(session("f", "idle", T0 - 180_000));
        f.turn_started = T0 - 180_000 - 362_000;
        let r = handle(&look(T0, vec![f.clone()])).to_json();
        let b = blocks(&r["card"]);
        assert_eq!(b[1].0, "strip");
        assert_eq!((b[1].1["text"].as_str(), b[1].1["side"].as_str()), (Some("Done in 6m 02s"), Some("3 min ago")));
        assert_eq!((b[2].0, b[2].1["label"].as_str(), b[2].1["text"].as_str()), ("text", Some("Result"), Some("The tiles are half the size now.")));
        assert_eq!(b[3].0, "files");
        assert_eq!(b[3].1["rows"][1], json!({ "name": "hub.css", "added": 142, "removed": 58 }));
        assert!(b.iter().all(|x| x.0 != "field"));
        f.asking = Some(ClaudeAsking { id: "s1".into(), kind: "stop".into(), ..ClaudeAsking::default() });
        let r = handle(&look(T0, vec![f])).to_json();
        let field = blocks(&r["card"]).into_iter().find(|x| x.0 == "field").unwrap().1.clone();
        assert_eq!(field["action"], "reply:s1");
        let said = handle(&Event { event: "action".into(), action: "reply:s1".into(), args: json!({ "text": "now the rest" }), now: T0, ..Event::default() }).to_json();
        assert_eq!(said["answer"], json!({ "id": "s1", "behavior": "reply", "text": "now the rest" }));
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
        assert_eq!(r["card"]["focus"], "focus:w");
        assert_eq!(r["cards_more"][0]["tone"], "finished");
        assert_eq!(r["cards_more"][1]["tone"], "working");
        assert_eq!(r["cards_more"][0]["id"], "sf");
    }

    #[test]
    fn a_session_has_its_card_for_as_long_as_it_runs() {
        fresh();
        let s = session("f", "idle", T0 - 60_000);
        assert_eq!(handle(&look(T0, vec![s.clone()])).to_json()["card"]["tone"], "finished");
        // looking at it (the Focus button) does not take it away
        let r = handle(&Event { event: "action".into(), action: "focus:f".into(), now: T0, ..Event::default() }).to_json();
        assert_eq!(r["card"]["tone"], "finished");
        assert_eq!(r["focus"]["title"], "nadi");
        assert!(r["focus"]["exe"].as_str().unwrap().ends_with("Code.exe"));
        // nor does being quiet for a long time: it is idle then
        assert_eq!(handle(&look(T0 + FINISHED_MS + 1, vec![s.clone()])).to_json()["card"]["tone"], "idle");
        assert_eq!(handle(&look(T0 + 10 * FINISHED_MS, vec![s])).to_json()["card"]["tone"], "idle");
        // it goes when its source does: the editor was closed, and the island no longer lists it
        assert_eq!(handle(&look(T0 + 10 * FINISHED_MS + 2000, vec![])).to_json()["card"], Value::Null);
    }

    #[test]
    fn every_session_of_an_editor_has_a_card_and_all_go_with_it() {
        fresh();
        let list: Vec<ClaudeSession> = (0..9).map(|i| session(&format!("v{i}"), if i == 0 { "busy" } else { "idle" }, T0 - 1_000_000 - i * 1000)).collect();
        let r = handle(&look(T0, list)).to_json();
        assert_eq!(r["cards_more"].as_array().unwrap().len(), 8);
        assert_eq!(r["card"]["tone"], "working");
        assert_eq!(handle(&look(T0 + 2000, vec![])).to_json()["card"], Value::Null);
    }

    fn desktop_window(title: &str) -> Event {
        Event {
            event: "processes".into(),
            windows: vec![Window { exe: "claude.exe".into(), path: "C:\\Users\\a\\AppData\\Local\\AnthropicClaude\\app-1.0\\claude.exe".into(), title: title.into(), front: true, ..Window::default() }],
            now: T0,
            ..Event::default()
        }
    }

    #[test]
    fn the_desktop_app_has_a_card_while_it_runs() {
        fresh();
        let r = handle(&desktop_window("Claude")).to_json();
        assert_eq!(r["card"]["title"], "Claude Desktop");
        assert_eq!(r["card"]["sub"], "In front");
        // with sessions it comes last
        let r = handle(&look(T0, vec![session("a", "busy", T0 - 1000)])).to_json();
        assert_eq!(r["card"]["title"], "Fix the thing");
        assert_eq!(r["cards_more"][0]["title"], "Claude Desktop");
        // pressing it brings its window to the front
        let r = handle(&Event { event: "action".into(), action: "focus:desktop".into(), now: T0, ..Event::default() }).to_json();
        assert!(r["focus"]["exe"].as_str().unwrap().ends_with("claude.exe"));
        // it is quit: its card goes (the session stays)
        let r = handle(&Event { event: "processes".into(), windows: vec![], now: T0 + 2000, ..Event::default() }).to_json();
        assert_eq!(r["card"]["title"], "Fix the thing");
        assert!(r["cards_more"].as_array().unwrap().is_empty());
        // the command line tool is not the app
        fresh();
        let cli = Event { event: "processes".into(), windows: vec![Window { exe: "claude.exe".into(), path: "C:\\Users\\a\\.local\\bin\\claude.exe".into(), front: true, ..Window::default() }], ..Event::default() };
        assert_eq!(handle(&cli).to_json()["card"], Value::Null);
    }

    /// the banners said at a look
    fn said(r: &Value) -> Vec<Value> {
        r["say"].as_array().cloned().unwrap_or_default()
    }

    /// a session that waits for a permission, as the island holds it
    fn asked(id: &str, updated: u64) -> ClaudeSession {
        let mut s = session(id, "busy", updated);
        s.waiting_for = "permission".into();
        s.asking = Some(ClaudeAsking { id: "r1".into(), kind: "permission".into(), tool: "Bash".into(), icon: "term".into(), detail: "cargo build --release".into(), age_ms: 100 });
        s
    }

    #[test]
    fn a_session_that_needs_a_permission_brings_a_banner_with_allow_and_deny_and_answering_takes_it_back() {
        fresh();
        let mut s = session("w", "busy", T0);
        handle(&look(T0, vec![s.clone()]));
        // the island holds the request: the banner has the buttons, as soon as it is there
        let r = handle(&look(T0 + 2000, vec![asked("w", T0 + 1500)])).to_json();
        let say = &said(&r)[0];
        assert_eq!(say["text"], "Fix the thing");
        assert_eq!(say["priority"], 90);
        assert_eq!(say["ambient"], true);
        assert_eq!(say["pill"]["key"], "w");
        assert_eq!(say["pill"]["tone"], "permission");
        assert_eq!(say["pill"]["press"], "focus:w");
        assert_eq!(say["pill"]["sub"], "Wants to run cargo build --release");
        assert_eq!(say["pill"]["tile"]["text"], "C");
        assert_eq!(say["pill"]["buttons"][0], json!({ "label": "Allow", "action": "allow:r1", "tone": "ok" }));
        assert_eq!(say["pill"]["buttons"][1]["action"], "deny:r1");
        // told again while it waits: no second banner
        assert!(said(&handle(&look(T0 + 4000, vec![asked("w", T0 + 1500)])).to_json()).is_empty());
        // answered: the banner is taken back
        s.updated = T0 + 6000;
        let r = handle(&look(T0 + 7000, vec![s])).to_json();
        assert_eq!(r["cancel"], json!(["w"]));
    }

    #[test]
    fn a_permission_the_island_does_not_hold_is_told_after_a_moment_and_has_no_buttons() {
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        let mut s = session("w", "busy", T0 + 1000);
        s.waiting_for = "permission".into();
        s.pending = Some(ClaudePending { tool: "Edit".into(), icon: "pen".into(), detail: "lib.rs".into() });
        // (too soon: the request may still come to the island)
        assert!(said(&handle(&look(T0 + 2000, vec![s.clone()])).to_json()).is_empty());
        let r = handle(&look(T0 + 4000, vec![s])).to_json();
        let say = &said(&r)[0];
        assert_eq!(say["pill"]["tone"], "permission");
        assert_eq!(say["pill"]["sub"], "Wants to change lib.rs");
        assert!(say["pill"].get("buttons").is_none());
    }

    #[test]
    fn a_session_that_wants_a_few_words_brings_the_ringing_phone() {
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        let mut s = session("w", "busy", T0 + 1000);
        s.waiting_for = "input".into();
        s.pending = Some(ClaudePending { tool: "AskUserQuestion".into(), icon: "ask".into(), detail: "which file should I change?".into() });
        let r = handle(&look(T0 + 4000, vec![s])).to_json();
        assert_eq!((said(&r)[0]["pill"]["tone"].as_str(), said(&r)[0]["pill"]["sub"].as_str()), (Some("reply"), Some("Asks: which file should I change?")));
        // a turn held open for a reply
        fresh();
        handle(&look(T0, vec![session("h", "busy", T0)]));
        let mut done = session("h", "idle", T0 + 30_000);
        done.reply = "The tiles are half the size.".into();
        done.asking = Some(ClaudeAsking { id: "s1".into(), kind: "stop".into(), ..ClaudeAsking::default() });
        let r = handle(&look(T0 + 31_000, vec![done])).to_json();
        assert_eq!((said(&r)[0]["pill"]["tone"].as_str(), said(&r)[0]["pill"]["sub"].as_str()), (Some("reply"), Some("Says: The tiles are half the size.")));
    }

    #[test]
    fn a_session_that_works_for_a_moment_has_started_and_a_quick_answer_has_not() {
        fresh();
        handle(&look(T0, vec![session("a", "idle", T0 - 600_000)]));
        // it begins: not yet news
        let mut s = session("a", "busy", T0 + 1000);
        s.turn_started = T0 + 1000;
        assert!(said(&handle(&look(T0 + 2000, vec![s.clone()])).to_json()).is_empty());
        // it is still at it
        let r = handle(&look(T0 + 5000, vec![s.clone()])).to_json();
        let say = &said(&r)[0];
        assert_eq!((say["pill"]["tone"].as_str(), say["pill"]["sub"].as_str(), say["priority"].as_i64()), (Some("working"), Some("Started working \u{b7} VS Code"), Some(40)));
        assert_eq!(say["pill"]["ms"], 3000);
        // said once for the turn
        assert!(said(&handle(&look(T0 + 7000, vec![s])).to_json()).is_empty());
        // a quick one is over before it matters
        fresh();
        handle(&look(T0, vec![session("a", "idle", T0 - 600_000)]));
        let mut q = session("a", "busy", T0 + 1000);
        q.turn_started = T0 + 1000;
        assert!(said(&handle(&look(T0 + 2000, vec![q])).to_json()).is_empty());
        assert!(said(&handle(&look(T0 + 4000, vec![session("a", "idle", T0 + 3000)])).to_json()).is_empty());
    }

    #[test]
    fn a_turn_that_took_a_while_is_done_with_how_long_and_what_it_changed_and_a_quick_one_is_not() {
        fresh();
        handle(&look(T0, vec![session("a", "busy", T0)]));
        let mut done = session("a", "idle", T0 + 362_000);
        done.turn_started = T0;
        done.files = vec![ClaudeFile { name: "lib.rs".into(), added: 4, removed: 1 }, ClaudeFile { name: "hub.css".into(), added: 2, removed: 0 }];
        let r = handle(&look(T0 + 363_000, vec![done])).to_json();
        let say = &said(&r)[0];
        assert_eq!((say["pill"]["tone"].as_str(), say["pill"]["sub"].as_str(), say["priority"].as_i64()), (Some("finished"), Some("Done in 6m 02s \u{b7} 2 files changed"), Some(60)));
        // a quick answer (under the limit of the settings, 10 s)
        fresh();
        handle(&look(T0, vec![session("a", "busy", T0)]));
        assert!(said(&handle(&look(T0 + 4000, vec![session("a", "idle", T0 + 3000)])).to_json()).is_empty());
        // ... with the limit at "never", it is told
        fresh();
        handle(&look(T0, vec![session("a", "busy", T0)]));
        let mut ev = look(T0 + 4000, vec![session("a", "idle", T0 + 3000)]);
        ev.settings = json!({ "skip": 0 });
        assert_eq!(said(&handle(&ev).to_json()).len(), 1);
    }

    #[test]
    fn a_turn_with_errors_has_stopped() {
        fresh();
        handle(&look(T0, vec![session("a", "busy", T0)]));
        let mut done = session("a", "idle", T0 + 60_000);
        done.errors = 2;
        let r = handle(&look(T0 + 61_000, vec![done.clone()])).to_json();
        assert_eq!((said(&r)[0]["pill"]["tone"].as_str(), said(&r)[0]["pill"]["sub"].as_str()), (Some("stopped"), Some("Stopped \u{b7} 2 errors")));
        // with that moment off, it is only done
        fresh();
        handle(&look(T0, vec![session("a", "busy", T0)]));
        let mut ev = look(T0 + 61_000, vec![done]);
        ev.settings = json!({ "b_stop": false });
        assert_eq!(said(&handle(&ev).to_json())[0]["pill"]["tone"], "finished");
    }

    #[test]
    fn going_quiet_and_opening_and_closing_are_off_until_they_are_asked_for() {
        // (a session that finished long ago, seen now)
        fresh();
        handle(&look(T0, vec![session("a", "busy", T0 - 400_000)]));
        let idle = session("a", "idle", T0 - 1000);
        let mut ev = look(T0 + 1000, vec![idle.clone()]);
        handle(&ev);
        ev = look(T0 + 6 * 60_000, vec![idle.clone()]);
        assert!(said(&handle(&ev).to_json()).is_empty());
        // ... asked for
        fresh();
        let mut first = look(T0, vec![session("a", "busy", T0 - 400_000)]);
        first.settings = json!({ "b_quiet": true, "b_open": true });
        handle(&first);
        let mut done = look(T0 + 1000, vec![session("a", "idle", T0 - 1000)]);
        done.settings = first.settings.clone();
        handle(&done);
        let mut late = look(T0 + 6 * 60_000, vec![session("a", "idle", T0 - 1000)]);
        late.settings = first.settings.clone();
        let r = handle(&late).to_json();
        assert_eq!((said(&r)[0]["pill"]["tone"].as_str(), said(&r)[0]["pill"]["sub"].as_str()), (Some("quiet"), Some("Quiet for 6 min")));
        // a new session, and one that is gone
        let mut two = look(T0 + 6 * 60_000 + 2000, vec![session("a", "idle", T0 - 1000), session("b", "idle", T0)]);
        two.settings = first.settings.clone();
        let r = handle(&two).to_json();
        assert_eq!((said(&r)[0]["pill"]["tone"].as_str(), said(&r)[0]["pill"]["sub"].as_str()), (Some("info"), Some("New session \u{b7} VS Code")));
        let mut gone = look(T0 + 6 * 60_000 + 4000, vec![session("b", "idle", T0)]);
        gone.settings = first.settings.clone();
        let r = handle(&gone).to_json();
        assert_eq!((said(&r)[0]["pill"]["tone"].as_str(), said(&r)[0]["pill"]["sub"].as_str()), (Some("closed"), Some("Session closed")));
    }

    #[test]
    fn several_things_at_once_are_one_banner_but_what_needs_you_is_never_in_it() {
        fresh();
        handle(&look(T0, vec![session("a", "busy", T0), session("b", "busy", T0), session("c", "busy", T0)]));
        let r = handle(&look(T0 + 61_000, vec![session("a", "idle", T0 + 60_000), session("b", "idle", T0 + 60_000), asked("c", T0 + 1000)])).to_json();
        let all = said(&r);
        assert_eq!(all.len(), 2);
        // (the one that needs you: its own, with its buttons)
        let own = all.iter().find(|x| x["pill"]["tone"] == "permission").unwrap();
        assert_eq!(own["pill"]["buttons"][0]["action"], "allow:r1");
        let many = all.iter().find(|x| x["pill"]["key"] == "claude-many").unwrap();
        assert_eq!((many["text"].as_str(), many["pill"]["sub"].as_str(), many["pill"]["tone"].as_str()), (Some("2 sessions"), Some("2 done"), Some("finished")));
    }

    #[test]
    fn no_banner_over_the_hub_with_the_setting_off_at_the_first_look_or_for_the_window_in_front() {
        let waiting = |updated: u64| {
            let mut s = session("w", "busy", updated);
            s.waiting_for = "input".into();
            s
        };
        // over the hub
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        let mut ev = look(T0 + 4000, vec![waiting(T0 + 1000)]);
        ev.hub = true;
        assert!(said(&handle(&ev).to_json()).is_empty());
        // the setting off
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        let mut ev = look(T0 + 4000, vec![waiting(T0 + 1000)]);
        ev.settings = json!({ "alerts": false });
        assert!(said(&handle(&ev).to_json()).is_empty());
        // the moment off
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        let mut ev = look(T0 + 4000, vec![waiting(T0 + 1000)]);
        ev.settings = json!({ "b_needs": false });
        assert!(said(&handle(&ev).to_json()).is_empty());
        // a session that was waiting before the island looked is not news
        fresh();
        assert!(said(&handle(&look(T0 + 4000, vec![waiting(T0 + 1000)])).to_json()).is_empty());
        // its window in front, on its project
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        handle(&Event { event: "processes".into(), windows: vec![Window { exe: "code.exe".into(), title: "lib.rs - nadi - Visual Studio Code".into(), front: true, ..Window::default() }], ..Event::default() });
        assert!(said(&handle(&look(T0 + 4000, vec![waiting(T0 + 1000)])).to_json()).is_empty());
        // ... but another folder of the same editor is not it
        fresh();
        handle(&look(T0, vec![session("w", "busy", T0)]));
        handle(&Event { event: "processes".into(), windows: vec![Window { exe: "code.exe".into(), title: "other - Visual Studio Code".into(), front: true, ..Window::default() }], ..Event::default() });
        assert_eq!(said(&handle(&look(T0 + 4000, vec![waiting(T0 + 1000)])).to_json()).len(), 1);
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
