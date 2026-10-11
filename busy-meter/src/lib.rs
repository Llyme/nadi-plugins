//! Busy meter: a ring in the hub for how busy the PC is (the busier of the processor and the memory), and a pill on the
//! island when the processor stays above a limit for a few readings. The pill has a button that snoozes it.
//!
//! It shows what a plugin can do beyond a card: it reads the `system` sensor, adds a ring, offers a pill, answers a
//! button, and opens a window of its own (the pill's second button asks for how long to snooze). It sees only two numbers.

use nadi_plugin::*;
use std::cell::RefCell;

/// readings in a row above the limit before the pill comes (a reading is 2 s)
const READINGS: u32 = 3;
const SNOOZE_MS: u64 = 10 * 60_000;

#[derive(Default)]
struct State {
    over: u32,
    snoozed_until: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn colour(busy: f32) -> &'static str {
    if busy >= 90.0 {
        "#e87a7a"
    } else if busy >= 70.0 {
        "#e8b04a"
    } else {
        "#5ac88c"
    }
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        match ev.event.as_str() {
            "action" if ev.action == "snooze" => {
                s.snoozed_until = ev.now + SNOOZE_MS;
                s.over = 0;
                Reply::nothing().no_pill()
            }
            // the second button: a window, to say for how long
            "action" if ev.action == "ask" => Reply::modal(
                Modal::new("Snooze the busy pill")
                    .text("For how long should it stay away?")
                    .choice("for", &[("10", "10 minutes"), ("60", "1 hour"), ("480", "8 hours")], "10")
                    .button("cancel", "Cancel")
                    .primary("snooze", "Snooze"),
            ),
            // what the window's buttons say
            "action" if ev.action == "modal" => {
                if ev.args["button"] != "snooze" {
                    return Reply::nothing();
                }
                let minutes: u64 = ev.args["values"]["for"].as_str().and_then(|m| m.parse().ok()).unwrap_or(10).min(24 * 60);
                s.snoozed_until = ev.now + minutes * 60_000;
                s.over = 0;
                Reply::nothing().no_pill()
            }
            "system" => {
                let Some(sys) = ev.system.as_ref() else { return Reply::nothing() };
                let busy = sys.cpu.max(sys.ram);
                let limit = ev.setting_f64("limit", 90.0) as f32;
                s.over = if sys.cpu > limit { s.over + 1 } else { 0 };
                let ring = Ring::new("busy", busy, format!("Busy · {:.0}%\nCPU {:.0}% · RAM {:.0}%", busy, sys.cpu, sys.ram), colour(busy));
                let r = Reply::nothing().rings(vec![ring]);
                if s.over >= READINGS && ev.now >= s.snoozed_until {
                    r.pill(
                        Pill::new()
                            .icon("timer")
                            .title("The processor is busy")
                            .sub(format!("{:.0}% for {} s", sys.cpu, s.over * 2))
                            .right(format!("{:.0}%", sys.cpu))
                            .rank(ev.setting_f64("rank", 5.0) as i32)
                            .buttons(vec![Button::new("Snooze for 10 minutes", "snooze").icon("x"), Button::new("Choose how long", "ask")]),
                    )
                } else {
                    r.no_pill()
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

    fn system(cpu: f32, ram: f32, now: u64) -> Event {
        Event {
            event: "system".into(),
            now,
            settings: json!({ "limit": 90 }),
            system: Some(System { cpu, ram, ram_used_gb: 8.0, ram_total_gb: 16.0, gpu: None }),
            ..Event::default()
        }
    }

    #[test]
    fn the_second_button_opens_a_window_and_its_answer_snoozes_for_as_long_as_it_says() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let ask = Event { event: "action".into(), action: "ask".into(), now: 1000, ..Event::default() };
        let r = handle(&ask).to_json();
        assert_eq!(r["modal"]["title"], "Snooze the busy pill");
        assert_eq!(r["modal"]["blocks"][1]["options"][1]["id"], "60");
        assert_eq!(r["modal"]["buttons"][1]["primary"], true);
        // "Cancel" changes nothing
        let cancel = Event { event: "action".into(), action: "modal".into(), args: json!({ "button": "cancel", "values": { "for": "60" }, "closed": true }), now: 2000, ..Event::default() };
        assert_eq!(handle(&cancel).to_json()["pill"], Value::Null);
        STATE.with(|s| assert_eq!(s.borrow().snoozed_until, 0));
        // "Snooze" for an hour
        let snooze = Event { event: "action".into(), action: "modal".into(), args: json!({ "button": "snooze", "values": { "for": "60" }, "closed": true }), now: 3000, ..Event::default() };
        handle(&snooze);
        STATE.with(|s| assert_eq!(s.borrow().snoozed_until, 3000 + 60 * 60_000));
    }

    #[test]
    fn a_pill_comes_after_a_few_busy_readings_and_a_button_snoozes_it() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        // the ring follows the busier of the two numbers
        let r = handle(&system(30.0, 55.0, 0)).to_json();
        assert_eq!(r["rings"][0]["pct"], 55.0);
        assert_eq!(r["rings"][0]["color"], "#5ac88c");
        assert_eq!(r["pill"], Value::Null);
        // two busy readings are not enough, the third brings the pill
        assert_eq!(handle(&system(95.0, 20.0, 2000)).to_json()["pill"], Value::Null);
        assert_eq!(handle(&system(95.0, 20.0, 4000)).to_json()["pill"], Value::Null);
        let r = handle(&system(95.0, 20.0, 6000)).to_json();
        assert_eq!(r["pill"]["title"], "The processor is busy");
        assert_eq!(r["pill"]["buttons"][0]["action"], "snooze");
        assert_eq!(r["pill"]["rank"], 5);
        // the button takes it away, and it stays away for ten minutes
        let press = Event { event: "action".into(), action: "snooze".into(), now: 7000, ..Event::default() };
        assert_eq!(handle(&press).to_json()["pill"], Value::Null);
        for i in 0..5 {
            assert_eq!(handle(&system(95.0, 20.0, 8000 + i * 2000)).to_json()["pill"], Value::Null);
        }
        // ... then it may come again
        for i in 0..3 {
            handle(&system(95.0, 20.0, 7000 + SNOOZE_MS + i * 2000));
        }
        assert_eq!(handle(&system(95.0, 20.0, 7000 + SNOOZE_MS + 6000)).to_json()["pill"]["right"], "95%");
        // quiet again: the pill goes
        assert_eq!(handle(&system(10.0, 20.0, 9000 + SNOOZE_MS)).to_json()["pill"], Value::Null);
    }
}
