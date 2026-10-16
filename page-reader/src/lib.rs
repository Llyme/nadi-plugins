//! Page Reader: "what are you browsing". A card in the expanded island for each page you have open (a walkthrough, a post, an
//! article, ...: a page of no known kind has none, and a video has the media card). It shows nothing on the collapsed island.
//!
//! The window title only names the page, so the rest comes from the NADI browser extension (the `nadi-chrome` repository): the page
//! describes itself, exactly, and costs the browser next to nothing. The island keeps the link to the extension and tells this module
//! what the pages are (`browser`); the module says which sites are never read and which buttons are safe to press (the extension
//! reads nothing until it is told), keeps the pages that have a card, works out what kind of page each is (`kind`), and draws the
//! card. A guide's words and a page's picture are too big to pass through here: the card names them, and the island fetches them.
//!
//! Private windows, blocked sites (banking, mail, health, your own list) and pages the extension did not describe (a sign-in page)
//! are never shown.

mod ext;
mod kind;
mod page;
mod text;

use ext::{ExtData, ExtMeta, ExtVideo, GuideLite, LdItem};
use kind::{Evidence, PageKind};
use nadi_plugin::*;
use page::{Node, NodeKind, Page};
use std::cell::RefCell;
use std::collections::HashMap;
use text::PagePreview;

