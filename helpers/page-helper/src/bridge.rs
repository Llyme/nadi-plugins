//! The browser extension's side of the bridge (the extension is the `nadi-chrome` repo), in the NADI Chrome plugin's own program.
//!
//! The extension keeps a WebSocket open to this app on 127.0.0.1 and tells it about the pages open in the browser: the headings,
//! short paragraphs, buttons and links with where they sit, the page's own metadata (JSON-LD, Open Graph), its video, and how far
//! down you are. That is exact, and it costs the browser next to nothing. It is the only way a page is read: without the
//! extension, a browser is not read at all.
//!
//! Nothing is accepted from a web page: the WebSocket handshake must carry the `Origin` of a browser extension (a web page cannot
//! forge one), and the extension sends nothing until this app has said hello and sent its configuration. The configuration (which
//! sites are never read, which buttons are safe to press) is the module's: it says it in `browser.config`. Nothing here is stored.
//!
//! A module is told what the pages are (`browser` event: the pages it watches, when they change, and the tab lists) and may ask for
//! a button to be pressed or a tab to go to another page. What is too big to pass through a module (a guide's words, a picture) goes
//! from here straight to the island's own page: see `browser_get`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tungstenite::{handshake::server::{ErrorResponse, Request, Response}, Message};

/// the extension tries these in order
pub const PORTS: [u16; 5] = [47653, 47654, 47655, 47656, 47657];
const PROTOCOL: u64 = 1;
/// the most pages a module is told of in one event (a page can be a lot of text)
const PAGES_PER_EVENT: usize = 2;
/// the most nodes of a page a module is told of
const MAX_NODES: usize = 400;
/// how often a module is told of what changed
const FEED_EVERY: Duration = Duration::from_secs(1);

// ---------------------------------------------------------------- what the extension sends

/// A section of a guide (or an entry of its contents) the page names: where it is and what it is called.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct GuideLink {
    pub t: String,
    pub u: String,
    /// how deep in the contents list
    pub d: u8,
}

/// A guide as the extension read it: its body as blocks (`["h", level, text]`, `["p", text]`, `["ul", [items]]`,
/// `["ol", [items]]`, `["pre", text]`, `["tbl", [[cells]]]`), and the sections around it.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Guide {
    /// changes when the guide's words do (worked out here): the island fetches the guide once per key
    pub key: String,
    pub blocks: Vec<Value>,
    /// a plain-text FAQ: the original text, untouched
    pub raw: Option<String>,
    pub text: bool,
    pub cut: bool,
    pub prev: Option<GuideLink>,
    pub next: Option<GuideLink>,
    pub toc: Vec<GuideLink>,
    pub url: String,
}

