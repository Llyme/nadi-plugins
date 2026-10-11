//! Calendar: the events of a calendar link (.ics), on the island. One plugin, all of it here: the island reads the link for it
//! (the manifest names the address, `"format": "ics"`: a module has no network of its own) and this code works from the events.
//!
//! * a widget for the head: the date and the time (the island keeps the clock ticking), and what is next in its tooltip;
//!   pressed, it opens
//! * a pane: a month, a day picked on it, and the events of that day
//! * a card in the hub with the next ones
//! * a banner shortly before one starts
//!
//! It asks for the `calendar` permission, is told when the calendar changes, and asks the island to wake it every 15 seconds
//! to look at the clock. It only sees events that have not finished (an hour of grace), so a day that is over shows none.

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashSet;

/// events the card lists
const LISTED: usize = 6;
/// how often it looks at the clock, ms
const LOOK_MS: u64 = 15_000;
/// the card sits among the others
const RANK: i32 = 45;
const DAY_MS: i64 = 86_400_000;

#[derive(Default)]
struct State {
    events: Vec<CalEvent>,
    told: HashSet<String>,
    /// the pane is open
    open: bool,
    /// the month it shows: year * 12 + (month - 1)
    month: i32,
    /// the day picked, as days since 1970 (local)
    sel: i64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// the events that have not finished (an hour of grace after the end), first ones first
fn upcoming(events: &[CalEvent], now_ms: i64) -> Vec<&CalEvent> {
    events.iter().filter(|e| e.end_ms >= now_ms - 3_600_000).take(LISTED).collect()
}

/// the events that start within `lead_ms` from now and were not yet told (each is told once)
fn due<'a>(events: &'a [CalEvent], now_ms: i64, lead_ms: i64, told: &mut HashSet<String>) -> Vec<&'a CalEvent> {
    events
        .iter()
        .filter(|e| {
            let delta = e.start_ms - now_ms;
            delta > 0 && delta <= lead_ms
        })
        .filter(|e| told.insert(e.uid.clone()))
        .collect()
}

/// the first event leads the card; the others are what it opens to
fn card(list: &[&CalEvent]) -> Card {
    let first = list[0];
    let rest: Vec<(String, String, Option<f32>)> = list[1..].iter().map(|e| (e.summary.clone(), when(e.start_ms, e.all_day), None)).collect();
    let mut c = Card::new().icon("calendar").rank(RANK).title(first.summary.clone()).sub(when(first.start_ms, first.all_day));
    if !rest.is_empty() {
        c = c.list(&rest);
    }
    c
}

// ------------------------------------------------------------------------------------------------- days and months