/// how many pages get their own card, across all browsers: the ones focused last, and then the other open tabs the extension has read
const CARDS: usize = 8;
/// a page not focused for this long drops out of the list
const TTL_MS: u64 = 180_000;
/// the cards are shown while a browser window was seen this recently
const FRESH_MS: u64 = 30_000;
/// how often the open tabs are compared with the list, to drop closed ones
const PRUNE_MS: u64 = 5_000;
/// a page missing from every tab strip must miss the check for this long before it is dropped: mid-load titles flicker, closed tabs
/// do not come back
const PRUNE_GRACE_MS: u64 = 10_000;
const BROWSERS: &[&str] = &["chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "arc"];
const NAMES: &[(&str, &str)] = &[("msedge", "Edge"), ("chrome", "Chrome"), ("brave", "Brave"), ("opera", "Opera"), ("vivaldi", "Vivaldi")];

/// what is known of a page that has been read
#[derive(Clone, Default)]
struct Info {
    kind: Option<PageKind>,
    domain: Option<String>,
    preview: Option<PagePreview>,
    /// the page's main picture: an id the island fetches
    image: Option<u64>,
}

#[derive(Clone)]
struct Entry {
    /// the card's name: stays when the tab goes to another page of its guide
    id: u32,
    browser: String,
    title: String,
    /// the program's path and its name, from the window
    path: String,
    app: String,
    last_seen: u64,
    info: Option<Info>,
}

#[derive(Default)]
struct State {
    /// recently focused pages, most recent first, across all browser windows
    mru: Vec<Entry>,
    next_id: u32,
    /// the browser in front is a private / incognito window
    private: bool,
    /// a browser window was last seen (ms)
    seen: Option<u64>,
    /// the browsers that have a window now
    windows: Vec<String>,
    /// the browsers with the extension connected, as last told
    browsers: Vec<String>,
    last_prune: u64,
    /// pages missing from the tab lists, and since when
    absent: HashMap<(String, String), u64>,
    /// what the extension was last told to do (so it is told again only when it changes)
    told: Option<bool>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

// ------------------------------------------------------------------------------------------------------------ the titles

fn is_new_tab_or_empty(url: Option<&str>, title: &str) -> bool {
    const EMPTY_TITLES: &[&str] = &["new tab", "new tab page", "start page", "home", "about:blank", ""];
    let t = title.to_lowercase();
    if EMPTY_TITLES.contains(&t.as_str()) {
        return true;
    }
    if let Some(u) = url {
        let ul = u.to_lowercase();
        const NEW_TAB_URLS: &[&str] = &[
            "about:blank", "chrome://newtab", "chrome://new-tab-page", "chrome://home", "edge://newtab", "edge://new-tab-page", "brave://newtab", "vivaldi://newtab",
            "vivaldi://startpage", "opera://newtab", "opera://startpage", "firefox://newtab", "about:newtab", "about:home",
        ];
        if NEW_TAB_URLS.iter().any(|&prefix| ul.starts_with(prefix)) {
            return true;
        }
    }
    false
}

/// "Rust docs - Google Chrome - Michael" -> "Rust docs"
fn clean_title(title: &str) -> String {
    let lower = title.to_lowercase();
    let mut cut = title.len();
    for marker in [" - google chrome", " - microsoft edge", " - microsoft\u{200b} edge", " - brave", " - vivaldi", " - opera", " \u{2014} mozilla firefox", " - mozilla firefox"] {
        if let Some(i) = lower.find(marker) {
            cut = cut.min(i);
        }
    }
    let mut t = title[..cut].trim().to_string();
    // Edge and Chrome put the profile's name before their own: "Zeppelin - Wikipedia - Personal"
    if let Some(i) = t.rfind(" - ") {
        let profile = t[i + 3..].trim().to_lowercase();
        if matches!(profile.as_str(), "personal" | "work" | "school" | "default" | "guest") || profile.starts_with("profile ") {
            t.truncate(i);
        }
    }
    // Edge: "Page and 3 more pages - Profile" (the tab count and profile trail the title)
    if let Some(i) = t.rfind(" and ") {
        let mut words = t[i + 5..].split_whitespace();
        let counted = words.next().map_or(false, |n| n.chars().all(|c| c.is_ascii_digit()));
        if counted && words.next() == Some("more") {
            t.truncate(i);
        }
    }
    t
}

fn domain_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    (!host.is_empty()).then(|| host.to_lowercase())
}

/// "C:\...\msedge.exe" or "msedge.exe" -> "msedge"
fn stem(exe: &str) -> String {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_lowercase();
    file.strip_suffix(".exe").unwrap_or(&file).to_string()
}

fn is_browser(exe: &str) -> bool {
    BROWSERS.contains(&stem(exe).as_str())
}

// ------------------------------------------------------------------------------------------------------ the pages that have cards

impl State {
    /// Most-recent-first push with dedup and a cap.
    fn push(&mut self, browser: &str, title: &str, path: &str, app: &str, now: u64) {
        if let Some(pos) = self.mru.iter().position(|e| e.browser == browser && e.title == title) {
            let mut e = self.mru.remove(pos);
            e.last_seen = now;
            if !app.is_empty() {
                e.app = app.to_string();
            }
            if !path.is_empty() {
                e.path = path.to_string();
            }
            self.mru.insert(0, e);
            return;
        }
        self.next_id += 1;
        self.mru.insert(0, Entry { id: self.next_id, browser: browser.into(), title: title.into(), path: path.into(), app: app.into(), last_seen: now, info: None });
        self.mru.truncate(CARDS);
    }

    /// The tabs the extension has read are open, so they have cards as well, behind the ones that were visited: they go to the back
    /// of the list (a page that is already there keeps its place) while there is room.
    fn seed(&mut self, browser: &str, titles: &[String], now: u64) {
        let (path, app) = self.mru.iter().find(|e| e.browser == browser).map(|e| (e.path.clone(), e.app.clone())).unwrap_or_default();
        for t in titles {
            if let Some(e) = self.mru.iter_mut().find(|e| e.browser == browser && e.title == *t) {
                e.last_seen = now;
            } else if self.mru.len() < CARDS && !is_new_tab_or_empty(None, t) {
                self.next_id += 1;
                self.mru.push(Entry { id: self.next_id, browser: browser.into(), title: t.clone(), path: path.clone(), app: app.clone(), last_seen: now, info: None });
            }
        }
    }

    /// A tab went to another page of its guide: the card of the page it left is the card of the page it is on now, in the same place.
    fn rename(&mut self, browser: &str, old: &str, new: &str) {
        let mut i = 0;
        while i < self.mru.len() {
            if self.mru[i].title == old && self.mru[i].browser == browser {
                if self.mru.iter().any(|x| x.browser == browser && x.title == new) {
                    self.mru.remove(i); // (the new page is in the list already)
                    continue;
                }
                self.mru[i].title = new.to_string();
            }
            i += 1;
        }
    }

    fn fresh(&self, now: u64) -> bool {
        self.seen.map_or(false, |t| now.saturating_sub(t) <= FRESH_MS)
    }

    /// Look at the windows: each browser window's page leads the list in its turn, and the one in front last (so it leads). A private
    /// window never enters the list.
    fn observe(&mut self, windows: &[Window], now: u64) {
        let mut any = false;
        self.windows.clear();
        for w in windows.iter().rev().filter(|w| is_browser(&w.exe)) {
            any = true;
            let b = stem(&w.exe);
            if !self.windows.contains(&b) {
                self.windows.push(b.clone());
            }
            let cleaned = clean_title(&w.title);
            if kind::is_private_title(&w.title) || is_new_tab_or_empty(None, &cleaned) {
                continue;
            }
            let app = if w.name.is_empty() { b.clone() } else { w.name.clone() };
            self.push(&b, &cleaned, &w.path, &app, now);
        }
        if let Some(front) = windows.iter().find(|w| w.front && is_browser(&w.exe)) {
            self.private = kind::is_private_title(&front.title);
        }
        if any {
            self.seen = Some(now);
        }
    }

    /// closed tabs and windows drop out of the list: the extension lists every open tab, so a page no window names any more is gone
    /// (background tabs stay listed and are kept). Missing pages get a grace period first.
    fn prune(&mut self, feed: &BrowserFeed, now: u64) {
        if now.saturating_sub(self.last_prune) < PRUNE_MS {
            return;
        }
        self.last_prune = now;
        // an empty scan is a hiccup, not "everything closed": prune nothing
        if self.windows.is_empty() {
            return;
        }
        let keys: Vec<(String, String)> = self.mru.iter().map(|e| (e.browser.clone(), e.title.clone())).collect();
        let mut gone: Vec<(String, String)> = Vec::new();
        for (browser, title) in &keys {
            let open = if !self.windows.contains(browser) {
                false // no window of this browser left
            } else {
                match feed.tabs.get(browser) {
                    Some(list) if !list.is_empty() => list.iter().any(|t| *t == title.to_lowercase()),
                    _ => true, // no tab list (no extension): the time is the fallback
                }
            };
            let key = (browser.clone(), title.clone());
            if open {
                self.absent.remove(&key);
            } else if self.absent.get(&key).map_or(false, |t| now.saturating_sub(*t) >= PRUNE_GRACE_MS) {
                gone.push(key.clone());
                self.absent.remove(&key);
            } else {
                self.absent.entry(key).or_insert(now);
            }
        }
        self.absent.retain(|k, _| keys.contains(k));
        if !gone.is_empty() {
            self.mru.retain(|e| !gone.contains(&(e.browser.clone(), e.title.clone())));
        }
    }

    /// what the island is asked to tell: the pages that have cards
    fn watch(&self) -> Vec<(String, String)> {
        self.mru.iter().map(|e| (e.browser.clone(), e.title.clone())).collect()
    }
}

// ----------------------------------------------------------------------------------------------------- what the extension said

fn page_of(p: &Value) -> Page {
    let nodes = p["nodes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|n| {
                    let k = match n["k"].as_str()? {
                        "h" => NodeKind::Heading(n["l"].as_u64().unwrap_or(2).clamp(1, 6) as u8),
                        "p" => NodeKind::Text,
                        "li" => NodeKind::Item,
                        "b" => NodeKind::Button,
                        "a" => NodeKind::Link,
                        _ => return None,
                    };
                    let f = |key: &str| n[key].as_f64().unwrap_or(0.0) as f32;
                    Some(Node { kind: k, name: n["t"].as_str().unwrap_or("").to_string(), left: f("x"), top: f("y"), right: f("x") + f("w"), bottom: f("y") + f("h"), block: n["k"] == "p" })
                })
                .collect()
        })
        .unwrap_or_default();
    Page { url: p["url"].as_str().map(str::to_string), nodes, view: (0.0, p["vh"].as_f64().unwrap_or(0.0) as f32) }
}

