//! Claude Code: the sessions running on this PC, as cards, and a pill when one needs you or has finished.
//!
//! The island reads the files Claude Code keeps and tells the module what each session is doing (`claude`, every 2 s), for every
//! session whose process is still there. The module decides what a session is (waiting for you, working, finished, idle) and draws
//! a card for each one, the ones that need you first. A card stays as long as its session runs: it goes only when its source is
//! gone (the editor was closed, the terminal exited), never because it was looked at or has been quiet. The Claude desktop app has a
//! card too while it runs. The module also brings a pill when a session starts waiting for you or finishes a turn that took a while;
//! it sees which window is in front (`processes`), so it does not interrupt you with a session you are looking at.
//!
//! (How much of the Claude limits is used is another plugin, `claude-usage`: it reaches Anthropic, so it sees nothing of this PC.)

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// idle again after a turn, this recently: finished, and the result waits for you
const FINISHED_MS: u64 = 15 * 60 * 1000;
/// a turn that took no more than this is a quick answer, not news
const QUICK_MS: u64 = 5_000;
/// the most cards (the island lets a module have this many)
const MAX_CARDS: usize = 24;
const TILE: &str = "#d97757";

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
    /// the briefs: session id -> (how it stood, when the busy stretch began)
    seen: HashMap<String, (Kind, u64)>,
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
                let (says, cancel) = alerts(&mut st, ev.hub, ev.setting_bool("alerts", true));
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
