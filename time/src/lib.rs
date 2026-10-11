//! Time: every so often the island drops down as a small pill that says what time it is. It lines up with the clock
//! (every 30 minutes means :00 and :30), so it never needs to count: it asks the island to wake it at the next minute,
//! and says the time when the minute is one of the marks. It sees nothing but the time.
//!
//! Pressing "Show it now" in its settings sends it the `preview` action; what it says in answer to a button
//! is always shown.

use nadi_plugin::*;
use std::cell::Cell;

thread_local! {
    /// the (day, minute of the day) of the last mark that was said: one for each
    static LAST: Cell<Option<(i64, u32)>> = Cell::new(None);
}

/// "3:30 PM" or "15:30"
fn clock(c: &Civil, h24: bool) -> String {
    if h24 {
        format!("{:02}:{:02}", c.hour, c.minute)
    } else {
        let (h12, suffix) = match c.hour {
            0 => (12, "AM"),
            1..=11 => (c.hour, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        format!("{h12}:{:02} {suffix}", c.minute)
    }
}

/// "Fri, Oct 9"
fn date(c: &Civil) -> String {
    format!("{}, {} {}", WEEKDAYS[c.weekday as usize], MONTHS[(c.month - 1) as usize], c.day)
}

fn say(c: &Civil, h24: bool, priority: i32) -> Say {
    Say::new(clock(c, h24), priority).pill("clock", date(c)).big().for_ms(4000)
}

/// whether a minute of the day is one the plugin speaks at, for "every this many minutes"
fn is_mark(minute_of_day: u32, every: u32) -> bool {
    every > 0 && minute_of_day % every == 0
}

/// ms until the next minute begins (a little after, so the clock has moved)
fn to_next_minute(now_ms: u64) -> u64 {
    60_000 - now_ms % 60_000 + 20
}

fn handle(ev: &Event) -> Reply {
    let c = ev.local();
    let h24 = ev.setting_bool("h24", false);
    if ev.event == "action" {
        return if ev.action == "preview" { Reply::nothing().say_with(say(&c, h24, 10)) } else { Reply::nothing() };
    }
    let every = ev.setting_f64("every", 60.0).max(1.0) as u32;
    let minute_of_day = c.hour * 60 + c.minute;
    let mark = (c.days, minute_of_day);
    let mut r = Reply::nothing().wake_in(to_next_minute(ev.now));
    if is_mark(minute_of_day, every) && LAST.with(|l| l.get()) != Some(mark) {
        LAST.with(|l| l.set(Some(mark)));
        // not over the hub, not in a game
        r = r.say_with(say(&c, h24, 10).ambient());
    }
    r
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, now: u64, tz: i32, every: u32) -> Event {
        Event { event: kind.into(), now, tz_offset: tz, settings: json!({ "every": every, "h24": false }), ..Event::default() }
    }

    #[test]
    fn writes_the_time_both_ways() {
        let t = civil(1_791_559_845_000, 0); // Fri 2026-10-09 15:30:45
        assert_eq!(clock(&t, true), "15:30");
        assert_eq!(clock(&t, false), "3:30 PM");
        assert_eq!(date(&t), "Fri, Oct 9");
        assert_eq!(clock(&civil(1_791_559_845_000 - 15 * 3_600_000 - 30 * 60_000, 0), false), "12:00 AM");
        assert_eq!(clock(&civil(1_791_559_845_000 - 3 * 3_600_000, 0), false), "12:30 PM");
    }

    fn said(r: &Reply) -> Vec<Value> {
        r.to_json()["say"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn says_the_time_at_a_mark_once() {
        let at = 1_791_559_845_000 - 45_000; // 15:30:00 UTC
        // 15:31 is not a mark
        assert!(said(&handle(&ev("tick", at + 60_000, 0, 60))).is_empty());
        // 15:30 is a mark for every 30 minutes
        let r = handle(&ev("tick", at, 0, 30));
        let s = said(&r);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0]["text"], "3:30 PM");
        assert_eq!(s[0]["pill"]["look"], "big");
        assert_eq!(s[0]["ambient"], true);
        // the same minute again (the island called it twice): once
        assert!(said(&handle(&ev("tick", at + 5_000, 0, 30))).is_empty());
        // and it always asks to be woken at the next minute
        assert_eq!(handle(&ev("tick", at + 61_000, 0, 30)).to_json()["wake_ms"], 59_000 + 20);
        // a button is answered, and not as ambient
        let p = handle(&Event { action: "preview".into(), ..ev("action", at, 0, 60) });
        assert_eq!(said(&p)[0]["ambient"], false);
    }

    #[test]
    fn the_hour_is_the_hour_of_the_pc_not_of_utc() {
        // 15:30 UTC is 00:30 the next day in Tokyo: a mark for every 30 minutes, and every hour at :00 only
        let at = 1_791_559_845_000 - 45_000;
        assert_eq!(said(&handle(&ev("tick", at, 540, 30)))[0]["text"], "12:30 AM");
        assert!(said(&handle(&ev("tick", at + 3_600_000 + 1, 540, 40))).is_empty());
    }
}