fn ext_of(p: &Value) -> ExtData {
    ExtData {
        guide: serde_json::from_value::<GuideLite>(p["guide"].clone()).ok().filter(|g| !g.key.is_empty()),
        meta: serde_json::from_value::<ExtMeta>(p["meta"].clone()).unwrap_or_default(),
        ld: serde_json::from_value::<Vec<LdItem>>(p["ld"].clone()).unwrap_or_default(),
        video: serde_json::from_value::<ExtVideo>(p["video"].clone()).ok(),
        words: p["words"].as_u64().unwrap_or(0) as u32,
    }
}

/// What the extension described for a page that has a card.
fn read_page(st: &mut State, p: &Value) {
    let (browser, title) = (p["browser"].as_str().unwrap_or(""), p["title"].as_str().unwrap_or(""));
    let url = p["url"].as_str().unwrap_or("");
    let Some(i) = st.mru.iter().position(|e| e.browser == browser && e.title == title) else { return };
    if kind::is_blocked(Some(url), title) || is_new_tab_or_empty(Some(url), title) {
        st.mru[i].info = None;
        return;
    }
    let page = page_of(p);
    let data = ext_of(p);
    let ev = Evidence { url: Some(url), title, page: Some(&page), ext: Some(&data) };
    let info = Info { kind: kind::read(&ev), domain: domain_of(url), preview: text::preview_from_ext(&page), image: p["image"].as_u64() };
    let empty = info.kind.is_none() && info.domain.is_none() && info.preview.is_none() && info.image.is_none();
    st.mru[i].info = (!empty).then_some(info);
}

