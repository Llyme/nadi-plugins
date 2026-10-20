//! Battery: how full the battery is, as a liquid in a vessel. A tile (the whole tile is the vessel), a card (the charge of the last three
//! hours, how it stands, how worn it is), three widgets for the heads of the islands, and a banner when it runs low or critical, is
//! plugged in or out, is full, battery saver comes on, or it is wearing out. The island tells the module the battery (`battery`, with
//! the `battery` permission: how full it is, whether the charger is in, the time left, the rate, the health) and draws the liquid; the
//! module only decides what to say. A PC with no battery is told so once, and shows nothing.

use nadi_plugin::*;
use std::cell::RefCell;

const GREEN: &str = "#5ac88c";
const AMBER: &str = "#e8b04a";
const RED: &str = "#e87a7a";
const GREY: &str = "#8e8e96";
/// the charge of this many hours is kept for the graph, and the graph has at most this many points
const HOURS_MS: u64 = 3 * 3_600_000;
const POINTS: usize = 54;
/// a sample is kept at least this far apart
const SAMPLE_MS: u64 = 120_000;

#[derive(Default)]
struct State {
    /// the battery as it was last told: what a change is a change from
    seen: Option<Battery>,
    /// (when, how full) over the last three hours
    samples: Vec<(u64, f32)>,
    /// the lowest health a banner has said, so it says again only after another 10%
    health_said: Option<f64>,
    /// the module said there is no battery
    told_none: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// "3h 20m", "48 min"
fn dur(min: i64) -> String {
    let min = min.max(0);
    if min >= 60 {
        format!("{}h {:02}m", min / 60, min % 60).replace("h 00m", "h")
    } else {
        format!("{min} min")
    }
}

/// the colour of the liquid: green, amber at the low mark, red at the critical one, and green whatever the number while it charges
fn color_of(percent: f64, charging: bool, low: f64, critical: f64) -> &'static str {
    if charging {
        GREEN
    } else if percent <= critical {
        RED
    } else if percent <= low {
        AMBER
    } else {
        GREEN
    }
}

/// what is left or what is to come, in a few words
fn left_of(b: &Battery) -> String {
    match (b.charging, b.plugged, b.minutes_to_full, b.minutes_left) {
        (true, _, Some(m), _) => format!("{} to full", dur(m)),
        (true, _, None, _) => "Charging".into(),
        (false, true, _, _) if b.percent.unwrap_or(0.0) >= 99.5 => "Fully charged".into(),
        (false, true, _, _) => "Plugged in".into(),
        (false, false, _, Some(m)) => format!("{} left", dur(m)),
        _ => String::new(),
    }
}

/// the line under the title of the card
fn sub_of(b: &Battery) -> String {
    let left = left_of(b);
    let head = if b.charging { "Charging" } else if b.plugged { "Plugged in" } else { "On battery" };
    if left.is_empty() || left == head {
        head.to_string()
    } else if b.plugged && !b.charging {
        left
    } else {
        format!("{head} \u{b7} {left}")
    }
}

/// the points of the graph: what was sampled in the last three hours, at most `POINTS` of them, the newest the number it is now
fn graph_of(samples: &[(u64, f32)]) -> Vec<f32> {
    if samples.len() <= POINTS {
        return samples.iter().map(|s| s.1).collect();
    }
    (0..POINTS).map(|i| samples[i * (samples.len() - 1) / (POINTS - 1)].1).collect()
}

/// a banner: the circle is the liquid, filled to the charge
fn banner(title: &str, sub: String, tone: &str, color: &str, b: &Battery, ms: u64, prio: i32) -> Say {
    let percent = b.percent.unwrap_or(0.0);
    Say::new(title, prio)
        .pill("battery", "")
        .keyed("battery", tone, "")
        .with_sub(sub)
        .with_tile(Tile::liquid(format!("{}", percent.round() as i64), color, (percent / 100.0) as f32, b.charging), Vec::new())
        .for_ms(ms)
        .ambient()
}

fn about(b: &Battery) -> String {
    let p = format!("{}%", b.percent.unwrap_or(0.0).round() as i64);
    match b.minutes_left {
        Some(m) => format!("{p} \u{b7} about {} left", dur(m)),
        None => p,
    }
}