/// days since 1970-01-01 of a date (the proleptic Gregorian calendar)
fn days_from(y: i32, m: u32, d: u32) -> i64 {
    let y = (if m <= 2 { y - 1 } else { y }) as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// the local date of a day number
fn of_day(n: i64, tz: i32) -> Civil {
    civil((n * 86_400 + 43_200) * 1000 - tz as i64 * 60_000, tz)
}

/// the first day of a month
fn first_of(month: i32) -> i64 {
    days_from(month.div_euclid(12), (month.rem_euclid(12) + 1) as u32, 1)
}

/// the start of a local day, ms (UTC)
fn day_start(n: i64, tz: i32) -> i64 {
    n * DAY_MS - tz as i64 * 60_000
}

/// does the event touch the day (an end is exclusive: a 00:00-00:00 all-day event covers exactly one day)
fn on_day(e: &CalEvent, n: i64, tz: i32) -> bool {
    let (from, to) = (day_start(n, tz), day_start(n + 1, tz));
    let last = if e.end_ms > e.start_ms { e.end_ms - 1 } else { e.start_ms };
    e.start_ms < to && last >= from
}

fn clock12(c: &Civil) -> String {
    let (h, suffix) = match c.hour {
        0 => (12, "AM"),
        1..=11 => (c.hour, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    };
    format!("{h}:{:02} {suffix}", c.minute)
}

/// "3:00 PM – 4:00 PM", "All day", "3:00 PM →", "→ 4:00 PM" as the event sits on the day
fn span_text(e: &CalEvent, n: i64, tz: i32) -> String {
    if e.all_day {
        return "All day".into();
    }
    let at = |ms: i64| clock12(&civil(ms, tz));
    if e.end_ms <= e.start_ms {
        return at(e.start_ms);
    }
    let (from, to) = (day_start(n, tz), day_start(n + 1, tz));
    match (e.start_ms >= from, e.end_ms <= to) {
        (true, true) => format!("{} – {}", at(e.start_ms), at(e.end_ms)),
        (true, false) => format!("{} →", at(e.start_ms)),
        (false, true) => format!("→ {}", at(e.end_ms)),
        _ => "All day".into(),
    }
}

const MONTH_NAMES: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAY_NAMES: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
/// the column labels, Sunday first
const COLUMNS: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];

/// the pane: the month, and the events of the day picked
fn pane(s: &State, ev: &Event) -> Blocks {
    let tz = ev.tz_offset;
    let today = of_day(((ev.now as i64 + tz as i64 * 60_000).div_euclid(DAY_MS)).max(0), tz).days;
    let week_start = if ev.setting_f64("week_start", 0.0) as i64 == 1 { 1usize } else { 0usize };
    let (y, m) = (s.month.div_euclid(12), s.month.rem_euclid(12) as usize);
    let first = first_of(s.month);
    // (0 is Monday in a Civil; the columns here start on Sunday or on Monday)
    let sunday_based = (of_day(first, tz).weekday as usize + 1) % 7;
    let offset = (sunday_based + 7 - week_start) % 7;
    let start = first - offset as i64;
    let labels: Vec<&str> = (0..7).map(|i| COLUMNS[(i + week_start) % 7]).collect();
    let cells: Vec<Cell> = (0..42)
        .map(|i| {
            let n = start + i;
            let c = of_day(n, tz);
            let in_month = c.month as usize == m + 1;
            let dots = s.events.iter().filter(|e| on_day(e, n, tz)).count().min(3) as u32;
            Cell {
                t: c.day.to_string(),
                id: format!("d:{n}"),
                state: if n == s.sel { "sel" } else if n == today { "today" } else if !in_month { "other" } else { "" },
                dots,
            }
        })
        .collect();
    let mut b = Blocks::new().head(&format!("{} {y}", MONTH_NAMES[m]), "prev", "next", "today").grid(&labels, &cells);
    if ev.settings["url"].as_str().unwrap_or("").trim().is_empty() {
        return b.text("muted", "No calendar link yet. Paste one in Settings > Plugins > Calendar.");
    }
    let mut list: Vec<&CalEvent> = s.events.iter().filter(|e| on_day(e, s.sel, tz)).collect();
    list.sort_by_key(|e| (!e.all_day, e.start_ms));
    if !list.is_empty() {
        let c = of_day(s.sel, tz);
        b = b.text("muted", format!("{}, {} {} · {} {}", WEEKDAY_NAMES[c.weekday as usize], MONTHS[(c.month - 1) as usize], c.day, list.len(), if list.len() == 1 { "event" } else { "events" }));
        let rows: Vec<Row> = list
            .iter()
            .map(|e| Row { title: e.summary.clone(), sub: String::new(), time: span_text(e, s.sel, tz), past: e.end_ms < ev.now as i64 && !e.all_day })
            .collect();
        b = b.rows(&rows, "");
    }
    b
}

/// the widget's tooltip: what comes next
fn tip(events: &[CalEvent], now: i64, tz: i32) -> Blocks {
    match upcoming(events, now).first() {
        None => Blocks::new().text("muted", "Nothing coming up"),
        Some(e) => {
            let c = civil(e.start_ms, tz);
            let at = if e.all_day { "all day".to_string() } else { clock12(&c) };
            Blocks::new().text("title", e.summary.clone()).text("muted", format!("{}, {} {} · {at}", WEEKDAYS[c.weekday as usize], MONTHS[(c.month - 1) as usize], c.day))
        }
    }
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let tz = ev.tz_offset;
        let now = ev.now as i64;
        let today = (now + tz as i64 * 60_000).div_euclid(DAY_MS);
        let mut opened = false;
        match ev.event.as_str() {
            "calendar" => s.events = ev.events.clone(),
            "start" => {
                // (switched on again: what was told before is told again if it is still ahead)
                s.told.clear();
                s.open = false;
            }
            "action" if ev.action == "widget" => {
                if s.open && ev.args["pane_open"].as_bool().unwrap_or(true) {
                    s.open = false;
                    // the pane goes; the cards are back
                    return Reply::close_pane().with_widget("date", date_widget(ev), tip(&s.events, now, tz));
                }
                s.open = true;
                opened = true;
                s.sel = today;
                s.month = of_day(today, tz).year * 12 + of_day(today, tz).month as i32 - 1;
            }
            "action" if ev.action == "pane" => {
                let id = ev.args["id"].as_str().unwrap_or("");
                match id {
                    "prev" | "next" => {
                        let delta = if id == "prev" { -1 } else { 1 };
                        s.month += delta;
                        // (the same day of the month where it exists)
                        let d = of_day(s.sel, tz).day;
                        let last = first_of(s.month + 1) - first_of(s.month);
                        s.sel = first_of(s.month) + (d as i64 - 1).min(last - 1);
                    }
                    "today" => {
                        s.sel = today;
                        s.month = of_day(today, tz).year * 12 + of_day(today, tz).month as i32 - 1;
                    }
                    d if d.starts_with("d:") => {
                        if let Ok(n) = d[2..].parse::<i64>() {
                            s.sel = n;
                            let c = of_day(n, tz);
                            s.month = c.year * 12 + c.month as i32 - 1;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        let list = upcoming(&s.events, now);
        let mut r = if list.is_empty() { Reply::clear() } else { Reply::card(card(&list)) };
        r = r.wake_in(LOOK_MS).with_widget("date", date_widget(ev), tip(&s.events, now, tz));
        if s.open {
            r = r.with_pane("Calendar", pane(&s, ev), opened);
        }
        if ev.setting_bool("remind", true) {
            let lead_ms = ev.setting_f64("lead_min", 15.0).max(1.0) as i64 * 60_000;
            let events = s.events.clone();
            for e in due(&events, now, lead_ms, &mut s.told) {
                r = r.say(format!("{} in {} min", e.summary, ((e.start_ms - now) / 60_000).max(1)), 40);
            }
        }
        r
    })
}

/// the date and the time, as the island ticks them
fn date_widget(ev: &Event) -> Blocks {
    Blocks::new().clock_time(ev.setting_bool("h24", false)).clock_date()
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    fn e(uid: &str, start: i64, end: i64) -> CalEvent {
        CalEvent::new(uid, uid, start, end)
    }

    #[test]
    fn a_reminder_comes_once_and_only_inside_the_lead() {
        let events = vec![e("soon", 10 * 60_000, 11 * 60_000), e("later", 90 * 60_000, 91 * 60_000), e("past", -5 * 60_000, -4 * 60_000)];
        let mut told = HashSet::new();
        let lead = 15 * 60_000;
        let first = due(&events, 0, lead, &mut told);
        assert_eq!(first.iter().map(|e| e.uid.as_str()).collect::<Vec<_>>(), vec!["soon"]);
        assert!(due(&events, 60_000, lead, &mut told).is_empty());
        // later comes into the lead
        let next = due(&events, 80 * 60_000, lead, &mut told);
        assert_eq!(next.iter().map(|e| e.uid.as_str()).collect::<Vec<_>>(), vec!["later"]);
    }

    #[test]
    fn the_card_lists_what_has_not_finished() {
        let hour = 3_600_000;
        let events = vec![e("old", -10 * hour, -9 * hour), e("just ended", -2 * hour, -hour - 1), e("going", -hour / 2, hour / 2), e("next", hour, 2 * hour)];
        let list = upcoming(&events, 0);
        assert_eq!(list.iter().map(|e| e.uid.as_str()).collect::<Vec<_>>(), vec!["going", "next"]);
    }

    #[test]
    fn the_module_tells_a_reminder_and_draws_the_card() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let events = vec![e("standup", 10 * 60_000, 40 * 60_000), e("lunch", 300 * 60_000, 360 * 60_000)];
        let ev = |kind: &str, now: u64| Event { event: kind.into(), now, events: events.clone(), settings: json!({ "remind": true, "lead_min": 15 }), ..Event::default() };
        let r = handle(&ev("calendar", 0)).to_json();
        assert_eq!(r["card"]["title"], "standup");
        assert_eq!(r["card"]["sub"], "{when:600000}");
        assert_eq!(r["card"]["rank"], 45);
        assert_eq!(r["card"]["body"][0]["rows"][0]["title"], "lunch");
        assert_eq!(r["say"][0]["text"], "standup in 10 min");
        assert_eq!(r["wake_ms"], 15_000);
        // the widget shows the clock and the date, and says what is next
        assert_eq!(r["widgets"]["date"]["blocks"][0]["clock"], "time");
        assert_eq!(r["widgets"]["date"]["blocks"][1]["clock"], "date");
        assert_eq!(r["widgets"]["date"]["tip"][0]["text"], "standup");
        // the next call tells nothing again
        assert_eq!(handle(&ev("tick", 15_000)).to_json()["say"].as_array().unwrap().len(), 0);
        // the calendar is empty: the card goes
        let empty = Event { event: "calendar".into(), now: 0, events: vec![], settings: json!({}), ..Event::default() };
        assert_eq!(handle(&empty).to_json()["card"], Value::Null);
        // with the reminder switched off it says nothing
        STATE.with(|s| *s.borrow_mut() = State::default());
        let quiet = Event { settings: json!({ "remind": false, "lead_min": 15 }), ..ev("calendar", 0) };
        assert_eq!(handle(&quiet).to_json()["say"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn days_and_months_agree_with_the_calendar() {
        assert_eq!(days_from(1970, 1, 1), 0);
        assert_eq!(days_from(2026, 10, 11), 20_737);
        let c = of_day(20_737, 0);
        assert_eq!((c.year, c.month, c.day, c.weekday), (2026, 10, 11, 6));
        // 2026-10 starts on a Thursday, and has 31 days
        assert_eq!(of_day(first_of(2026 * 12 + 9), 0).weekday, 3);
        assert_eq!(first_of(2026 * 12 + 10) - first_of(2026 * 12 + 9), 31);
        // a zone ahead of UTC: the same moment is the next day there
        assert_eq!(of_day(20_737, 540).day, 11);
    }

    #[test]
    fn the_widget_opens_a_month_and_a_day_lists_its_events() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        // 2026-10-11 09:00 UTC
        let now = 1_791_709_200_000u64;
        let day = |n: i64| n * DAY_MS;
        let events = vec![e("a", day(20_737) + 10 * 3_600_000, day(20_737) + 11 * 3_600_000), e("b", day(20_739) + 3_600_000, day(20_739) + 7_200_000)];
        let ev = |kind: &str, action: &str, args: Value| Event {
            event: kind.into(),
            action: action.into(),
            args,
            now,
            events: events.clone(),
            settings: json!({ "url": "https://example.com/a.ics", "remind": false }),
            ..Event::default()
        };
        // nothing is open until the widget is pressed
        let r = handle(&ev("calendar", "", json!(null))).to_json();
        assert!(r.get("pane").is_none());
        // pressed: the pane opens on today
        let r = handle(&ev("action", "widget", json!({ "id": "date", "pane_open": false }))).to_json();
        assert_eq!(r["pane"]["title"], "Calendar");
        assert_eq!(r["pane"]["open"], true);
        assert_eq!(r["pane"]["blocks"][0]["head"], "October 2026");
        let grid = &r["pane"]["blocks"][1]["grid"];
        assert_eq!(grid["cells"].as_array().unwrap().len(), 42);
        assert_eq!(grid["labels"][0], "S");
        // the first cell is the Sunday before Thursday 1 October: 27 September
        assert_eq!(grid["cells"][0]["t"], "27");
        let today = grid["cells"].as_array().unwrap().iter().find(|c| c["state"] == "sel").unwrap();
        assert_eq!(today["t"], "11");
        assert_eq!(today["dots"], 1);
        // today's event is listed, with its time
        assert_eq!(r["pane"]["blocks"][3]["rows"][0]["title"], "a");
        assert_eq!(r["pane"]["blocks"][3]["rows"][0]["time"], "10:00 AM – 11:00 AM");
        // a day picked: its events
        let r = handle(&ev("action", "pane", json!({ "id": "d:20739" }))).to_json();
        assert_eq!(r["pane"]["open"], false);
        assert_eq!(r["pane"]["blocks"][3]["rows"][0]["title"], "b");
        // the next month
        let r = handle(&ev("action", "pane", json!({ "id": "next" }))).to_json();
        assert_eq!(r["pane"]["blocks"][0]["head"], "November 2026");
        // the widget pressed again, with the pane open: it closes
        let r = handle(&ev("action", "widget", json!({ "id": "date", "pane_open": true }))).to_json();
        assert_eq!(r["pane"], Value::Null);
        // the user went elsewhere and the pane is not showing: pressed, it opens again
        let r = handle(&ev("action", "widget", json!({ "id": "date", "pane_open": false }))).to_json();
        assert_eq!(r["pane"]["open"], true);
    }

    #[test]
    fn without_a_link_the_pane_says_where_to_put_one() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let ev = Event { event: "action".into(), action: "widget".into(), args: json!({ "pane_open": false }), now: 1_791_709_200_000, settings: json!({ "url": "" }), ..Event::default() };
        let r = handle(&ev).to_json();
        assert!(r["pane"]["blocks"][2]["text"].as_str().unwrap().contains("No calendar link"));
    }
}