// ---------------------------------------------------------------------------------------------------------------- the cards

fn glyph(kind: &str) -> &'static str {
    match kind {
        "walkthrough" => "steps",
        "wiki" => "book",
        "news" => "news",
        "video" => "play",
        "recipe" => "pot",
        "qna" => "ask",
        "api" => "code",
        "product" => "tag",
        "search" => "search",
        "webapp" => "branch",
        _ => "globe",
    }
}

/// the kind's own summary line is a better lead than the page's first paragraph
fn has_summary(k: &PageKind) -> bool {
    k.fields.iter().any(|f| f.key == "Summary" || f.key == "Gist")
}

fn card_of(e: &Entry, info: &Info, k: &PageKind) -> Card {
    let domain = info.domain.clone().unwrap_or_default();
    let sub = if domain.is_empty() { k.label.to_string() } else { format!("{} \u{b7} {domain}", k.label) };
    let peek = k.peek.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| domain.clone());
    let mut card = Card::new().id(format!("p{}", e.id)).icon(glyph(k.id)).rank(60).title(e.title.clone()).sub(sub).peek(peek).focus_action("focus");
    if let Some(g) = &k.guide {
        // a guide: laid out to be read, with the picture beside it
        return card.reader(&g.key, info.image);
    }
    if let Some(id) = info.image {
        card = card.picture(id);
    }
    if !k.fields.is_empty() {
        let rows: Vec<(&str, String)> = k.fields.iter().map(|f| (f.key, f.value.clone())).collect();
        card = card.facts(&rows);
    }
    if let Some(p) = &info.preview {
        if let Some(lead) = p.lead.as_deref().filter(|_| !has_summary(k)) {
            card = card.text(lead);
        }
        if !p.buttons.is_empty() {
            card = card.buttons(p.buttons.iter().map(|b| Button::new(b.label.clone(), format!("press:{}", b.label))).collect());
        }
    }
    card
}

