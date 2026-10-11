//! Now playing: a pill on the collapsed island and a card in the hub for what is playing, with play, skip, a bar to seek on,
//! a jump of 5 seconds each way, and the playback speed. It reads the `media` sensor (with the cover picture) and presses the
//! player's buttons through `media_control`.
//!
//! The pill and the card follow one player: the one that is playing, else the first. The card lists the others.

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// the playback speeds the speed window offers
const RATES: [f64; 13] = [0.1, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0, 8.0, 16.0];
/// the card leads the hub's list, the pill sits below the games'
const CARD_RANK: i32 = 10;
const PILL_RANK: i32 = 30;

#[derive(Default)]
struct State {
    sessions: Vec<Session>,
    /// the cover pictures that were told, by player: (which, the picture)
    arts: HashMap<String, (u64, String)>,
    /// which picture the pill and the card hold now (a picture is sent once, then "keep")
    sent: Option<(String, u64)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// the player the pill and the card follow: the one playing, else the first that has a title
fn primary(sessions: &[Session]) -> Option<&Session> {
    sessions.iter().find(|s| s.playing).or_else(|| sessions.iter().find(|s| !s.title.is_empty())).or_else(|| sessions.first())
}

fn rate_text(r: f64) -> String {
    format!("{}×", (r * 100.0).round() / 100.0)
}

/// the transport buttons (previous, play or pause, next)
fn transport(s: &Session) -> Vec<Button> {
    let mut b = Vec::new();
    if s.can_previous {
        b.push(Button::new("Previous", "prev").icon("prev"));
    }
    b.push(Button::new(if s.playing { "Pause" } else { "Play" }, "toggle").icon(if s.playing { "pause" } else { "play" }));
    if s.can_next {
        b.push(Button::new("Next", "next").icon("next"));
    }
    b
}

fn reply_for(st: &mut State) -> Reply {
    let Some(s) = primary(&st.sessions).cloned() else {
        st.sent = None;
        return Reply::clear().no_pill();
    };
    // the picture: told with the first reply that has it, then kept
    let image = match st.arts.get(&s.source) {
        Some((id, art)) if *id == s.art_id => {
            if st.sent.as_ref() == Some(&(s.source.clone(), *id)) {
                "keep".to_string()
            } else {
                st.sent = Some((s.source.clone(), *id));
                art.clone()
            }
        }
        _ => {
            st.sent = None;
            String::new()
        }
    };
    let progress = (s.duration > 0.0).then(|| (s.position / s.duration).clamp(0.0, 1.0) as f32);
    let mut pill = Pill::new().icon("music").image(image.clone()).title(s.title.clone()).sub(s.artist.clone()).rank(PILL_RANK);
    pill.buttons = transport(&s);
    if let Some(p) = progress {
        pill = pill.progress(p);
    }
    let mut card = Card::new().icon("music").tile(image).rank(CARD_RANK).title(s.title.clone()).sub(s.artist.clone());
    card.body.push(json!({ "kind": "buttons", "items": transport(&s) }));
    if let Some(p) = progress {
        card = card.seek(p, "seek");
    }
    let mut more: Vec<Button> = Vec::new();
    if s.can_seek {
        more.push(Button::new("Back 5 seconds", "back").icon("back"));
        more.push(Button::new("Forward 5 seconds", "fwd").icon("fwd"));
    }
    if s.can_rate {
        more.push(Button::new(rate_text(if s.rate > 0.0 { s.rate } else { 1.0 }), "rate"));
    }
    if !more.is_empty() {
        card.body.push(json!({ "kind": "buttons", "items": more }));
    }
    let others: Vec<(String, String, Option<f32>)> = st.sessions.iter().filter(|o| o.source != s.source && !o.title.is_empty()).map(|o| (o.title.clone(), o.artist.clone(), None)).collect();
    if !others.is_empty() {
        card = card.list(&others);
    }
    Reply::card(card).pill(pill)
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        match ev.event.as_str() {
            "start" => {
                st.sent = None;
                Reply::nothing()
            }
            "media" => {
                // (what was told of a picture is kept; the players that are gone take theirs with them)
                for s in &ev.sessions {
                    if let Some(a) = &s.art {
                        st.arts.insert(s.source.clone(), (s.art_id, a.clone()));
                    }
                }
                st.arts.retain(|k, _| ev.sessions.iter().any(|s| &s.source == k));
                st.sessions = ev.sessions.clone();
                reply_for(&mut st)
            }
            "action" => {
                let Some(s) = primary(&st.sessions).cloned() else { return Reply::nothing() };
                let src = s.source.clone();
                match ev.action.as_str() {
                    "toggle" => Reply::nothing().media("play_pause", src),
                    "prev" => Reply::nothing().media("previous", src),
                    "next" => Reply::nothing().media("next", src),
                    "back" => Reply::nothing().media_to("seek_by", src, -5.0),
                    "fwd" => Reply::nothing().media_to("seek_by", src, 5.0),
                    "seek" if s.duration > 0.0 => {
                        let f = ev.args["fraction"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
                        Reply::nothing().media_to("seek", src, f * s.duration)
                    }
                    "rate" => {
                        let now = if s.rate > 0.0 { s.rate } else { 1.0 };
                        let rows: Vec<(String, String, String)> = RATES.iter().map(|r| (r.to_string(), rate_text(*r), String::new())).collect();
                        let rows: Vec<(&str, &str, &str)> = rows.iter().map(|(i, l, s)| (i.as_str(), l.as_str(), s.as_str())).collect();
                        Reply::modal(
                            Modal::new("Playback speed")
                                .width(280)
                                .list("rate", &rows, &RATES.iter().find(|r| (**r - now).abs() < 0.01).map(|r| r.to_string()).unwrap_or_else(|| "1".into()))
                                .button("cancel", "Cancel")
                                .primary("set", "Set"),
                        )
                    }
                    // the window's answer: the speed picked
                    "modal" if ev.args["button"] == "set" => {
                        let rate: f64 = ev.args["values"]["rate"].as_str().and_then(|r| r.parse().ok()).unwrap_or(1.0);
                        Reply::nothing().media_to("set_rate", src, rate)
                    }
                    // the card's tile: the player is brought to the front
                    "focus" => Reply::nothing().media("focus", src),
                    _ => Reply::nothing(),
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

    fn session(source: &str, title: &str, playing: bool) -> Session {
        Session { source: source.into(), title: title.into(), artist: "Artist".into(), playing, position: 30.0, duration: 120.0, can_previous: true, can_next: true, can_seek: true, can_rate: true, rate: 1.5, art_id: 7, art: Some("data:image/png;base64,AAAA".into()), ..Session::default() }
    }

    fn media(sessions: Vec<Session>) -> Event {
        Event { event: "media".into(), sessions, ..Event::default() }
    }

    fn action(a: &str, args: Value) -> Event {
        Event { event: "action".into(), action: a.into(), args, ..Event::default() }
    }

    #[test]
    fn the_pill_and_the_card_follow_the_player_that_plays() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let r = handle(&media(vec![session("a", "Paused", false), session("b", "Playing", true)])).to_json();
        assert_eq!(r["pill"]["title"], "Playing");
        assert_eq!(r["pill"]["buttons"][1]["icon"], "pause");
        assert_eq!(r["pill"]["image"], "data:image/png;base64,AAAA");
        assert_eq!(r["card"]["title"], "Playing");
        assert_eq!(r["card"]["body"][1]["kind"], "seek");
        assert_eq!(r["card"]["body"][1]["value"].as_f64().map(|v| (v * 100.0).round()), Some(25.0));
        // the other player is listed
        assert_eq!(r["card"]["body"][3]["rows"][0]["title"], "Paused");
        // the picture is sent once, then kept
        let r = handle(&media(vec![session("a", "Paused", false), session("b", "Playing", true)])).to_json();
        assert_eq!(r["pill"]["image"], "keep");
        assert_eq!(r["card"]["image"], "keep");
        // nothing playing: the pill and the card go
        let r = handle(&media(vec![])).to_json();
        assert_eq!(r["pill"], Value::Null);
        assert_eq!(r["card"], Value::Null);
    }

    #[test]
    fn the_buttons_press_the_player() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        handle(&media(vec![session("b", "Playing", true)]));
        assert_eq!(handle(&action("toggle", json!({}))).to_json()["media"]["action"], "play_pause");
        assert_eq!(handle(&action("next", json!({}))).to_json()["media"]["action"], "next");
        let back = handle(&action("back", json!({}))).to_json();
        assert_eq!((back["media"]["action"].as_str(), back["media"]["value"].as_f64()), (Some("seek_by"), Some(-5.0)));
        let seek = handle(&action("seek", json!({ "fraction": 0.5 }))).to_json();
        assert_eq!((seek["media"]["action"].as_str(), seek["media"]["value"].as_f64()), (Some("seek"), Some(60.0)));
        assert_eq!(seek["media"]["source"], "b");
        // the speed: a window, then the answer
        let ask = handle(&action("rate", json!({}))).to_json();
        assert_eq!(ask["modal"]["title"], "Playback speed");
        assert_eq!(ask["modal"]["blocks"][0]["value"], "1.5");
        let set = handle(&action("modal", json!({ "button": "set", "values": { "rate": "2" } }))).to_json();
        assert_eq!((set["media"]["action"].as_str(), set["media"]["value"].as_f64()), (Some("set_rate"), Some(2.0)));
        let cancel = handle(&action("modal", json!({ "button": "cancel" }))).to_json();
        assert!(cancel.get("media").is_none());
    }
}