/// What changed since the battery was last told, as banners (at most one is said: the one that matters most), and the setting for each.
fn banners(st: &mut State, now: &Battery, ev: &Event) -> Option<Say> {
    let prev = st.seen.clone()?;
    if !ev.setting_bool("alerts", true) {
        return None;
    }
    let (Some(p), Some(q)) = (now.percent, prev.percent) else { return None };
    let low = ev.setting_f64("low", 20.0);
    // (the critical mark is under the low one)
    let critical = ev.setting_f64("critical", 10.0).min(low - 1.0).max(1.0);
    let on = |k: &str, d: bool| ev.setting_bool(k, d);
    let color = color_of(p, now.charging, low, critical);
    let mut said: Vec<Say> = Vec::new();
    let draining = !now.plugged && !now.charging;
    if draining && on("b_critical", true) && q > critical && p <= critical {
        said.push(banner("Battery critical", format!("{} \u{b7} plug in", about(now)), "critical", RED, now, 8000, 90));
    } else if draining && on("b_critical", true) && critical > 5.0 && q > 5.0 && p <= 5.0 {
        // (and again, at the last few percent)
        said.push(banner("Battery critical", format!("{} \u{b7} plug in", about(now)), "critical", RED, now, 8000, 90));
    } else if draining && on("b_low", true) && q > low && p <= low {
        said.push(banner("Battery low", about(now), "low", AMBER, now, 5000, 60));
    }
    if !prev.plugged && now.plugged && on("b_plugged", true) {
        let mut sub = format!("{}%", p.round() as i64);
        if let Some(m) = now.minutes_to_full {
            sub.push_str(&format!(" \u{b7} about {} to full", dur(m)));
        }
        if let Some(w) = now.rate_w.filter(|w| *w > 0.5) {
            sub.push_str(&format!(" \u{b7} {} W", w.round() as i64));
        }
        said.push(banner("Charging", sub, "charging", GREEN, now, 3000, 40));
    }
    if prev.plugged && !now.plugged && on("b_unplugged", false) {
        let tone = if color == AMBER { "low" } else if color == RED { "critical" } else { "charging" };
        said.push(banner("On battery", about(now), tone, color, now, 3000, 40));
    }
    if now.plugged && p >= 99.5 && q < 99.5 && on("b_full", true) {
        said.push(banner("Fully charged", "100% \u{b7} you can unplug".to_string(), "full", GREEN, now, 4000, 40));
    }
    if !prev.saver && now.saver && on("b_saver", false) {
        said.push(banner("Battery saver on", format!("{}% \u{b7} it dims the screen and slows the PC down", p.round() as i64), "saver", AMBER, now, 4000, 40));
    }
    if let Some(h) = now.health.filter(|h| *h < 80.0) {
        if st.health_said.map_or(true, |s| h <= s - 10.0) {
            st.health_said = Some(h);
            if on("b_health", false) {
                let wh = match (now.full_wh, now.design_wh) {
                    (Some(f), Some(d)) => format!(" \u{b7} {} Wh of {}", f.round() as i64, d.round() as i64),
                    _ => String::new(),
                };
                said.push(banner("Battery is wearing out", format!("Holds {}% of what it did new{wh}", h.round() as i64), "health", GREY, now, 6000, 40));
            }
        }
    }
    said.into_iter().max_by_key(|s| s.priority)
}