fn cards_of(st: &State, now: u64) -> Vec<Card> {
    if !st.fresh(now) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for e in st.mru.iter().filter(|e| now.saturating_sub(e.last_seen) <= TTL_MS).take(CARDS) {
        let Some(info) = &e.info else { continue };
        // (a page the island does not know the kind of is not worth a card: only a walkthrough, a post, an article, ... is. A video has
        // none either: the media card covers it)
        let Some(k) = info.kind.as_ref().filter(|k| k.id != "video") else { continue };
        out.push(card_of(e, info, k));
    }
    out
}

/// the line under the plugin's name
fn note_of(browsers: &[String]) -> String {
    if browsers.is_empty() {
        return "Waiting for the browser extension (without it only the window title is shown).".to_string();
    }
    let names: Vec<String> = browsers.iter().map(|x| NAMES.iter().find(|(k, _)| k == x).map_or(x.clone(), |(_, n)| n.to_string())).collect();
    format!("Reading pages in {}.", names.join(", "))
}

/// What the extension is told to do: which sites are never read, which buttons are safe, whether pictures are sent.
fn config(images: bool) -> Value {
    json!({ "hosts": kind::BLOCK_HOSTS, "hostWords": kind::BLOCK_HOST_WORDS, "titleWords": kind::BLOCK_TITLE_WORDS, "safe": text::SAFE_BUTTONS, "images": images })
}

fn render(st: &mut State, ev: &Event, extra: Option<(&str, Value)>) -> Reply {
    let now = ev.now;
    let mut cards = cards_of(st, now);
    let mut reply = if cards.is_empty() {
        Reply::clear().more_cards(Vec::new())
    } else {
        let first = cards.remove(0);
        Reply::card(first).more_cards(cards)
    };
    let mut ask = json!({ "watch": if st.private { Vec::new() } else { st.watch() } });
    // (the extension reads nothing until it is told; and told again when the setting changes)
    let images = ev.setting_bool("images", true);
    if st.told != Some(images) {
        st.told = Some(images);
        ask["config"] = config(images);
    }
    if let Some((k, v)) = extra {
        ask[k] = v;
    }
    reply = reply.browser(ask);
    reply.note(note_of(&st.browsers))
}

// ---------------------------------------------------------------------------------------------------------------------- the calls