impl Guide {
    /// Only what looks like blocks is kept, and the key is worked out from the words.
    fn seal(mut self) -> Option<Guide> {
        use std::hash::{Hash, Hasher};
        self.blocks.retain(|b| b.as_array().map_or(false, |a| a.len() >= 2 && a[0].is_string()));
        if self.blocks.len() < 3 {
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.url.hash(&mut h);
        for b in &self.blocks {
            b.to_string().hash(&mut h);
        }
        self.key = format!("{:x}", h.finish());
        Some(self)
    }

    /// the headings of the third level and below, in order (what a closed card says is the section on screen)
    fn heads(&self) -> Vec<String> {
        self.blocks
            .iter()
            .filter_map(|b| {
                let a = b.as_array()?;
                (a[0].as_str()? == "h" && a.get(1)?.as_u64()? >= 3).then(|| a.get(2).and_then(|t| t.as_str()).map(|t| t.chars().take(200).collect::<String>()))?
            })
            .take(3)
            .collect()
    }
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
#[serde(default)]
pub struct NodeMsg {
    k: String,
    l: Option<u8>,
    t: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PageMsg {
    url: String,
    title: String,
    blocked: bool,
    tab: i64,
    win: i64,
    active: bool,
    /// the height of the window
    vh: f32,
    words: u32,
    meta: Value,
    ld: Value,
    video: Value,
    nodes: Vec<NodeMsg>,
    guide: Option<Guide>,
    /// the page's main picture, shrunk by the extension: a `data:image/jpeg;base64,...` address
    image: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TabInfo {
    tab: i64,
    win: i64,
    title: String,
    active: bool,
}

/// What the extension described for one tab.
#[derive(Clone, Debug)]
pub struct ExtSnapshot {
    pub url: String,
    pub title: String,
    pub vh: f32,
    pub words: u32,
    pub meta: Value,
    pub ld: Value,
    pub video: Value,
    pub nodes: Vec<NodeMsg>,
    pub guide: Option<Guide>,
    /// the page's main picture, when the extension sent one
    pub image: Option<PageImage>,
    /// changes whenever anything about this page changes (a read, the video)
    pub version: u64,
}

/// A page's picture. The id changes only when the picture does, so the island fetches the bytes once.
#[derive(Clone, Debug)]
pub struct PageImage {
    pub id: u64,
    pub data: Arc<str>,
}

// ---------------------------------------------------------------- state

struct TabPage {
    win: i64,
    active: bool,
    snap: ExtSnapshot,
}

struct Conn {
    browser: String,
    tx: mpsc::Sender<String>,
}

#[derive(Default)]
struct Inner {
    conns: HashMap<u64, Conn>,
    pages: HashMap<(u64, i64), TabPage>,
    lists: HashMap<u64, Vec<TabInfo>>,
    waiters: HashMap<u64, mpsc::Sender<bool>>,
    /// pictures asked for (see `guide_picture`): who is waiting for which
    picture_waiters: HashMap<u64, mpsc::Sender<Option<String>>>,
    /// the pictures that came, by (the guide's key, the picture's number, its size): a card that is drawn again does not ask
    /// again. Emptied when it grows too big.
    pictures: HashMap<(String, u32, u32), String>,
    /// tabs the island sent to another page of a guide: (the address and the title it had, when). The first page the tab then
    /// reports at another address is the same card, moved on (see `take_moves`).
    nav: HashMap<(u64, i64), (String, String, Instant)>,
    /// cards whose tab moved on: (browser, title before, title now)
    moves: Vec<(String, String, String)>,
    next: u64,
    version: u64,
    next_image: u64,
}

#[derive(Default)]
pub struct ExtState {
    inner: Mutex<Inner>,
    /// what every extension is told to do (see `set_config`); sent as it connects, and when it changes
    config: Mutex<String>,
    serving: AtomicBool,
}

/// the one link of this app (it is a socket on this PC)
pub fn state() -> &'static Arc<ExtState> {
    static S: OnceLock<Arc<ExtState>> = OnceLock::new();
    S.get_or_init(|| Arc::new(ExtState::default()))
}

/// "C:\\...\\msedge.exe" or "Edge" or "msedge" -> "msedge"
pub fn browser_of(exe: &str) -> String {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_lowercase();
    file.strip_suffix(".exe").unwrap_or(&file).to_string()
}

/// "Rust docs - Google Chrome - Michael" -> "Rust docs": a title as the page names itself
pub fn clean_title(title: &str) -> String {
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

impl ExtState {
    /// the browser has an extension connected: the only source of its pages
    pub fn covers(&self, browser: &str) -> bool {
        let b = browser_of(browser);
        self.inner.lock().unwrap().conns.values().any(|c| c.browser == b)
    }

    /// the browsers that are connected
    pub fn browsers(&self) -> Vec<String> {
        let mut v: Vec<String> = self.inner.lock().unwrap().conns.values().map(|c| c.browser.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    /// The page the extension described for a tab of that browser, found by its title. The extension reads every tab; when two
    /// have the same title the one in front wins, then the most recently described.
    pub fn snapshot(&self, browser: &str, title: &str) -> Option<ExtSnapshot> {
        let b = browser_of(browser);
        let want = title.trim().to_lowercase();
        let g = self.inner.lock().unwrap();
        g.pages
            .iter()
            .filter(|((c, _), _)| g.conns.get(c).map_or(false, |c| c.browser == b))
            .filter(|(_, p)| clean_title(&p.snap.title).trim().to_lowercase() == want)
            .max_by_key(|(_, p)| (p.active, p.snap.version))
            .map(|(_, p)| p.snap.clone())
    }

    /// Send the tab (found by its title) to another page of its guide, on the same site. The page is not read here: it is read by
    /// the extension once it has loaded, like any other, and the card follows the tab (`take_moves`). `true` when the browser took
    /// the order.
    pub fn navigate(&self, browser: &str, title: &str, url: &str) -> bool {
        let b = browser_of(browser);
        let want = title.trim().to_lowercase();
        let (tx, rx) = mpsc::channel();
        let id = {
            let mut g = self.inner.lock().unwrap();
            let found = g
                .pages
                .iter()
                .filter(|((c, _), _)| g.conns.get(c).map_or(false, |c| c.browser == b))
                .filter(|(_, p)| clean_title(&p.snap.title).trim().to_lowercase() == want)
                .max_by_key(|(_, p)| (p.active, p.snap.version))
                .map(|((c, tab), p)| (*c, *tab, p.snap.url.clone(), clean_title(&p.snap.title)));
            let Some((conn_id, tab, old_url, old_title)) = found else { return false };
            let Some(conn) = g.conns.get(&conn_id) else { return false };
            let sent = conn.tx.send(json!({ "t": "go", "id": g.next + 1, "tab": tab, "url": url }).to_string()).is_ok();
            if !sent {
                return false;
            }
            g.next += 1;
            let id = g.next;
            g.waiters.insert(id, tx);
            g.nav.insert((conn_id, tab), (old_url, old_title, Instant::now()));
            id
        };
        let ok = rx.recv_timeout(Duration::from_millis(4000)).unwrap_or(false);
        let mut g = self.inner.lock().unwrap();
        g.waiters.remove(&id);
        if !ok {
            g.nav.retain(|_, (_, _, at)| at.elapsed() < Duration::from_secs(1));
        }
        ok
    }

    /// Scroll the tab (found by its title) to a heading of its page, by the heading's words: a guide followed step by step from the
    /// island. `true` when the page found it.
    pub fn scroll(&self, browser: &str, title: &str, heading: &str) -> bool {
        let b = browser_of(browser);
        let want = title.trim().to_lowercase();
        let (tx, rx) = mpsc::channel();
        let id = {
            let mut g = self.inner.lock().unwrap();
            let found = g
                .pages
                .iter()
                .filter(|((c, _), _)| g.conns.get(c).map_or(false, |c| c.browser == b))
                .filter(|(_, p)| clean_title(&p.snap.title).trim().to_lowercase() == want)
                .max_by_key(|(_, p)| (p.active, p.snap.version))
                .map(|((c, tab), _)| (*c, *tab));
            let Some((conn_id, tab)) = found else { return false };
            let Some(conn) = g.conns.get(&conn_id) else { return false };
            if conn.tx.send(json!({ "t": "scroll", "id": g.next + 1, "tab": tab, "heading": heading }).to_string()).is_err() {
                return false;
            }
            g.next += 1;
            let id = g.next;
            g.waiters.insert(id, tx);
            id
        };
        let ok = rx.recv_timeout(Duration::from_millis(1500)).unwrap_or(false);
        self.inner.lock().unwrap().waiters.remove(&id);
        ok
    }

    /// The cards whose tab went to another page since last asked: (browser, title before, title now).
    pub fn take_moves(&self) -> Vec<(String, String, String)> {
        std::mem::take(&mut self.inner.lock().unwrap().moves)
    }

    /// The titles (as the window names them) of the pages the extension has described for that browser: every tab it could read,
    /// not only the ones that were visited. The tabs in front come first.
    pub fn open_pages(&self, browser: &str) -> Vec<String> {
        let b = browser_of(browser);
        let g = self.inner.lock().unwrap();
        let mut found: Vec<(bool, u64, String)> = g
            .pages
            .iter()
            .filter(|((c, _), _)| g.conns.get(c).map_or(false, |c| c.browser == b))
            .map(|(_, p)| (p.active, p.snap.version, clean_title(&p.snap.title)))
            .filter(|(_, _, t)| !t.trim().is_empty())
            .collect();
        found.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
        found.into_iter().map(|(_, _, t)| t).collect()
    }

    /// The titles of the tabs the extension lists for that browser, lower case: what is open (not blocked ones).
    pub fn tab_titles(&self, browser: &str) -> Option<Vec<String>> {
        let b = browser_of(browser);
        let g = self.inner.lock().unwrap();
        let mut found = false;
        let mut out = Vec::new();
        for (id, _) in g.conns.iter().filter(|(_, c)| c.browser == b) {
            found = true;
            if let Some(list) = g.lists.get(id) {
                out.extend(list.iter().map(|t| clean_title(&t.title).to_lowercase()));
            }
        }
        found.then_some(out)
    }

    /// Press a button of the page in front, by its text (the module decides which are safe).
    pub fn press(&self, browser: &str, label: &str) -> bool {
        let b = browser_of(browser);
        let (tx, rx) = mpsc::channel();
        let id = {
            let mut g = self.inner.lock().unwrap();
            let Some(conn) = g.conns.values().find(|c| c.browser == b) else { return false };
            let sent = conn.tx.send(json!({ "t": "press", "id": g.next + 1, "label": label }).to_string()).is_ok();
            if !sent {
                return false;
            }
            g.next += 1;
            let id = g.next;
            g.waiters.insert(id, tx);
            id
        };
        let ok = rx.recv_timeout(Duration::from_millis(1500)).unwrap_or(false);
        self.inner.lock().unwrap().waiters.remove(&id);
        ok
    }

    /// the guide of a card, by the key the card was given (see `Guide::seal`): always the page's own
    pub fn guide(&self, key: &str) -> Option<Guide> {
        let g = self.inner.lock().unwrap();
        g.pages.values().find_map(|p| p.snap.guide.as_ref().filter(|x| x.key == key).cloned())
    }

    /// A picture inside a guide (a table's icon, a figure), by the number its marker has: the extension finds it in the tab (from
    /// the browser's own copy when it can, else without cookies), shrinks it to `max` px and sends it as a `data:` address. The
    /// app never contacts the site. Blocks until it comes (the extension does them one at a time).
    pub fn guide_picture(&self, key: &str, no: u32, max: u32) -> Option<String> {
        let max = max.clamp(32, 480);
        let (tx, rx) = mpsc::channel();
        let id = {
            let mut g = self.inner.lock().unwrap();
            if let Some(d) = g.pictures.get(&(key.to_string(), no, max)) {
                return Some(d.clone());
            }
            let found = g.pages.iter().find(|(_, p)| p.snap.guide.as_ref().map_or(false, |x| x.key == key)).map(|((c, tab), _)| (*c, *tab));
            let (conn_id, tab) = found?;
            let conn = g.conns.get(&conn_id)?;
            let sent = conn.tx.send(json!({ "t": "img", "id": g.next + 1, "tab": tab, "n": no, "max": max }).to_string()).is_ok();
            if !sent {
                return None;
            }
            g.next += 1;
            let id = g.next;
            g.picture_waiters.insert(id, tx);
            id
        };
        let got = rx.recv_timeout(Duration::from_secs(40)).ok().flatten();
        let mut g = self.inner.lock().unwrap();
        g.picture_waiters.remove(&id);
        if let Some(d) = &got {
            if g.pictures.len() >= 300 {
                g.pictures.clear();
            }
            g.pictures.insert((key.to_string(), no, max), d.clone());
        }
        got
    }

    /// the bytes of a picture the extension sent, as a `data:` address
    pub fn image(&self, id: u64) -> Option<String> {
        let g = self.inner.lock().unwrap();
        g.pages.values().find_map(|p| p.snap.image.as_ref().filter(|i| i.id == id).map(|i| i.data.to_string()))
    }

    /// what the extensions are told: every connected one gets it now, and a new one when it says hello
    pub fn set_config(&self, msg: String) {
        *self.config.lock().unwrap() = msg.clone();
        for c in self.inner.lock().unwrap().conns.values() {
            let _ = c.tx.send(msg.clone());
        }
    }

    fn config(&self) -> String {
        let c = self.config.lock().unwrap().clone();
        if c.is_empty() {
            disabled()
        } else {
            c
        }
    }

    fn version(&self) -> u64 {
        self.inner.lock().unwrap().version
    }

    /// The link is served from now on (once started, it stays: it is a socket on this PC).
    pub fn serve(self: &Arc<Self>) {
        if !self.serving.swap(true, Ordering::Relaxed) {
            spawn(self.clone());
        }
    }
}

/// what an extension is told when no plugin wants it to read: nothing
pub fn disabled() -> String {
    json!({ "t": "config", "enabled": false }).to_string()
}

/// What the module says the extensions are to do (`browser.config`): which sites are never read (`hosts`, `hostWords`,
/// `titleWords`), which buttons may be pressed (`safe`) and whether pictures are sent (`images`). It is read as far as the extension
/// reads it; `enabled` is the host's to say.
pub fn config_json(module: &Value) -> String {
    let list = |k: &str| -> Vec<String> { module.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).take(400).map(|s| s.chars().take(120).collect()).collect()).unwrap_or_default() };
    json!({
        "t": "config",
        "enabled": true,
        "hosts": list("hosts"),
        "hostWords": list("hostWords"),
        "titleWords": list("titleWords"),
        "safe": list("safe"),
        "images": module.get("images").and_then(|v| v.as_bool()).unwrap_or(false),
    })
    .to_string()
}

// ---------------------------------------------------------------- what a module is told

/// What one module has been told of the pages, and which it watches: a page is told when it changes, the lists when anything does.
#[derive(Default)]
pub struct Feed {
    /// the pages it asks to be told of: (browser, title as the window names it)
    pub watch: Vec<(String, String)>,
    sent: HashMap<(String, String), u64>,
    seen_version: u64,
    last: Option<Instant>,
    was_empty: bool,
}

impl Feed {
    /// The `browser` event, when there is something to say (at most once a second): the browsers with an extension, the tabs
    /// each lists and has open, the cards that moved to another page of their guide, and the watched pages that changed.
    pub fn poll(&mut self, ext: &ExtState) -> Option<Value> {
        if self.last.map_or(false, |t| t.elapsed() < FEED_EVERY) {
            return None;
        }
        let version = ext.version();
        let browsers = ext.browsers();
        let moves = ext.take_moves();
        let mut pages: Vec<Value> = Vec::new();
        for (browser, title) in self.watch.iter() {
            if pages.len() >= PAGES_PER_EVENT {
                break;
            }
            let Some(s) = ext.snapshot(browser, title) else { continue };
            let key = (browser.clone(), title.clone());
            if self.sent.get(&key) == Some(&s.version) {
                continue;
            }
            self.sent.insert(key, s.version);
            let guide = s.guide.as_ref().map(|g| json!({ "key": g.key, "text": g.text, "heads": g.heads() }));
            pages.push(json!({
                "browser": browser, "title": title, "url": s.url, "version": s.version, "vh": s.vh, "words": s.words,
                "meta": s.meta, "ld": s.ld, "video": s.video, "guide": guide, "image": s.image.as_ref().map(|i| i.id),
                "nodes": s.nodes.iter().take(MAX_NODES).collect::<Vec<_>>(),
            }));
        }
        self.sent.retain(|(b, t), _| self.watch.iter().any(|(wb, wt)| wb == b && wt == t));
        let changed = version != self.seen_version || !moves.is_empty() || !pages.is_empty();
        let empty = browsers.is_empty();
        if !changed && self.last.map_or(false, |t| t.elapsed() < Duration::from_secs(5)) && self.was_empty == empty {
            return None;
        }
        self.last = Some(Instant::now());
        self.seen_version = version;
        self.was_empty = empty;
        let mut tabs = serde_json::Map::new();
        let mut open = serde_json::Map::new();
        for b in &browsers {
            if let Some(list) = ext.tab_titles(b) {
                tabs.insert(b.clone(), json!(list));
            }
            open.insert(b.clone(), json!(ext.open_pages(b)));
        }
        Some(json!({ "browsers": browsers, "tabs": tabs, "open": open, "moves": moves, "pages": pages }))
    }
}

// ---------------------------------------------------------------- the server

pub fn spawn(ext: Arc<ExtState>) {
    std::thread::spawn(move || {
        let Some(listener) = PORTS.iter().find_map(|p| TcpListener::bind(("127.0.0.1", *p)).ok()) else {
            return; // every port is taken: the extension is simply not served
        };
        for stream in listener.incoming().flatten() {
            let ext = ext.clone();
            std::thread::spawn(move || {
                let _ = serve(&ext, stream);
            });
        }
    });
}

fn origin_ok(req: &Request) -> bool {
    req.headers().get("Origin").and_then(|v| v.to_str().ok()).map_or(false, |o| o.starts_with("chrome-extension://"))
}

fn serve(ext: &Arc<ExtState>, stream: TcpStream) -> Option<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    stream.set_nodelay(true).ok();
    let mut ws = tungstenite::accept_hdr_with_config(
        stream,
        |req: &Request, resp: Response| {
            if origin_ok(req) {
                Ok(resp)
            } else {
                Err(ErrorResponse::new(Some("only a browser extension may connect".to_string())))
            }
        },
        Some(tungstenite::protocol::WebSocketConfig { max_message_size: Some(1 << 20), max_frame_size: Some(1 << 20), ..Default::default() }),
    )
    .ok()?;
    ws.get_mut().set_read_timeout(Some(Duration::from_millis(250))).ok()?;
    let (tx, rx) = mpsc::channel::<String>();
    let mut id = 0u64;
    let started = Instant::now();
    loop {
        // a connection that never says hello is dropped
        if id == 0 && started.elapsed() > Duration::from_secs(10) {
            break;
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                if !handle(ext, &mut id, &tx, &text) {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
        while let Ok(out) = rx.try_recv() {
            if ws.send(Message::Text(out)).is_err() {
                id_cleanup(ext, id);
                return Some(());
            }
        }
    }
    id_cleanup(ext, id);
    Some(())
}

fn id_cleanup(ext: &Arc<ExtState>, id: u64) {
    if id == 0 {
        return;
    }
    let mut g = ext.inner.lock().unwrap();
    g.conns.remove(&id);
    g.lists.remove(&id);
    g.pages.retain(|(c, _), _| *c != id);
    g.version += 1;
}

/// one message from the extension; `false` ends the connection
fn handle(ext_state: &Arc<ExtState>, id: &mut u64, tx: &mpsc::Sender<String>, text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return true };
    let t = v.get("t").and_then(|t| t.as_str()).unwrap_or("");
    let ext = ext_state;
    if t == "hello" {
        if v.get("v").and_then(|x| x.as_u64()) != Some(PROTOCOL) {
            let _ = tx.send(json!({ "t": "error", "why": "protocol" }).to_string());
            return false;
        }
        let browser = v.get("browser").and_then(|b| b.as_str()).unwrap_or("chrome").to_lowercase();
        let mut g = ext.inner.lock().unwrap();
        g.next += 1;
        *id = g.next;
        g.conns.insert(*id, Conn { browser, tx: tx.clone() });
        g.version += 1;
        drop(g);
        let _ = tx.send(ext.config());
        return true;
    }
    if *id == 0 {
        return true; // nothing counts before hello
    }
    match t {
        "ping" => {
            let _ = tx.send(json!({ "t": "pong" }).to_string());
        }
        "page" => {
            let Ok(mut m) = serde_json::from_value::<PageMsg>(v) else { return true };
            let mut g = ext.inner.lock().unwrap();
            g.version += 1;
            let version = g.version;
            if m.blocked || m.url.is_empty() {
                // the extension says this tab is not to be shown
                g.pages.remove(&(*id, m.tab));
                return true;
            }
            // the same picture again keeps its id
            let image = match m.image.take().filter(|d| d.starts_with("data:image/")) {
                Some(d) => match g.pages.get(&(*id, m.tab)).and_then(|p| p.snap.image.clone()).filter(|i| *i.data == *d) {
                    Some(same) => Some(same),
                    None => {
                        g.next_image += 1;
                        Some(PageImage { id: g.next_image, data: d.into() })
                    }
                },
                None => None,
            };
            let snap = ExtSnapshot {
                url: m.url.clone(),
                title: m.title.clone(),
                vh: m.vh,
                words: m.words,
                meta: m.meta,
                ld: m.ld,
                video: m.video,
                nodes: std::mem::take(&mut m.nodes),
                guide: m.guide.take().and_then(Guide::seal),
                image,
                version,
            };
            // a tab that came to the front is the one active in its window
            if m.active {
                for ((c, _), p) in g.pages.iter_mut() {
                    if *c == *id && p.win == m.win {
                        p.active = false;
                    }
                }
            }
            // a tab the island sent to another page of its guide has arrived there: the card is the same card
            g.nav.retain(|_, (_, _, at)| at.elapsed() < Duration::from_secs(30));
            if let Some((old_url, old_title, _)) = g.nav.get(&(*id, m.tab)).cloned() {
                if old_url != m.url {
                    g.nav.remove(&(*id, m.tab));
                    let now = clean_title(&m.title);
                    if let Some(b) = g.conns.get(id).map(|c| c.browser.clone()) {
                        if now != old_title {
                            g.moves.push((b, old_title, now));
                        }
                    }
                }
            }
            g.pages.insert((*id, m.tab), TabPage { win: m.win, active: m.active, snap });
        }
        "image" => {
            // the picture arrives after the page: it was being fetched and shrunk
            let tab = v.get("tab").and_then(|x| x.as_i64()).unwrap_or(-1);
            let Some(d) = v.get("image").and_then(|x| x.as_str()).filter(|d| d.starts_with("data:image/") && d.len() < 400_000) else { return true };
            let mut g = ext.inner.lock().unwrap();
            g.version += 1;
            let version = g.version;
            g.next_image += 1;
            let image = PageImage { id: g.next_image, data: d.into() };
            if let Some(p) = g.pages.get_mut(&(*id, tab)) {
                p.snap.image = Some(image);
                p.snap.version = version;
            }
        }
        "video" => {
            let tab = v.get("tab").and_then(|x| x.as_i64()).unwrap_or(-1);
            let video = v.get("video").cloned().unwrap_or(Value::Null);
            let mut g = ext.inner.lock().unwrap();
            g.version += 1;
            let version = g.version;
            if let Some(p) = g.pages.get_mut(&(*id, tab)) {
                p.snap.video = video;
                p.snap.version = version;
            }
        }
        "active" => {
            let tab = v.get("tab").and_then(|x| x.as_i64()).unwrap_or(-1);
            let win = v.get("win").and_then(|x| x.as_i64()).unwrap_or(-1);
            let mut g = ext.inner.lock().unwrap();
            g.version += 1;
            for ((c, t), p) in g.pages.iter_mut() {
                if *c == *id && p.win == win {
                    p.active = *t == tab;
                }
            }
        }
        "gone" => {
            let tab = v.get("tab").and_then(|x| x.as_i64()).unwrap_or(-1);
            let mut g = ext.inner.lock().unwrap();
            g.version += 1;
            g.pages.remove(&(*id, tab));
        }
        "tabs" => {
            let list: Vec<TabInfo> = v.get("list").and_then(|l| serde_json::from_value(l.clone()).ok()).unwrap_or_default();
            let mut g = ext.inner.lock().unwrap();
            g.version += 1;
            // a page of a tab that is no longer listed (closed, or now blocked) is forgotten
            let open: Vec<i64> = list.iter().map(|t| t.tab).collect();
            g.pages.retain(|(c, tab), _| *c != *id || open.contains(tab));
            for t in &list {
                if let Some(p) = g.pages.get_mut(&(*id, t.tab)) {
                    p.active = t.active && t.win == p.win;
                }
            }
            g.lists.insert(*id, list);
        }
        "imgd" => {
            let wid = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
            let data = v
                .get("data")
                .and_then(|x| x.as_str())
                .filter(|d| d.starts_with("data:image/") && d.len() < 600_000 && v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false))
                .map(str::to_string);
            if let Some(w) = ext.inner.lock().unwrap().picture_waiters.get(&wid) {
                let _ = w.send(data);
            }
        }
        "pressed" | "went" | "scrolled" => {
            let wid = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            if let Some(w) = ext.inner.lock().unwrap().waiters.get(&wid) {
                let _ = w.send(ok);
            }
        }
        _ => {}
    }
    true
}

