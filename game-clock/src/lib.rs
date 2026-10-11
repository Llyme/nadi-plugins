//! Game clock: which game is open, and for how long. It says so, once, after the time you set (a banner of
//! priority 40). The programs that count as games are a setting. It only sees which programs have a window.

use nadi_plugin::*;
use std::cell::RefCell;

#[derive(Default)]
struct State {
    exe: String,
    since: u64,
    said: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn nice(exe: &str) -> String {
    let n = exe.trim_end_matches(".exe");
    let mut c = n.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn span(ms: u64) -> String {
    let m = ms / 60_000;
    if m >= 60 {
        format!("{}h {}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

fn handle(ev: &Event) -> Reply {
    if ev.event != "processes" {
        return Reply::nothing();
    }
    let games: Vec<String> = ev.setting_str("games").split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
    let remind = (ev.setting_f64("remind_min", 120.0).max(1.0) * 60_000.0) as u64;
    let Some(w) = ev.windows.iter().find(|w| games.contains(&w.exe)) else {
        STATE.with(|s| *s.borrow_mut() = State::default());
        return Reply::clear();
    };
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.exe != w.exe {
            *s = State { exe: w.exe.clone(), since: ev.now, said: 0 };
        }
        let played = ev.now.saturating_sub(s.since);
        let mut r = Reply::card(
            Card::new()
                .icon("gamepad")
                .title(nice(&w.exe))
                .sub(format!("Playing · {}", span(played)))
                .peek(span(played))
                .facts(&[("Session", span(played)), ("Reminder", format!("after {}", span(remind)))]),
        );
        // once for each stretch of the set length
        let due = played / remind;
        if due > s.said {
            s.said = due;
            r = r.say(format!("You have been playing {} for {}", nice(&w.exe), span(played)), 40);
        }
        r
    })
}

plugin!(handle);
