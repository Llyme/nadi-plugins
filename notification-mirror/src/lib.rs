//! Notification mirror: other apps' Windows notifications (Discord, Viber, ...) shown on the island as banners. Pressing a
//! banner opens the app it came from.
//!
//! It reads the `notifications` sensor (what Windows shows as a toast). Viber draws a popup of its own instead of a toast, with
//! no text to read, so for Viber it only notices that the popup came (a small tool window titled "ViberPC") and says that
//! there is a new message.

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashSet;

#[derive(Default)]
struct State {
    /// the Viber popups that were there at the last look
    popups: HashSet<i64>,
    primed: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// what pressing a banner opens, and the name a pile of them goes under, by the app that sent it
fn link_for(app: &str) -> (&'static str, &'static str) {
    let a = app.to_lowercase();
    if a.contains("discord") {
        ("discord://", "Discord")
    } else if a.contains("viber") {
        ("viber://chats", "")
    } else {
        ("", "")
    }
}

/// the text of the banner: the title (the app's name if there is none), and the body (the app's name if only a title came)
fn words(t: &Toast) -> (String, String) {
    let title = if t.title.is_empty() { t.app.clone() } else { t.title.clone() };
    let body = if t.title.is_empty() || t.body.is_empty() {
        if t.title.is_empty() {
            t.body.clone()
        } else {
            t.app.clone()
        }
    } else {
        t.body.clone()
    };
    (title, body)
}

/// Viber's popup: a tool window titled "ViberPC", small (the big one is the chat window)
fn viber_popups(windows: &[Window]) -> HashSet<i64> {
    windows.iter().filter(|w| w.exe == "viber.exe" && w.title == "ViberPC" && w.w > 50 && w.w < 600).map(|w| w.id).collect()
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let mut r = Reply::nothing();
        match ev.event.as_str() {
            "start" => {
                s.popups.clear();
                s.primed = false;
            }
            "notifications" => {
                for t in &ev.notifications {
                    let (title, body) = words(t);
                    let (open, group) = link_for(&t.app);
                    let mut say = Say::new(body, 0).titled(title).opens(open);
                    if !group.is_empty() {
                        say = say.grouped(group);
                    }
                    r = r.say_with(say);
                }
            }
            "processes" => {
                let now = if ev.setting_bool("viber", true) { viber_popups(&ev.windows) } else { HashSet::new() };
                // (what was open when it started is not news)
                let fresh = s.primed && now.iter().any(|id| !s.popups.contains(id));
                s.popups = now;
                s.primed = true;
                if fresh {
                    r = r.say_with(Say::new("New message", 0).titled("Viber").opens("viber://chats").grouped("Viber"));
                }
            }
            _ => {}
        }
        r
    })
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    fn toast(app: &str, title: &str, body: &str) -> Toast {
        Toast { seq: 1, app: app.into(), title: title.into(), body: body.into() }
    }

    fn windows(ids: &[i64]) -> Vec<Window> {
        ids.iter().map(|i| Window { exe: "viber.exe".into(), title: "ViberPC".into(), w: 300, h: 80, id: *i, tool: true, ..Window::default() }).collect()
    }

    #[test]
    fn a_toast_becomes_a_banner_that_opens_its_app() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let ev = Event { event: "notifications".into(), notifications: vec![toast("Discord", "Sam", "are you there?"), toast("Mail", "", "A body alone")], ..Event::default() };
        let r = handle(&ev).to_json();
        let say = r["say"].as_array().unwrap();
        assert_eq!((say[0]["title"].as_str(), say[0]["text"].as_str(), say[0]["open"].as_str(), say[0]["group"].as_str()), (Some("Sam"), Some("are you there?"), Some("discord://"), Some("Discord")));
        // no title: the app's name is the title, and the text is what came
        assert_eq!((say[1]["title"].as_str(), say[1]["text"].as_str()), (Some("Mail"), Some("A body alone")));
        assert!(say[1].get("open").is_none());
    }

    #[test]
    fn the_viber_popup_is_noticed_once_and_what_was_there_at_the_start_is_not_news() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let look = |ids: &[i64], viber: bool| Event { event: "processes".into(), windows: windows(ids), settings: json!({ "viber": viber }), ..Event::default() };
        assert_eq!(handle(&look(&[7], true)).to_json()["say"].as_array().unwrap().len(), 0, "already there at the start");
        assert_eq!(handle(&look(&[7, 8], true)).to_json()["say"][0]["title"], "Viber");
        assert_eq!(handle(&look(&[7, 8], true)).to_json()["say"].as_array().unwrap().len(), 0, "told once");
        // the popup is gone, and another one comes
        handle(&look(&[], true));
        assert_eq!(handle(&look(&[9], true)).to_json()["say"].as_array().unwrap().len(), 1);
        // switched off in the settings: nothing
        handle(&look(&[], false));
        assert_eq!(handle(&look(&[10], false)).to_json()["say"].as_array().unwrap().len(), 0);
        // the big chat window is not a popup
        let big = Event { event: "processes".into(), windows: vec![Window { exe: "viber.exe".into(), title: "ViberPC".into(), w: 1200, h: 800, id: 11, ..Window::default() }], settings: json!({ "viber": true }), ..Event::default() };
        assert_eq!(handle(&big).to_json()["say"].as_array().unwrap().len(), 0);
    }
}
