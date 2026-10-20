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
/// the colour of the three bars of the sound (the plugin's tint)
const TINT: &str = "#c23a7a";

#[derive(Default)]
struct State {
    sessions: Vec<Session>,
    /// the cover pictures that were told, by player: (which, the picture)
    arts: HashMap<String, (u64, String)>,
    /// which picture the pill and the card hold now (a picture is sent once, then "keep")
    sent: Option<(String, u64)>,
    /// the setting: the sound shown as three bars (the island draws them, and they are gone while nothing is heard)
    viz: bool,
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
        return Reply::clear().no_pill().with_widget("song", Blocks::new(), Blocks::new()).with_widget("playpause", Blocks::new(), Blocks::new()).rest(&[]);
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
    // the card: the buttons in its header, and under it the seek bar with the jumps at its sides and the speed after them
    let mut card = Card::new().icon("music").tile(image.clone()).rank(CARD_RANK).title(s.title.clone()).sub(s.artist.clone()).controls(transport(&s));
    if st.viz {
        card = card.with_viz(TINT);
    }
    let mut back: Vec<Button> = Vec::new();
    let mut after: Vec<Button> = Vec::new();
    if s.can_seek {
        back.push(Button::new("Back 5 seconds", "back").icon("back"));
        after.push(Button::new("Forward 5 seconds", "fwd").icon("fwd"));
    }
    if s.can_rate {
        after.push(Button::new(rate_text(if s.rate > 0.0 { s.rate } else { 1.0 }), "rate"));
    }
    match progress {
        Some(p) => card = card.seek_with(p, "seek", back, after),
        // (no length to seek in: the buttons stand alone)
        None => {
            back.extend(after);
            if !back.is_empty() {
                card = card.buttons(back);
            }
        }
    }
    let others: Vec<(String, String, Option<f32>)> = st.sessions.iter().filter(|o| o.source != s.source && !o.title.is_empty()).map(|o| (o.title.clone(), o.artist.clone(), None)).collect();
    if !others.is_empty() {
        card = card.list(&others);
    }
    // the widgets of the resting island (its manifest's `rests`): the song, and the button; it is on while the music plays
    let song = if st.viz { Blocks::new().viz(16, TINT) } else { Blocks::new() }.item(&image, "music", &s.title, &s.artist);
    let button = Blocks::new().icon_button(if s.playing { "pause" } else { "play" }, "playpause", if s.playing { "Pause" } else { "Play" });
    let on: &[&str] = if s.playing { &["playing"] } else { &[] };
    Reply::card(card).pill(pill).with_widget("song", song, Blocks::new()).with_widget("playpause", button, Blocks::new()).rest(on)
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        // (what the setting says, as of this event: the next card is drawn with it)
        let viz = ev.setting_bool("visualizer", true);
        let changed = std::mem::replace(&mut st.viz, viz) != viz;
        match ev.event.as_str() {
            "start" => {
                st.sent = None;
                Reply::nothing()
            }
            // (the setting changed: what is on show is drawn again)
            _ if changed && !st.sessions.is_empty() => reply_for(&mut st),
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
                    // the widgets: the button (and a press on the song brings the player to the front)
                    "widget" => match ev.args["id"].as_str() {
                        Some("playpause") => Reply::nothing().media("play_pause", src),
                        Some("song") => Reply::nothing().media("focus", src),
                        _ => Reply::nothing(),
                    },
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
        assert_eq!(r["card"]["controls"][1]["icon"], "pause");
        assert_eq!(r["card"]["body"][0]["kind"], "seek");
        assert_eq!(r["card"]["body"][0]["value"].as_f64().map(|v| (v * 100.0).round()), Some(25.0));
        assert_eq!(r["card"]["body"][0]["before"][0]["action"], "back");
        assert_eq!(r["card"]["body"][0]["after"][1]["action"], "rate");
        // the other player is listed
        assert_eq!(r["card"]["body"][1]["rows"][0]["title"], "Paused");
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
    fn the_resting_island_is_on_while_the_music_plays() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let r = handle(&media(vec![session("b", "Playing", true)])).to_json();
        assert_eq!(r["rest"], json!(["playing"]));
        // (the bars of the sound, in front of the song)
        assert_eq!(r["widgets"]["song"]["blocks"][0], json!({ "viz": 16, "color": TINT }));
        assert_eq!(r["widgets"]["song"]["blocks"][1]["item"]["text"], "Playing");
        assert_eq!(r["widgets"]["song"]["blocks"][1]["item"]["tile"], "data:image/png;base64,AAAA");
        assert_eq!(r["widgets"]["playpause"]["blocks"][0]["icon_button"]["icon"], "pause");
        // paused: it is not on, and the button offers play
        let r = handle(&media(vec![session("b", "Playing", false)])).to_json();
        assert_eq!(r["rest"], json!([]));
        assert_eq!(r["widgets"]["playpause"]["blocks"][0]["icon_button"]["icon"], "play");
        // the button presses the player
        assert_eq!(handle(&action("widget", json!({ "id": "playpause" }))).to_json()["media"]["action"], "play_pause");
        // gone: nothing is on
        let r = handle(&media(vec![])).to_json();
        assert_eq!(r["rest"], json!([]));
    }

    #[test]
    fn the_sound_is_shown_as_three_bars_unless_the_setting_says_not() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let r = handle(&media(vec![session("b", "Playing", true)])).to_json();
        assert_eq!(r["card"]["viz"], TINT);
        // the setting off: no bars on the card or the widget, and what is on show is drawn again at once
        let mut off = action("noop", json!({}));
        off.settings = json!({ "visualizer": false });
        let r = handle(&off).to_json();
        assert!(r["card"].get("viz").is_none());
        assert_eq!(r["widgets"]["song"]["blocks"][0]["item"]["text"], "Playing");
        // on again
        let r = handle(&media(vec![session("b", "Playing", true)])).to_json();
        assert_eq!(r["card"]["viz"], TINT);
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