fn handle(ev: &Event) -> Reply {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        kind::set_now(ev.now);
        match ev.event.as_str() {
            "start" => {
                *st = State::default();
                render(&mut st, ev, None)
            }
            "processes" => {
                st.observe(&ev.windows, ev.now);
                render(&mut st, ev, None)
            }
            "browser" => {
                let Some(feed) = &ev.browser else { return Reply::nothing() };
                st.browsers = feed.browsers.clone();
                for (browser, old, new) in &feed.moves {
                    st.rename(browser, old, new);
                }
                // the tabs that are open but were never in front have cards too
                if !st.private {
                    for b in st.windows.clone() {
                        if let Some(list) = feed.open.get(&b) {
                            st.seed(&b, list, ev.now);
                        }
                    }
                }
                st.prune(feed, ev.now);
                // a private window in front: nothing is read meanwhile, and the other cards stay as they are
                if !st.private {
                    for p in &feed.pages {
                        read_page(&mut st, p);
                    }
                }
                render(&mut st, ev, None)
            }
            "action" => {
                // (the card that was pressed; the page in front when it is not said)
                let card = ev.args["card"].as_str().unwrap_or("");
                let Some(e) = st.mru.iter().find(|e| format!("p{}", e.id) == card).or_else(|| st.mru.first()).cloned() else { return Reply::nothing() };
                match ev.action.as_str() {
                    // the card's icon: the browser window the page is in
                    "focus" => render(&mut st, ev, None).focus(Some(&e.path).filter(|p| !p.is_empty()).map(String::as_str), Some(&e.title)),
                    // the tab goes to another section of the guide (within the same site); the card follows the tab
                    "go" => {
                        let url = ev.args["url"].as_str().unwrap_or("");
                        render(&mut st, ev, Some(("go", json!({ "id": "go", "browser": e.browser, "title": e.title, "url": url }))))
                    }
                    // a button of the page: the extension presses the page's button of that name
                    a if a.starts_with("press:") => render(&mut st, ev, Some(("press", json!({ "id": "press", "browser": e.browser, "label": &a["press:".len()..] })))),
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

    const T0: u64 = 1_800_000_000_000;

    fn win(exe: &str, title: &str, front: bool) -> Window {
        Window { exe: exe.into(), title: title.into(), front, path: format!("C:\\Apps\\{exe}"), name: "Edge".into(), ..Window::default() }
    }

    fn fresh() {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }

    #[test]
    fn titles_lose_the_browser_suffix_and_the_profile_name() {
        assert_eq!(clean_title("Rust docs - Google Chrome"), "Rust docs");
        assert_eq!(clean_title("Inbox and 3 more pages - Personal - Microsoft\u{200b} Edge"), "Inbox");
        assert_eq!(clean_title("Zeppelin - Wikipedia - Personal - Microsoft\u{200b} Edge"), "Zeppelin - Wikipedia");
        assert_eq!(domain_of("https://www.Example.com:8080/a?b#c").as_deref(), Some("example.com"));
        assert_eq!(domain_of("chrome://settings"), None);
        assert_eq!(stem("C:\\x\\MSEdge.exe"), "msedge");
    }

    #[test]
    fn the_extension_is_told_what_to_read_once_and_the_pages_that_have_cards_are_watched() {
        fresh();
        let r = handle(&Event { event: "start".into(), now: T0, settings: json!({}), ..Event::default() }).to_json();
        assert_eq!(r["browser"]["config"]["images"], true);
        assert!(r["browser"]["config"]["hosts"].as_array().unwrap().iter().any(|h| h == "paypal.com"));
        assert!(r["browser"]["config"]["safe"].as_array().unwrap().iter().any(|h| h == "share"));
        assert_eq!(r["note"], "Waiting for the browser extension (without it only the window title is shown).");
        // a browser window: its page is watched; the config is not sent again
        let ev = Event { event: "processes".into(), now: T0 + 2000, settings: json!({}), windows: vec![win("msedge.exe", "Zeppelin - Wikipedia - Microsoft\u{200b} Edge", true)], ..Event::default() };
        let r = handle(&ev).to_json();
        assert_eq!(r["browser"]["watch"], json!([["msedge", "Zeppelin - Wikipedia"]]));
        assert!(r["browser"].get("config").is_none());
        // the setting changes: told again
        let ev = Event { settings: json!({ "images": false }), ..ev };
        assert_eq!(handle(&ev).to_json()["browser"]["config"]["images"], false);
    }

    #[test]
    fn a_page_the_extension_described_as_a_wiki_has_a_card_and_a_private_window_has_none() {
        fresh();
        let ev = |e: &str, windows: Vec<Window>, now: u64| Event { event: e.into(), now, settings: json!({}), windows, ..Event::default() };
        handle(&ev("start", vec![], T0));
        handle(&ev("processes", vec![win("msedge.exe", "Zeppelin - Wikipedia - Microsoft\u{200b} Edge", true)], T0 + 2000));
        let page = json!({ "browser": "msedge", "title": "Zeppelin - Wikipedia", "url": "https://en.wikipedia.org/wiki/Zeppelin", "version": 3, "vh": 800, "words": 5000,
            "meta": { "ogType": "article", "site": "Wikipedia", "description": "A zeppelin is a type of rigid airship." }, "ld": [], "video": null, "guide": null, "image": 7, "nodes": [] });
        let feed = BrowserFeed { browsers: vec!["msedge".into()], pages: vec![page], ..BrowserFeed::default() };
        let r = handle(&Event { event: "browser".into(), now: T0 + 3000, settings: json!({}), browser: Some(feed.clone()), ..Event::default() }).to_json();
        let c = &r["card"];
        assert_eq!(c["title"], "Zeppelin - Wikipedia");
        assert_eq!(c["icon"], "book");
        assert_eq!(c["sub"], "Wiki article \u{b7} en.wikipedia.org");
        assert_eq!(c["focus"], "focus");
        assert_eq!(c["body"][0], json!({ "kind": "picture", "id": 7 }));
        assert_eq!(r["note"], "Reading pages in Edge.");
        // a private window in front: nothing is read and nothing is watched
        let private = ev("processes", vec![win("msedge.exe", "Zeppelin - InPrivate - Microsoft\u{200b} Edge", true)], T0 + 5000);
        let r = handle(&private).to_json();
        assert_eq!(r["browser"]["watch"], json!([]));
        // a page of no known kind has none, and no page is read before the browser was seen
        fresh();
        handle(&ev("processes", vec![win("msedge.exe", "Some page - Microsoft\u{200b} Edge", true)], T0));
        let plain = json!({ "browser": "msedge", "title": "Some page", "url": "https://example.com/", "version": 1, "vh": 800, "words": 10, "meta": {}, "ld": [], "nodes": [] });
        let feed = BrowserFeed { browsers: vec!["msedge".into()], pages: vec![plain], ..BrowserFeed::default() };
        let r = handle(&Event { event: "browser".into(), now: T0 + 1000, settings: json!({}), browser: Some(feed), ..Event::default() }).to_json();
        assert_eq!(r["card"], Value::Null);
    }

    #[test]
    fn a_card_follows_its_tab_to_the_next_page_and_keeps_its_name() {
        let mut st = State::default();
        st.push("msedge", "Day 1", "C:/b/msedge.exe", "Edge", T0);
        st.push("msedge", "Other", "C:/b/msedge.exe", "Edge", T0);
        let id = st.mru.iter().find(|e| e.title == "Day 1").unwrap().id;
        st.rename("msedge", "Day 1", "Day 2");
        let e = st.mru.iter().find(|e| e.title == "Day 2").unwrap();
        assert_eq!(e.id, id);
        assert_eq!(st.mru.len(), 2);
        // the new page was in the list already: the old card just goes
        st.rename("msedge", "Day 2", "Other");
        assert_eq!(st.mru.len(), 1);
    }

    #[test]
    fn closed_tabs_drop_out_after_a_grace_and_open_ones_stay() {
        let mut st = State::default();
        st.windows = vec!["chrome".into()];
        st.push("chrome", "Page A", "", "Chrome", T0);
        st.push("chrome", "Page B", "", "Chrome", T0);
        let feed = BrowserFeed { tabs: [("chrome".to_string(), vec!["page b".to_string()])].into(), ..BrowserFeed::default() };
        st.prune(&feed, T0 + 10_000);
        assert_eq!(st.mru.len(), 2, "it is missed once: a grace first");
        st.prune(&feed, T0 + 21_000);
        assert_eq!(st.mru.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Page B"]);
        // no window of the browser left: those go too, after the grace
        st.windows.clear();
        st.prune(&feed, T0 + 40_000);
        assert_eq!(st.mru.len(), 1, "an empty scan is a hiccup, not everything closed");
    }

    #[test]
    fn the_list_keeps_the_most_recent_across_browsers() {
        let mut st = State::default();
        for i in 0..CARDS + 2 {
            st.push(["chrome", "msedge", "brave"][i % 3], &format!("P{i}"), "", "App", T0);
        }
        let titles: Vec<String> = st.mru.iter().map(|e| e.title.clone()).collect();
        let want: Vec<String> = (2..CARDS + 2).rev().map(|i| format!("P{i}")).collect();
        assert_eq!(titles, want);
    }
}