fn nothing(widgets: Reply) -> Reply {
    widgets.with_widget("battery", Blocks::new(), Blocks::new()).with_widget("circle", Blocks::new(), Blocks::new()).with_widget("charge", Blocks::new(), Blocks::new())
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        if ev.event != "battery" {
            return Reply::nothing();
        }
        let b = ev.battery.clone();
        // a PC with no battery: nothing is shown, and it is said once why
        if !b.present || b.percent.is_none() {
            st.seen = None;
            let said = !st.told_none;
            st.told_none = true;
            let r = nothing(Reply::clear());
            return if said { r.note("No battery found: this PC runs on the wall.") } else { r };
        }
        st.told_none = false;
        let percent = b.percent.unwrap_or(0.0);
        // the charge of the last three hours
        let at = ev.now;
        st.samples.retain(|s| at.saturating_sub(s.0) <= HOURS_MS);
        let moved = st.samples.last().map_or(true, |s| (s.1 as f64 - percent).abs() >= 1.0 || at.saturating_sub(s.0) >= SAMPLE_MS);
        if moved {
            st.samples.push((at, percent as f32));
        }
        // the banner (the first look is not news)
        let said = banners(&mut st, &b, ev);
        if st.seen.is_none() {
            // (what is already wrong is not said: it is known)
            if let Some(h) = b.health.filter(|h| *h < 80.0) {
                st.health_said = Some(h);
            }
        }
        st.seen = Some(b.clone());

        let low = ev.setting_f64("low", 20.0);
        let critical = ev.setting_f64("critical", 10.0).min(low - 1.0).max(1.0);
        let color = color_of(percent, b.charging, low, critical);
        let level = (percent / 100.0) as f32;
        let big = format!("{}%", percent.round() as i64);
        let left = left_of(&b);
        let number = format!("{}", percent.round() as i64);

        // the card: the vessel is its circle, the battery lies at the right of its header
        let mut chips: Vec<(&str, String, &str)> = Vec::new();
        if b.plugged {
            chips.push(("plug", "Plugged in".into(), "ok"));
        }
        chips.push(("leaf", if b.saver { "Battery saver on".into() } else { "Battery saver off".into() }, if b.saver { "hot" } else { "" }));
        if let Some(h) = b.health {
            chips.push(("heart", format!("Health {}%", h.round() as i64), ""));
        }
        let points = graph_of(&st.samples);
        let mut card = Card::new()
            .icon("battery")
            .rank(40)
            .title("Battery")
            .sub(sub_of(&b))
            .tile_of(Tile::liquid(number.clone(), color, level, b.charging))
            .glance(big.clone(), left.clone())
            .chips(&chips);
        if points.len() >= 2 {
            card = card.graph(&points, 100.0, &[]);
        }
        let rate = b.rate_w.map(|w| format!("{} W", w.abs().round() as i64)).unwrap_or_else(|| "\u{2013}".into());
        let (time, time_label) = if b.charging {
            (b.minutes_to_full.map(dur).unwrap_or_else(|| "\u{2013}".into()), "To full")
        } else {
            (b.minutes_left.map(dur).unwrap_or_else(|| "\u{2013}".into()), "Left")
        };
        card = card.stats(&[
            (rate, if b.charging { "Charging" } else { "Drawing" }),
            (time, time_label),
            (b.health.map(|h| format!("{}%", h.round() as i64)).unwrap_or_else(|| "\u{2013}".into()), "Health"),
            (b.full_wh.map(|w| format!("{} Wh", w.round() as i64)).unwrap_or_else(|| "\u{2013}".into()), "Full charge"),
        ]);

        // the widgets: the battery and its words, the circle, and the line of the last hours
        let sub = if left.is_empty() { String::new() } else { left.clone() };
        let w_battery = Blocks::new().vessel("icon", level, color, b.charging, &big, &sub);
        let w_circle = Blocks::new().vessel("circle", level, color, b.charging, &number, "Battery");
        let w_charge = Blocks::new().spark(&points, color, &big, if b.charging { "Charging" } else { "Last 3 hours" });
        let tip = Blocks::new().text("", sub_of(&b));
        let mut reply = Reply::card(card)
            .with_widget("battery", w_battery, tip.clone())
            .with_widget("circle", w_circle, tip.clone())
            .with_widget("charge", w_charge, tip);
        if let Some(s) = said {
            reply = reply.say_with(s);
        }
        reply
    })
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const T0: u64 = 1_800_000_000_000;

    fn battery(percent: f64) -> Battery {
        Battery { present: true, percent: Some(percent), minutes_left: Some(200), health: Some(91.0), full_wh: Some(48.0), design_wh: Some(52.7), ..Battery::default() }
    }

    fn at(now: u64, b: Battery, settings: Value) -> Event {
        Event { event: "battery".into(), now, battery: b, settings, ..Event::default() }
    }

    fn fresh() {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }

    fn said(r: &Value) -> Vec<Value> {
        r["say"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn the_card_is_the_vessel_and_says_how_it_stands() {
        fresh();
        let r = handle(&at(T0, battery(64.0), json!({}))).to_json();
        let c = &r["card"];
        assert_eq!((c["title"].as_str(), c["sub"].as_str()), (Some("Battery"), Some("On battery \u{b7} 3h 20m left")));
        assert_eq!((c["tile"]["text"].as_str(), c["tile"]["color"].as_str(), c["tile"]["vessel"].as_bool()), (Some("64"), Some(GREEN), Some(true)));
        assert!((c["tile"]["ring"].as_f64().unwrap() - 0.64).abs() < 0.001);
        assert_eq!((c["glance"]["big"].as_str(), c["glance"]["caption"].as_str()), (Some("64%"), Some("3h 20m left")));
        let kinds: Vec<&str> = c["body"].as_array().unwrap().iter().map(|b| b["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["chips", "stats"]);
        // the widgets
        assert_eq!(r["widgets"]["battery"]["blocks"][0]["vessel"]["shape"], "icon");
        assert_eq!(r["widgets"]["circle"]["blocks"][0]["vessel"]["text"], "64");
        assert_eq!(r["widgets"]["charge"]["blocks"][0]["spark"]["text"], "64%");
        // later, with the last hours, it has a graph
        let r = handle(&at(T0 + 600_000, battery(60.0), json!({}))).to_json();
        let kinds: Vec<&str> = r["card"]["body"].as_array().unwrap().iter().map(|b| b["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["chips", "graph", "stats"]);
    }

    #[test]
    fn the_liquid_is_amber_when_low_red_when_critical_and_green_when_it_charges() {
        assert_eq!((color_of(64.0, false, 20.0, 10.0), color_of(20.0, false, 20.0, 10.0), color_of(10.0, false, 20.0, 10.0)), (GREEN, AMBER, RED));
        assert_eq!(color_of(5.0, true, 20.0, 10.0), GREEN);
        // charging: the bolt, and how long to full
        fresh();
        let b = Battery { plugged: true, charging: true, minutes_to_full: Some(42), minutes_left: None, rate_w: Some(18.0), ..battery(45.0) };
        let r = handle(&at(T0, b, json!({}))).to_json();
        assert_eq!(r["card"]["tile"]["bolt"], true);
        assert_eq!(r["card"]["sub"], "Charging \u{b7} 42 min to full");
        assert_eq!(r["card"]["body"][0]["items"][0]["text"], "Plugged in");
    }

    #[test]
    fn it_says_when_it_runs_low_and_when_it_runs_out() {
        fresh();
        handle(&at(T0, battery(25.0), json!({})));
        // (the first look is not news; this crosses the low mark)
        let r = handle(&at(T0 + 60_000, battery(20.0), json!({}))).to_json();
        let s = &said(&r)[0];
        assert_eq!((s["text"].as_str(), s["priority"].as_i64(), s["pill"]["tone"].as_str(), s["pill"]["sub"].as_str()), (Some("Battery low"), Some(60), Some("low"), Some("20% \u{b7} about 3h 20m left")));
        assert_eq!((s["pill"]["tile"]["text"].as_str(), s["pill"]["tile"]["color"].as_str(), s["pill"]["tile"]["vessel"].as_bool()), (Some("20"), Some(AMBER), Some(true)));
        assert_eq!((s["pill"]["key"].as_str(), s["ambient"].as_bool()), (Some("battery"), Some(true)));
        // told once for the crossing
        assert!(said(&handle(&at(T0 + 120_000, battery(19.0), json!({}))).to_json()).is_empty());
        // critical rings through do not disturb
        let r = handle(&at(T0 + 180_000, battery(10.0), json!({}))).to_json();
        let s = &said(&r)[0];
        assert_eq!((s["text"].as_str(), s["priority"].as_i64(), s["pill"]["tone"].as_str()), (Some("Battery critical"), Some(90), Some("critical")));
        assert_eq!(s["pill"]["sub"], "10% \u{b7} about 3h 20m left \u{b7} plug in");
        // ... and once more at 5
        let r = handle(&at(T0 + 240_000, battery(5.0), json!({}))).to_json();
        assert_eq!(said(&r)[0]["text"], "Battery critical");
    }

    #[test]
    fn the_marks_are_the_settings_and_each_banner_can_be_off() {
        fresh();
        handle(&at(T0, battery(40.0), json!({})));
        let r = handle(&at(T0 + 60_000, battery(29.0), json!({ "low": 30 }))).to_json();
        assert_eq!(said(&r)[0]["text"], "Battery low");
        fresh();
        handle(&at(T0, battery(25.0), json!({})));
        assert!(said(&handle(&at(T0 + 60_000, battery(20.0), json!({ "b_low": false }))).to_json()).is_empty());
        fresh();
        handle(&at(T0, battery(25.0), json!({})));
        assert!(said(&handle(&at(T0 + 60_000, battery(20.0), json!({ "alerts": false }))).to_json()).is_empty());
    }

    #[test]
    fn the_charger_going_in_says_charging_and_a_full_battery_says_so() {
        fresh();
        handle(&at(T0, battery(42.0), json!({})));
        let plugged = Battery { plugged: true, charging: true, minutes_to_full: Some(42), minutes_left: None, rate_w: Some(18.0), ..battery(42.0) };
        let r = handle(&at(T0 + 10_000, plugged.clone(), json!({}))).to_json();
        let s = &said(&r)[0];
        assert_eq!((s["text"].as_str(), s["pill"]["tone"].as_str(), s["pill"]["sub"].as_str()), (Some("Charging"), Some("charging"), Some("42% \u{b7} about 42 min to full \u{b7} 18 W")));
        assert_eq!(s["pill"]["tile"]["bolt"], true);
        // unplugged: off until it is asked for
        let out = Battery { plugged: false, charging: false, ..battery(42.0) };
        assert!(said(&handle(&at(T0 + 20_000, out, json!({}))).to_json()).is_empty());
        // full
        fresh();
        handle(&at(T0, Battery { plugged: true, charging: true, ..battery(98.0) }, json!({})));
        let r = handle(&at(T0 + 60_000, Battery { plugged: true, charging: false, minutes_left: None, ..battery(100.0) }, json!({}))).to_json();
        assert_eq!((said(&r)[0]["text"].as_str(), said(&r)[0]["pill"]["sub"].as_str()), (Some("Fully charged"), Some("100% \u{b7} you can unplug")));
    }

    #[test]
    fn battery_saver_and_wear_are_off_until_asked_for_and_wear_is_said_once() {
        fresh();
        handle(&at(T0, battery(50.0), json!({})));
        let on = Battery { saver: true, ..battery(50.0) };
        assert!(said(&handle(&at(T0 + 10_000, on.clone(), json!({}))).to_json()).is_empty());
        fresh();
        handle(&at(T0, battery(50.0), json!({ "b_saver": true, "b_health": true })));
        let r = handle(&at(T0 + 10_000, on, json!({ "b_saver": true, "b_health": true }))).to_json();
        assert_eq!(said(&r)[0]["text"], "Battery saver on");
        // wear: the first time it is under 80% of new
        let worn = Battery { health: Some(62.0), full_wh: Some(30.0), design_wh: Some(48.0), ..battery(50.0) };
        let r = handle(&at(T0 + 20_000, worn.clone(), json!({ "b_health": true }))).to_json();
        assert_eq!(said(&r)[0]["pill"]["sub"], "Holds 62% of what it did new \u{b7} 30 Wh of 48");
        assert!(said(&handle(&at(T0 + 30_000, worn, json!({ "b_health": true }))).to_json()).is_empty());
    }

    #[test]
    fn a_pc_with_no_battery_says_so_once_and_shows_nothing() {
        fresh();
        let none = Battery { present: false, ..Battery::default() };
        let r = handle(&at(T0, none.clone(), json!({}))).to_json();
        assert!(r["card"].is_null());
        assert_eq!(r["note"], "No battery found: this PC runs on the wall.");
        assert!(handle(&at(T0 + 1000, none, json!({}))).to_json().get("note").is_none());
    }

    #[test]
    fn times_read_as_hours_and_minutes() {
        assert_eq!((dur(200), dur(48), dur(120), dur(61)), ("3h 20m".to_string(), "48 min".to_string(), "2h".to_string(), "1h 01m".to_string()));
    }
}
