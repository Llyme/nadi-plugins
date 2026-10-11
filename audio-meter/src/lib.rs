//! Audio meter: what is playing, as bands. A card that moves with the sound (a live card: the island draws it as it
//! changes). The module hears levels and bands, never the sound itself, and cannot reach the network.

use nadi_plugin::*;
use std::cell::RefCell;

thread_local! {
    // what the bars show: they rise at once and fall slowly, so the meter reads as a meter
    static BARS: RefCell<[f32; 16]> = RefCell::new([0.0; 16]);
    static LAST_BEAT: RefCell<u64> = RefCell::new(0);
}

fn handle(ev: &Event) -> Reply {
    let Some(a) = &ev.audio else {
        return Reply::card(Card::new().icon("wave").title("Waiting for sound").sub("Nothing is playing"));
    };
    let bars = BARS.with(|b| {
        let mut b = b.borrow_mut();
        for (i, v) in b.iter_mut().enumerate() {
            let now = a.spectrum.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            *v = if now > *v { now } else { *v * 0.82 + now * 0.18 };
        }
        *b
    });
    if a.beat {
        LAST_BEAT.with(|l| *l.borrow_mut() = ev.now);
    }
    let kind = match a.kind.as_str() {
        "music" => "Music",
        "speech" => "Voices",
        _ => "Silence",
    };
    let sub = if ev.setting_bool("show_bpm", true) && a.bpm > 40.0 && a.kind == "music" {
        format!("{kind} · {} bpm", a.bpm.round())
    } else {
        kind.to_string()
    };
    Reply::card(Card::new().icon("wave").title("Audio").sub(sub).peek(format!("{}%", (a.level * 100.0).round())).bars(&bars))
}

plugin!(handle);
