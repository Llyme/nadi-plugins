//! What kind of page is in front of you, and what is worth saying about it.
//!
//! Evidence comes in tiers, cheapest first, and is combined as independent votes: each piece of evidence
//! gives a kind a weight, and a kind's confidence is `1 - product(1 - weight)`. The kinds are:
//!
//! * the address (domain and path patterns),
//! * the window title (its shape: " - Wikipedia", "How to ...", "Pull Request #212"),
//! * the page's structure from the browser extension (headings like "Step 3", "Ingredients", a price and an
//!   "Add to cart" button, a "0:34 / 4:12" timer),
//! * the page's own metadata from the extension (JSON-LD, Open Graph, the video's clock).
//!
//! Below `MIN_CONFIDENCE` nothing is claimed and the card stays a plain page card. Everything here is pure:
//! it takes what was read and returns what to show, so it is tested without a browser.

use crate::page::{Node, NodeKind, Page};
use serde::Serialize;

pub const MIN_CONFIDENCE: f32 = 0.6;

thread_local! {
    static NOW: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

/// The moment dates are read against (a module has no clock: the island's `now` of the event goes here).
pub fn set_now(ms: u64) {
    NOW.with(|n| n.set(ms as i64));
}

fn now_ms() -> i64 {
    NOW.with(|n| n.get())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Walkthrough,
    Wiki,
    News,
    Video,
    Recipe,
    Qna,
    Api,
    Product,
    Search,
    WebApp,
}

const KINDS: [Kind; 10] = [
    Kind::Walkthrough,
    Kind::Wiki,
    Kind::News,
    Kind::Video,
    Kind::Recipe,
    Kind::Qna,
    Kind::Api,
    Kind::Product,
    Kind::Search,
    Kind::WebApp,
];

impl Kind {
    fn idx(self) -> usize {
        KINDS.iter().position(|k| *k == self).unwrap_or(0)
    }
    pub fn id(self) -> &'static str {
        match self {
            Kind::Walkthrough => "walkthrough",
            Kind::Wiki => "wiki",
            Kind::News => "news",
            Kind::Video => "video",
            Kind::Recipe => "recipe",
            Kind::Qna => "qna",
            Kind::Api => "api",
            Kind::Product => "product",
            Kind::Search => "search",
            Kind::WebApp => "webapp",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Walkthrough => "Walkthrough",
            Kind::Wiki => "Wiki article",
            Kind::News => "News article",
            Kind::Video => "Video",
            Kind::Recipe => "Recipe",
            Kind::Qna => "Q&A thread",
            Kind::Api => "API reference",
            Kind::Product => "Product page",
            Kind::Search => "Search results",
            Kind::WebApp => "Web app",
        }
    }
}

pub struct Evidence<'a> {
    pub url: Option<&'a str>,
    /// the window title without the browser's name
    pub title: &'a str,
    pub page: Option<&'a Page>,
    /// what the browser extension said about the page (metadata, video), when it is connected
    pub ext: Option<&'a crate::ext::ExtData>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PageField {
    pub key: &'static str,
    pub value: String,
    /// worked out loosely (from the title or a guess at the scroll), not read from the page
    pub rough: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct PageKind {
    pub id: &'static str,
    pub label: &'static str,
    pub confidence: f32,
    pub fields: Vec<PageField>,
    /// the work pill's two lines
    pub main: String,
    pub sub: String,
    /// how far down the page you are, 0..1
    /// what the collapsed card says at its right
    pub peek: Option<String>,
    /// a guide laid out to be read: the card asks for its words by `key` (see `ext::Guide`)
    pub guide: Option<GuideRef>,
}

/// What the browsing card knows of a guide before it has its words.
#[derive(Serialize, Clone, Debug)]
pub struct GuideRef {
    pub key: String,
    pub text: bool,
}

// ---- small text helpers (no regex: the app does not carry one) ---------------------------------------

fn lc(s: &str) -> String {
    s.to_lowercase()
}

fn has(hay: &str, needle: &str) -> bool {
    hay.contains(needle)
}

fn has_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

fn digits_at(s: &str) -> Option<(u32, usize)> {
    let end = s.char_indices().find(|(_, c)| !c.is_ascii_digit()).map_or(s.len(), |(i, _)| i);
    (end > 0 && end <= 6).then(|| (s[..end].parse().unwrap_or(0), end))
}

/// "Step 3: Add the block", "3. Add the block", "3) Add the block", "Step 3 - ..." -> (3, "Add the block")
pub fn step_of(text: &str) -> Option<(u32, String)> {
    let t = text.trim();
    let l = lc(t);
    let (n, rest) = if let Some(r) = l.strip_prefix("step ") {
        let (n, used) = digits_at(r)?;
        (n, &t[t.len() - r.len() + used..])
    } else {
        let (n, used) = digits_at(&l)?;
        let after = &t[used..];
        let mut it = after.chars();
        match it.next() {
            Some('.') | Some(')') => {}
            _ => return None,
        }
        // "3.5 million" is not a step
        if it.next().map_or(true, |c| !c.is_whitespace()) {
            return None;
        }
        (n, &after[1..])
    };
    let name = rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | '\u{2013}' | '\u{2014}' | '.' | ')' | '\u{b7}')).trim();
    (n >= 1 && n <= 99).then(|| (n, name.to_string()))
}

fn clock_secs(s: &str) -> Option<f32> {
    let mut total = 0f32;
    let mut parts = 0;
    for p in s.trim().split(':') {
        let n: f32 = p.trim().parse().ok()?;
        total = total * 60.0 + n;
        parts += 1;
    }
    (parts >= 2 && parts <= 3).then_some(total)
}

/// "0:34 / 4:12" or "1:02:03 of 2:00:00" -> (position, length) in seconds
fn clock_pair(text: &str) -> Option<(f32, f32)> {
    let t = text.trim();
    let (a, b) = t.split_once('/').or_else(|| t.split_once(" of "))?;
    let (p, d) = (clock_secs(a)?, clock_secs(b)?);
    (d > 0.0 && p <= d + 1.0).then_some((p, d))
}

fn fmt_clock(secs: f32) -> String {
    let s = secs.max(0.0).round() as u32;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn has_price(text: &str) -> bool {
    let cs: Vec<char> = text.chars().collect();
    cs.windows(3).any(|w| matches!(w[0], '$' | '\u{20ac}' | '\u{a3}' | '\u{a5}' | '\u{20b1}') && (w[1].is_ascii_digit() || (w[1] == ' ' && w[2].is_ascii_digit())))
}

fn is_byline(text: &str) -> bool {
    let t = text.trim();
    t.len() < 70 && lc(t).starts_with("by ") && t[3..].chars().next().map_or(false, |c| c.is_uppercase())
}

fn is_ago(text: &str) -> bool {
    let l = lc(text);
    l.len() < 60 && l.chars().any(|c| c.is_ascii_digit()) && has_any(&l, &[" ago", "updated", "published", "min read"])
}

fn short(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}\u{2026}", s.chars().take(n.saturating_sub(1)).collect::<String>().trim_end())
    }
}

// ---- the votes ----------------------------------------------------------------------------------------

struct Votes {
    miss: [f32; 11],
}

impl Votes {
    fn new() -> Self {
        Self { miss: [1.0; 11] }
    }
    fn add(&mut self, k: Kind, w: f32) {
        let i = k.idx();
        self.miss[i] *= 1.0 - w.clamp(0.0, 0.98);
    }
    fn conf(&self, k: Kind) -> f32 {
        1.0 - self.miss[k.idx()]
    }
    fn best(&self) -> (Kind, f32) {
        KINDS.iter().map(|k| (*k, self.conf(*k))).fold((Kind::Walkthrough, 0.0), |a, b| if b.1 > a.1 { b } else { a })
    }
}

struct Url {
    host: String,
    path: String,
    query: String,
}

fn parse_url(u: &str) -> Option<Url> {
    let rest = u.split("://").nth(1)?;
    let (hostpath, query) = rest.split_once('?').map_or((rest, ""), |(a, b)| (a, b.split('#').next().unwrap_or("")));
    let hostpath = hostpath.split('#').next().unwrap_or("");
    let (host, path) = hostpath.split_once('/').map_or((hostpath, ""), |(h, p)| (h, p));
    let host = host.rsplit('@').next().unwrap_or(host).split(':').next().unwrap_or(host);
    Some(Url { host: lc(host.strip_prefix("www.").unwrap_or(host)), path: format!("/{}", lc(path)), query: lc(query) })
}

fn host_is(u: &Url, d: &str) -> bool {
    u.host == d || u.host.ends_with(&format!(".{d}"))
}

fn dated_path(p: &str) -> u8 {
    // /2026/10/02/ -> 2, /2026/10/ -> 1
    let segs: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    for i in 0..segs.len() {
        if segs[i].len() == 4 && segs[i].chars().all(|c| c.is_ascii_digit()) && (segs[i].starts_with("19") || segs[i].starts_with("20")) {
            let num = |s: Option<&&str>| s.map_or(false, |s| s.len() <= 2 && s.chars().all(|c| c.is_ascii_digit()));
            if num(segs.get(i + 1)) && num(segs.get(i + 2)) {
                return 2;
            }
            if num(segs.get(i + 1)) {
                return 1;
            }
        }
    }
    0
}

fn vote_url(v: &mut Votes, u: &Url) {
    let p = u.path.as_str();
    // wiki
    if has_any(&u.host, &["wikipedia.org", "wikimedia.org", "fandom.com", "wikia.com"]) || u.host.starts_with("wiki.") || u.host.ends_with(".wiki") {
        v.add(Kind::Wiki, 0.7);
    }
    if has(p, "/wiki/") {
        v.add(Kind::Wiki, 0.45);
    }
    // video
    if host_is(u, "youtube.com") && (p.starts_with("/watch") || p.starts_with("/shorts/") || p.starts_with("/live/")) || host_is(u, "youtu.be") {
        v.add(Kind::Video, 0.85);
    }
    if (host_is(u, "vimeo.com") && p.len() > 3 && p[1..].chars().all(|c| c.is_ascii_digit()))
        || (host_is(u, "twitch.tv") && p.starts_with("/videos/"))
        || (host_is(u, "dailymotion.com") && p.starts_with("/video"))
        || (host_is(u, "bilibili.com") && p.starts_with("/video/"))
        || (host_is(u, "netflix.com") && p.starts_with("/watch"))
        || (host_is(u, "tiktok.com") && has(p, "/video/"))
    {
        v.add(Kind::Video, 0.8);
    }
    // search
    let q = u.query.starts_with("q=") || has(&u.query, "&q=");
    if (has(&u.host, "google.") && p.starts_with("/search"))
        || (host_is(u, "bing.com") && p.starts_with("/search"))
        || (host_is(u, "duckduckgo.com") && q)
        || (host_is(u, "brave.com") && p.starts_with("/search"))
        || (host_is(u, "yahoo.com") && p.starts_with("/search"))
        || (host_is(u, "ecosia.org") && q)
    {
        v.add(Kind::Search, 0.85);
    } else if q && has(p, "search") {
        v.add(Kind::Search, 0.6);
    }
    // questions and threads
    if (host_is(u, "stackoverflow.com") || host_is(u, "stackexchange.com") || host_is(u, "superuser.com") || host_is(u, "serverfault.com") || host_is(u, "askubuntu.com")) && p.starts_with("/questions/") {
        v.add(Kind::Qna, 0.85);
    }
    if host_is(u, "ycombinator.com") && p.starts_with("/item") {
        v.add(Kind::Qna, 0.75);
    }
    if host_is(u, "quora.com") {
        v.add(Kind::Qna, 0.6);
    }
    // references
    if (host_is(u, "mozilla.org") && has(p, "/docs/"))
        || host_is(u, "docs.rs")
        || host_is(u, "devdocs.io")
        || host_is(u, "pkg.go.dev")
        || (host_is(u, "doc.rust-lang.org"))
        || (host_is(u, "nodejs.org") && p.starts_with("/api"))
        || (host_is(u, "apple.com") && p.starts_with("/documentation"))
        || (host_is(u, "microsoft.com") && has(p, "/api/"))
    {
        v.add(Kind::Api, 0.75);
    }
    if has_any(&u.host, &["readthedocs.io", "readthedocs.org"]) || host_is(u, "docs.python.org") || host_is(u, "learn.microsoft.com") {
        v.add(Kind::Api, 0.5);
    }
    if has_any(p, &["/api/", "/reference/", "/docs/"]) {
        v.add(Kind::Api, 0.35);
    }
    if u.host.starts_with("docs.") || u.host.starts_with("developer.") || u.host.starts_with("api.") {
        v.add(Kind::Api, 0.3);
    }
    // products
    if (has(&u.host, "amazon.") && (has(p, "/dp/") || has(p, "/gp/product")))
        || (has(&u.host, "ebay.") && has(p, "/itm/"))
        || (has(&u.host, "aliexpress.") && has(p, "/item/"))
        || (host_is(u, "walmart.com") && has(p, "/ip/"))
        || (host_is(u, "etsy.com") && has(p, "/listing/"))
    {
        v.add(Kind::Product, 0.85);
    }
    if has_any(p, &["/products/", "/product/", "/dp/"]) {
        v.add(Kind::Product, 0.55);
    }
    // recipes
    if has_any(p, &["/recipe/", "/recipes/"]) {
        v.add(Kind::Recipe, 0.75);
    }
    if ["allrecipes.com", "seriouseats.com", "bbcgoodfood.com", "epicurious.com", "tasty.co", "food.com", "simplyrecipes.com", "delish.com"].iter().any(|d| host_is(u, d)) || (host_is(u, "nytimes.com") && u.path.starts_with("/recipes")) || host_is(u, "cooking.nytimes.com") {
        v.add(Kind::Recipe, 0.6);
    }
    // news
    match dated_path(p) {
        2 => v.add(Kind::News, 0.55),
        1 => v.add(Kind::News, 0.4),
        _ => {}
    }
    if has_any(p, &["/news/", "/article/", "/articles/", "/story/", "/stories/"]) {
        v.add(Kind::News, 0.4);
    }
    if ["bbc.com", "bbc.co.uk", "cnn.com", "nytimes.com", "reuters.com", "theguardian.com", "apnews.com", "bloomberg.com", "washingtonpost.com", "aljazeera.com", "forbes.com", "techcrunch.com", "theverge.com", "wired.com", "arstechnica.com", "npr.org", "foxnews.com", "nbcnews.com", "cbsnews.com", "usatoday.com", "independent.co.uk", "telegraph.co.uk", "ft.com", "wsj.com", "economist.com", "inquirer.net", "rappler.com", "gmanetwork.com", "philstar.com"].iter().any(|d| host_is(u, d)) {
        v.add(Kind::News, 0.45);
    }
    // web apps: a page of one item in a tool
    if (host_is(u, "github.com") && ["/pull/", "/issues/", "/actions/", "/commit/", "/compare/", "/discussions/"].iter().any(|s| has(p, s)))
        || (host_is(u, "gitlab.com") && (has(p, "/merge_requests/") || has(p, "/issues/")))
        || (host_is(u, "atlassian.net") && has(p, "/browse/"))
        || (host_is(u, "linear.app") && has(p, "/issue/"))
        || (host_is(u, "dev.azure.com") && has(p, "/pullrequest/"))
        || (host_is(u, "bitbucket.org") && has(p, "/pull-requests/"))
        || (host_is(u, "trello.com") && p.starts_with("/c/"))
    {
        v.add(Kind::WebApp, 0.85);
    }
    if host_is(u, "notion.so") || host_is(u, "figma.com") || (host_is(u, "google.com") && u.host.starts_with("docs.")) || host_is(u, "asana.com") {
        v.add(Kind::WebApp, 0.5);
    }
    // walkthroughs
    if host_is(u, "wikihow.com") {
        v.add(Kind::Walkthrough, 0.8);
    }
    if has_any(p, &["/walkthrough"]) {
        v.add(Kind::Walkthrough, 0.7);
    }
    if has_any(p, &["/tutorial", "/getting-started", "/quickstart", "/how-to", "/howto"]) {
        v.add(Kind::Walkthrough, 0.55);
    }
    if has_any(p, &["/guide", "/lesson", "/learn/"]) {
        v.add(Kind::Walkthrough, 0.4);
    }
}

fn vote_title(v: &mut Votes, title: &str) {
    let t = lc(title);
    if has_any(&t, &[" - wikipedia", " | wikipedia"]) {
        v.add(Kind::Wiki, 0.75);
    } else if t.ends_with(" wiki") || has(&t, "| fandom") || has(&t, " - fandom") {
        v.add(Kind::Wiki, 0.5);
    }
    if t.ends_with(" - youtube") {
        v.add(Kind::Video, 0.8);
    }
    if t.ends_with(" - google search") || has(&t, " at duckduckgo") || t.ends_with(" - brave search") {
        v.add(Kind::Search, 0.85);
    } else if t.ends_with(" - bing") || t.ends_with(" - search") {
        v.add(Kind::Search, 0.45);
    }
    if has_any(&t, &["- stack overflow", "- stack exchange", "- super user", "- server fault", "- ask ubuntu"]) {
        v.add(Kind::Qna, 0.85);
    } else if has(&t, "- quora") {
        v.add(Kind::Qna, 0.7);
    }
    if t.starts_with("how to ") || has_any(&t, &["tutorial", "step-by-step", "step by step", "getting started"]) {
        v.add(Kind::Walkthrough, 0.5);
    }
    if has(&t, "walkthrough") {
        v.add(Kind::Walkthrough, 0.7);
    }
    if t.ends_with(" guide") || has(&t, "guide to") {
        v.add(Kind::Walkthrough, 0.3);
    }
    if has(&t, " recipe") {
        v.add(Kind::Recipe, 0.65);
    }
    if has_any(&t, &[" | amazon", "amazon.com:", " - ebay"]) {
        v.add(Kind::Product, 0.45);
    }
    if has_price(title) {
        v.add(Kind::Product, 0.3);
    }
    if has_any(&t, &["api reference", "- mdn web docs", "| mdn"]) {
        v.add(Kind::Api, 0.7);
    }
    if has(&t, "documentation") {
        v.add(Kind::Api, 0.3);
    }
    if has_any(&t, &["pull request #", "merge request", "\u{b7} pull request"]) {
        v.add(Kind::WebApp, 0.85);
    } else if has_any(&t, &["issue #", "issues \u{b7}", " - jira", "[jira]"]) {
        v.add(Kind::WebApp, 0.7);
    }
}

/// The anchors of every kind, found in a list of texts (the page's nodes). `trust` scales the weights.
fn vote_texts(v: &mut Votes, texts: &[(&str, Option<NodeKind>)], trust: f32) {
    let lower: Vec<(String, Option<NodeKind>)> = texts.iter().map(|(t, k)| (lc(t), *k)).collect();
    let heading = |k: &Option<NodeKind>| matches!(k, Some(NodeKind::Heading(_)) | None);
    let count = |f: &dyn Fn(&str, &Option<NodeKind>) -> bool| lower.iter().filter(|(t, k)| f(t, k)).count();
    let any = |f: &dyn Fn(&str, &Option<NodeKind>) -> bool| count(f) > 0;
    let w = |x: f32| x * trust;

    // walkthrough: numbered steps
    let steps = texts.iter().filter(|(t, k)| (heading(k) || matches!(k, Some(NodeKind::Item))) && step_of(t).is_some()).count();
    match steps {
        0 => {}
        1 => v.add(Kind::Walkthrough, w(0.45)),
        _ => v.add(Kind::Walkthrough, w(0.8)),
    }
    if any(&|t, _| has_any(t, &["next step", "next lesson", "next:"]) && t.len() < 40) {
        v.add(Kind::Walkthrough, w(0.25));
    }
    if any(&|t, k| heading(k) && has_any(t, &["prerequisites", "before you begin", "what you'll need", "what you need", "requirements"])) {
        v.add(Kind::Walkthrough, w(0.25));
    }

    // wiki: the sections every article ends with
    let wiki = count(&|t, k| heading(k) && matches!(t.trim(), "contents" | "see also" | "references" | "external links" | "notes" | "further reading" | "citations" | "bibliography"));
    match wiki {
        0 => {}
        1 => v.add(Kind::Wiki, w(0.35)),
        _ => v.add(Kind::Wiki, w(0.8)),
    }
    if any(&|t, _| t.starts_with("from wikipedia")) {
        v.add(Kind::Wiki, w(0.6));
    }

    // news
    if texts.iter().any(|(t, _)| is_byline(t)) {
        v.add(Kind::News, w(0.3));
    }
    if texts.iter().any(|(t, _)| is_ago(t)) {
        v.add(Kind::News, w(0.25));
    }
    if any(&|t, _| t.len() < 30 && has(t, "min read")) {
        v.add(Kind::News, w(0.2));
    }
    if any(&|t, _| t.len() < 25 && t == "advertisement") {
        v.add(Kind::News, w(0.15));
    }

    // video
    if texts.iter().any(|(t, _)| clock_pair(t).is_some()) {
        v.add(Kind::Video, w(0.7));
    }
    if any(&|t, k| matches!(k, Some(NodeKind::Button)) && (t.starts_with("play") || t.starts_with("pause")) && t.len() < 24) {
        v.add(Kind::Video, w(0.5));
    }
    if any(&|t, _| has_any(t, &["autoplay", "theater mode", "theatre mode"])) {
        v.add(Kind::Video, w(0.3));
    }

    // recipe
    if any(&|t, _| t.trim() == "ingredients" || t.starts_with("ingredients")) {
        v.add(Kind::Recipe, w(0.4));
    }
    if any(&|t, k| heading(k) && matches!(t.trim(), "instructions" | "directions" | "method" | "preparation" | "steps")) {
        v.add(Kind::Recipe, w(0.35));
    }
    if any(&|t, _| has_any(t, &["prep time", "cook time", "total time"])) {
        v.add(Kind::Recipe, w(0.35));
    }
    if any(&|t, _| has_any(t, &["servings", "serves ", "yield"]) && t.len() < 40) {
        v.add(Kind::Recipe, w(0.2));
    }
    if any(&|t, _| has_any(t, &["jump to recipe", "print recipe"])) {
        v.add(Kind::Recipe, w(0.3));
    }

    // questions
    if any(&|t, k| heading(k) && has(t, "answer") && t.len() < 40) {
        v.add(Kind::Qna, w(0.45));
    }
    if any(&|t, _| has_any(t, &["ask question", "your answer", "accepted answer"])) {
        v.add(Kind::Qna, w(0.3));
    }
    if any(&|t, _| has_any(t, &["related questions", "linked questions"])) {
        v.add(Kind::Qna, w(0.2));
    }

    // references
    let api = count(&|t, k| heading(k) && matches!(t.trim(), "parameters" | "return value" | "returns" | "syntax" | "examples" | "example" | "browser compatibility" | "specifications" | "arguments" | "throws" | "exceptions" | "see also" | "usage"));
    match api {
        0 => {}
        1 => v.add(Kind::Api, w(0.2)),
        2 => v.add(Kind::Api, w(0.5)),
        _ => v.add(Kind::Api, w(0.75)),
    }
    if any(&|t, _| t.len() < 40 && has_any(t, &["deprecated", "experimental"])) {
        v.add(Kind::Api, w(0.1));
    }

    // product
    if any(&|t, k| matches!(k, Some(NodeKind::Button) | None) && has_any(t, &["add to cart", "add to bag", "add to basket", "buy now"])) {
        v.add(Kind::Product, w(0.55));
    }
    if texts.iter().any(|(t, _)| t.len() < 24 && has_price(t)) {
        v.add(Kind::Product, w(0.25));
    }
    if any(&|t, _| t.len() < 40 && has_any(t, &["in stock", "out of stock", "only ", "left in stock"]) && has_any(t, &["stock", "left"])) {
        v.add(Kind::Product, w(0.3));
    }
    if any(&|t, _| t.len() < 50 && has_any(t, &["out of 5", " ratings", " reviews"]) && t.chars().any(|c| c.is_ascii_digit())) {
        v.add(Kind::Product, w(0.25));
    }

    // search
    if any(&|t, _| t.starts_with("about ") && has(t, " results") && t.len() < 60) {
        v.add(Kind::Search, w(0.5));
    }
    if any(&|t, _| has_any(t, &["people also ask", "related searches", "searches related"])) {
        v.add(Kind::Search, w(0.4));
    }
    let h3 = texts.iter().filter(|(_, k)| matches!(k, Some(NodeKind::Heading(3)))).count();
    if h3 >= 6 {
        v.add(Kind::Search, w(0.35));
    }

    // tools
    let tabs = count(&|t, _| matches!(t.trim(), "pull requests" | "issues" | "files changed" | "conversation" | "commits" | "checks" | "actions" | "code" | "projects"));
    if tabs >= 3 {
        v.add(Kind::WebApp, w(0.55));
    }
    if any(&|t, _| has_any(t, &["merge pull request", "create pull request", "assignees", "reviewers"]) && t.len() < 40) {
        v.add(Kind::WebApp, w(0.3));
    }
}

// ---- reading the tree -------------------------------------------------------------------------------

fn lines_of(nodes: &[Node]) -> Vec<(&str, Option<NodeKind>)> {
    nodes.iter().map(|n| (n.name.as_str(), Some(n.kind))).collect()
}

/// The page's text as paragraphs. The tree hands over text in runs (a link splits a sentence in three), so
/// runs of text and links that sit on the same line, or on the next one, are joined; a heading, a button, a list
/// item or a gap of more than half a line ends the paragraph.
fn paragraphs(nodes: &[Node]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    let mut cur = String::new();
    let mut first = 0usize;
    let mut prev: Option<&Node> = None;
    let flush = |cur: &mut String, out: &mut Vec<(String, usize)>, first: usize| {
        let t = cur.trim();
        if !t.is_empty() {
            out.push((t.to_string(), first));
        }
        cur.clear();
    };
    for (i, n) in nodes.iter().enumerate() {
        if !matches!(n.kind, NodeKind::Text | NodeKind::Link) {
            flush(&mut cur, &mut out, first);
            prev = None;
            continue;
        }
        if n.block {
            flush(&mut cur, &mut out, first);
            out.push((n.name.trim().to_string(), i));
            prev = None;
            continue;
        }
        let breaks = prev.map_or(false, |p| {
            let line_h = (p.bottom - p.top).max(8.0);
            p.bottom > p.top && n.bottom > n.top && n.top - p.bottom > line_h * 0.5
        });
        if breaks {
            flush(&mut cur, &mut out, first);
        }
        if cur.is_empty() {
            first = i;
        } else {
            let t = n.name.trim_start();
            let glue = t.starts_with([')', ',', '.', ';', ':', '!', '?', '\'', '\u{2019}', ']']) || cur.ends_with(['(', '[']) || cur.ends_with(' ');
            if !glue {
                cur.push(' ');
            }
        }
        cur.push_str(n.name.trim());
        prev = Some(n);
    }
    flush(&mut cur, &mut out, first);
    out
}

/// "This article is about the airship. For other uses, see ...": a note above the text, not the text
fn is_hatnote(t: &str) -> bool {
    let l = lc(t);
    l.starts_with("this article is about") || l.starts_with("for other uses") || l.starts_with("not to be confused") || l.contains("redirects here") || (l.starts_with("for the ") && l.contains(", see "))
}

/// The value written next to a label: "Prep: 10 mins" or a "Prep" node followed by a "10 mins" node.
fn label_value(nodes: &[Node], labels: &[&str]) -> Option<String> {
    let i = nodes.iter().position(|n| n.name.len() < 24 && labels.contains(&lc(n.name.trim()).trim_end_matches(':')))?;
    if let Some((_, v)) = nodes[i].name.split_once(':') {
        if !v.trim().is_empty() {
            return Some(short(v, 30));
        }
    }
    nodes[i + 1..].iter().take(4).find(|x| matches!(x.kind, NodeKind::Text) && x.name.len() < 30 && x.name.chars().any(|c| c.is_ascii_digit())).map(|x| short(&x.name, 30))
}

/// The first paragraph after the heading called one of `names`.
fn para_after(nodes: &[Node], names: &[&str]) -> Option<String> {
    let i = nodes.iter().position(|n| matches!(n.kind, NodeKind::Heading(_)) && names.contains(&lc(n.name.trim()).as_str()))?;
    // (a heading often carries its own anchor link, which reads as a paragraph with the heading's name)
    let name = lc(nodes[i].name.trim());
    paragraphs(nodes).into_iter().find(|(t, at)| *at > i && t.chars().count() >= 8 && lc(t.trim()) != name).map(|(t, _)| short(&t, 70))
}

fn first_long_text(nodes: &[Node], min: usize, after_h1: bool) -> Option<String> {
    let start = if after_h1 { nodes.iter().position(|n| n.kind == NodeKind::Heading(1)).map_or(0, |i| i + 1) } else { 0 };
    paragraphs(nodes).into_iter().find(|(t, i)| *i >= start && t.chars().count() >= min && !is_ago(t) && !is_byline(t) && !is_hatnote(t)).map(|(t, _)| short(&t, 150))
}

/// The opening paragraph of the page: the first long paragraph after the main heading.
pub fn lead(nodes: &[Node]) -> Option<String> {
    let start = nodes.iter().position(|n| n.kind == NodeKind::Heading(1)).map_or(0, |i| i + 1);
    let mut out = String::new();
    for (t, i) in paragraphs(nodes) {
        if i < start {
            continue;
        }
        if t.chars().count() < 60 || is_ago(&t) || is_byline(&t) || is_hatnote(&t) {
            if !out.is_empty() {
                break;
            }
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&t);
        if out.chars().count() >= 260 {
            break;
        }
    }
    (!out.is_empty()).then(|| if out.chars().count() > 320 { format!("{}\u{2026}", out.chars().take(317).collect::<String>().trim_end()) } else { out })
}

fn h1_or_title(nodes: &[Node], title: &str) -> String {
    nodes.iter().find(|n| n.kind == NodeKind::Heading(1) && n.name.chars().count() >= 4).map(|n| n.name.clone()).unwrap_or_else(|| strip_site(title))
}

/// "Zeppelin - Wikipedia" -> "Zeppelin", "Port strike ends | The Daily Ledger" -> "Port strike ends"
pub fn strip_site(title: &str) -> String {
    for sep in [" | ", " - ", " \u{2013} ", " \u{2014} ", " \u{b7} "] {
        if let Some(i) = title.rfind(sep) {
            if i >= 8 {
                return title[..i].trim().to_string();
            }
        }
    }
    title.trim().to_string()
}

fn site_of(title: &str) -> Option<String> {
    for sep in [" | ", " - ", " \u{2013} ", " \u{2014} "] {
        if let Some(i) = title.rfind(sep) {
            let s = title[i + sep.len()..].trim();
            if !s.is_empty() && s.len() < 40 {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn field(key: &'static str, value: impl Into<String>, rough: bool) -> PageField {
    PageField { key, value: value.into(), rough }
}

fn query_of(url: &str) -> Option<String> {
    let q = url.split('?').nth(1)?;
    for pair in q.split(['&', '#']) {
        if let Some(v) = pair.strip_prefix("q=").or_else(|| pair.strip_prefix("query=")).or_else(|| pair.strip_prefix("search_query=")) {
            let mut out = String::new();
            let b = v.as_bytes();
            let mut i = 0;
            while i < b.len() {
                match b[i] {
                    b'+' => out.push(' '),
                    b'%' if i + 2 < b.len() => {
                        if let Ok(x) = u8::from_str_radix(&v[i + 1..i + 3], 16) {
                            out.push(x as char);
                            i += 2;
                        }
                    }
                    c => out.push(c as char),
                }
                i += 1;
            }
            return Some(out);
        }
    }
    None
}

/// What is worth saying about a page of this kind. `p` is the tree, if there was one.
fn fields_for(kind: Kind, ev: &Evidence) -> (Vec<PageField>, String, String) {
    let title = strip_site(ev.title);
    let empty: Vec<Node> = Vec::new();
    let nodes: &[Node] = ev.page.map_or(&empty[..], |p| &p.nodes[..]);
    let mut f: Vec<PageField> = Vec::new();
    let (main, mut sub);

    match kind {
        Kind::Walkthrough => {
            let steps: Vec<(&Node, u32, String)> = nodes.iter().filter(|n| matches!(n.kind, NodeKind::Heading(_) | NodeKind::Item)).filter_map(|n| step_of(&n.name).map(|(k, t)| (n, k, t))).collect();
            main = h1_or_title(nodes, ev.title);
            sub = "Walkthrough".to_string();
            if steps.is_empty() {
                f.push(field("Guide", short(&title, 60), true));
            } else {
                let total = steps.iter().map(|s| s.1).max().unwrap_or(1);
                f.push(field("Steps", format!("{total}"), false));
                if let Some(first) = steps.first() {
                    f.push(field("Starts with", short(if first.2.is_empty() { "Step 1" } else { &first.2 }, 60), false));
                }
                sub = format!("{total} steps");
            }
        }
        Kind::Wiki => {
            main = h1_or_title(nodes, ev.title);
            sub = "Wiki article".to_string();
            f.push(field("Article", short(&main, 60), false));
            if let Some(t) = first_long_text(nodes, 80, true) {
                f.push(field("Summary", t, false));
            }
            let secs: Vec<&Node> = nodes.iter().filter(|n| matches!(n.kind, NodeKind::Heading(2 | 3)) && n.name.len() < 60 && !matches!(lc(n.name.trim()).as_str(), "contents" | "navigation menu" | "personal tools")).collect();
            if !secs.is_empty() {
                f.push(field("Sections", format!("{}", nodes.iter().filter(|n| n.kind == NodeKind::Heading(2)).count()), false));
            }
        }
        Kind::News => {
            main = h1_or_title(nodes, ev.title);
            let outlet = site_of(ev.title).or_else(|| ev.url.and_then(parse_url).map(|u| u.host));
            f.push(field("Headline", short(&main, 70), false));
            if let Some(o) = &outlet {
                f.push(field("Outlet", o.clone(), false));
            }
            let by = nodes.iter().find(|n| is_byline(&n.name)).map(|n| n.name.trim().to_string());
            if let Some(b) = &by {
                f.push(field("Byline", short(b, 40), false));
            }
            let when = nodes.iter().find(|n| is_ago(&n.name) && !has(&lc(&n.name), "min read")).map(|n| short(&n.name, 40));
            if let Some(w) = &when {
                f.push(field("Published", w.clone(), false));
            }
            if let Some(t) = first_long_text(nodes, 80, true) {
                f.push(field("Gist", t, false));
            }
            let words: usize = paragraphs(nodes).iter().filter(|(t, _)| t.len() > 60).map(|(t, _)| t.split_whitespace().count()).sum();
            if words > 200 {
                f.push(field("Read time", format!("about {} min", ((words as f32 / 230.0).ceil() as u32).max(1)), true));
            }
            if let Some(n) = nodes.iter().find(|n| has_any(&lc(&n.name), &["subscribe to continue", "to keep reading", "free articles", "to read the full"]) && n.name.len() < 120) {
                f.push(field("Paywall", short(&n.name, 50), false));
            }
            sub = match (&outlet, &when) {
                (Some(o), Some(w)) => format!("{o} \u{b7} {}", short(w, 24)),
                (Some(o), None) => o.clone(),
                _ => "News article".to_string(),
            };
        }
        Kind::Video => {
            main = h1_or_title(nodes, ev.title).replace(" - YouTube", "");
            f.push(field("Title", short(&main, 70), false));
            sub = "Video".to_string();
            if let Some((pos, dur)) = nodes.iter().find_map(|n| clock_pair(&n.name)) {
                f.push(field("Time", format!("{} left of {}", fmt_clock(dur - pos), fmt_clock(dur)), false));
                sub = format!("{} left", fmt_clock(dur - pos));
            }
            let playing = nodes.iter().find(|n| n.kind == NodeKind::Button).and_then(|_| {
                nodes.iter().find_map(|n| {
                    let t = lc(&n.name);
                    if n.kind == NodeKind::Button && t.len() < 24 {
                        if t.starts_with("pause") {
                            return Some(true);
                        }
                        if t.starts_with("play") {
                            return Some(false);
                        }
                    }
                    None
                })
            });
            if let Some(pl) = playing {
                f.push(field("State", if pl { "Playing" } else { "Paused" }, false));
            }
            return (f, main, sub);
        }
        Kind::Recipe => {
            main = h1_or_title(nodes, ev.title);
            f.push(field("Dish", short(&main, 60), false));
            sub = "Recipe".to_string();
            let time = label_value(nodes, &["total time", "prep time", "cook time", "prep", "cook", "total"]);
            let serves = label_value(nodes, &["servings", "serves", "makes", "yield"]);
            match (&time, &serves) {
                (Some(t), Some(s)) => f.push(field("Time", format!("{t} \u{b7} serves {s}"), false)),
                (Some(t), None) => f.push(field("Time", t.clone(), false)),
                (None, Some(s)) => f.push(field("Serves", s.clone(), false)),
                _ => {}
            }
            if let Some(i) = nodes.iter().position(|n| n.name.len() < 24 && lc(n.name.trim()).trim_end_matches(':') == "ingredients") {
                let end = nodes[i + 1..]
                    .iter()
                    .position(|n| matches!(n.kind, NodeKind::Heading(_)) || (n.name.len() < 24 && matches!(lc(n.name.trim()).trim_end_matches(':'), "method" | "instructions" | "directions" | "nutrition")))
                    .map_or(nodes.len(), |e| i + 1 + e);
                let n = nodes[i + 1..end].iter().filter(|n| n.kind == NodeKind::Item).count();
                if n > 0 {
                    f.push(field("Ingredients", format!("{n} items"), false));
                }
            }
            let steps: Vec<&Node> = nodes.iter().filter(|n| matches!(n.kind, NodeKind::Heading(_) | NodeKind::Item) && step_of(&n.name).is_some()).collect();
            if !steps.is_empty() {
                f.push(field("Steps", format!("{}", steps.len()), false));
            }
            if let Some(t) = &time {
                sub = t.clone();
            }
        }
        Kind::Qna => {
            main = h1_or_title(nodes, ev.title);
            f.push(field("Question", short(&main, 70), false));
            sub = "Q&A thread".to_string();
            if let Some(n) = nodes.iter().find(|n| matches!(n.kind, NodeKind::Heading(_)) && has(&lc(&n.name), "answer") && n.name.len() < 30 && n.name.chars().any(|c| c.is_ascii_digit())) {
                f.push(field("Answers", n.name.trim().to_string(), false));
                sub = n.name.trim().to_string();
            }
            if nodes.iter().any(|n| has_any(&lc(&n.name), &["accepted answer", "accepted"]) && n.name.len() < 40) {
                f.push(field("Accepted", "Has an accepted answer", false));
            }
        }
        Kind::Api => {
            main = h1_or_title(nodes, ev.title);
            f.push(field("Symbol", short(&main, 60), false));
            let path = ev.url.and_then(parse_url).map(|u| u.path.trim_matches('/').split('/').rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" \u{203a} "));
            sub = path.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| "API reference".to_string());
            if let Some(p) = path.filter(|p| !p.is_empty()) {
                f.push(field("Path", p, false));
            }
            if let Some(t) = para_after(nodes, &["return value", "returns"]) {
                f.push(field("Returns", t, false));
            }
            // a badge next to the title: only the first screens, and only the badge itself
            let h1 = nodes.iter().position(|n| n.kind == NodeKind::Heading(1)).unwrap_or(0);
            if let Some(n) = nodes[h1..].iter().take(120).find(|n| matches!(lc(n.name.trim()).as_str(), "deprecated" | "experimental" | "non-standard")) {
                f.push(field("Status", n.name.trim().to_string(), false));
            }
        }
        Kind::Product => {
            main = h1_or_title(nodes, ev.title);
            f.push(field("Product", short(&main, 60), false));
            sub = "Product page".to_string();
            let prices: Vec<&str> = nodes.iter().filter(|n| n.name.len() < 24 && has_price(&n.name)).map(|n| n.name.trim()).collect();
            if let Some(p) = prices.first() {
                f.push(field("Price", short(p, 20), false));
                sub = short(p, 20);
                if let Some(old) = prices.iter().skip(1).find(|x| *x != p) {
                    f.push(field("Other price", short(old, 20), false));
                }
            }
            if let Some(n) = nodes.iter().find(|n| n.name.len() < 50 && has_any(&lc(&n.name), &["out of 5", " ratings", " reviews"]) && n.name.chars().any(|c| c.is_ascii_digit())) {
                f.push(field("Rating", short(&n.name, 40), false));
            }
            if let Some(n) = nodes.iter().find(|n| n.name.len() < 40 && has_any(&lc(&n.name), &["in stock", "out of stock", "left in stock"])) {
                f.push(field("Stock", short(&n.name, 36), false));
            }
        }
        Kind::Search => {
            let q = ev.url.and_then(query_of).unwrap_or_else(|| strip_site(ev.title));
            main = q.clone();
            f.push(field("Query", short(&q, 60), ev.url.is_none()));
            sub = "Search results".to_string();
            if let Some(n) = nodes.iter().find(|n| n.name.len() < 60 && lc(&n.name).starts_with("about ") && has(&lc(&n.name), " results")) {
                f.push(field("Results", short(n.name.trim(), 40), false));
                sub = short(n.name.trim(), 30);
            }
            let promo = |t: &str| {
                let l = lc(t);
                t.split_whitespace().count() < 3 || has_any(&l, &["learn more", "get our", "download", "browser", "sponsored", "privacy"])
            };
            let top: Vec<String> = nodes.iter().filter(|n| matches!(n.kind, NodeKind::Heading(2 | 3)) && !promo(&n.name)).take(3).map(|n| short(&n.name, 28)).collect();
            if !top.is_empty() {
                f.push(field("Top results", top.join(" \u{b7} "), false));
            }
        }
        Kind::WebApp => {
            main = h1_or_title(nodes, ev.title);
            f.push(field("Item", short(&main, 70), false));
            sub = "Web app".to_string();
            let state = nodes.iter().take(80).find(|n| matches!(n.name.trim(), "Open" | "Closed" | "Merged" | "Draft" | "In progress" | "Done" | "To Do"));
            if let Some(s) = state {
                f.push(field("State", s.name.trim().to_string(), false));
                sub = s.name.trim().to_string();
            }
            if let Some(n) = nodes.iter().find(|n| n.name.len() < 80 && has(&lc(&n.name), "check") && (has(&lc(&n.name), "passed") || has(&lc(&n.name), "failing") || has(&lc(&n.name), "successful") || has(&lc(&n.name), " / "))) {
                f.push(field("Checks", short(n.name.trim(), 50), false));
            }
        }
    }
    let _ = &mut sub;
    (f, main.clone(), sub)
}

/// Decide what the page is and what to say about it. `None` when nothing is sure enough.
pub fn read(ev: &Evidence) -> Option<PageKind> {
    let mut v = Votes::new();
    if let Some(u) = ev.url.and_then(parse_url) {
        vote_url(&mut v, &u);
    }
    vote_title(&mut v, ev.title);
    if let Some(d) = ev.ext {
        vote_ext(&mut v, d);
    }
    if let Some(p) = ev.page.filter(|p| p.nodes.len() >= 8) {
        vote_texts(&mut v, &lines_of(&p.nodes), 1.0);
    }
    let (kind, conf) = v.best();
    if conf < MIN_CONFIDENCE {
        return None;
    }
    let (mut fields, mut main, mut sub) = fields_for(kind, ev);
    if let Some(d) = ev.ext {
        enrich(kind, d, &mut fields, &mut main, &mut sub);
    }
    let (mut peek, mut guide) = (None, None);
    if matches!(kind, Kind::Walkthrough | Kind::Wiki) {
        if let Some(g) = ev.ext.and_then(|d| d.guide.as_ref()) {
            // the section on screen is what a closed card says
            peek = g.heads.first().map(|t| short(t, 24));
            guide = Some(GuideRef { key: g.key.clone(), text: g.text });
        }
    }
    Some(PageKind { id: kind.id(), label: kind.label(), confidence: conf, fields, main, sub, peek, guide })
}

// ---- what the page says about itself (from the browser extension) ---------------------------------------------

/// The page's own metadata is the strongest evidence there is: a page that says it is a Recipe is one.
fn vote_ext(v: &mut Votes, d: &crate::ext::ExtData) {
    for it in &d.ld {
        for t in &it.types {
            match t.as_str() {
                "Recipe" => v.add(Kind::Recipe, 0.95),
                "HowTo" => v.add(Kind::Walkthrough, 0.95),
                "NewsArticle" | "ReportageNewsArticle" | "AnalysisNewsArticle" | "OpinionNewsArticle" | "BackgroundNewsArticle" | "LiveBlogPosting" => v.add(Kind::News, 0.9),
                "Article" | "BlogPosting" => v.add(Kind::News, 0.5),
                "TechArticle" => {
                    v.add(Kind::Api, 0.3);
                    v.add(Kind::Walkthrough, 0.3);
                }
                "Product" | "ProductGroup" => v.add(Kind::Product, 0.95),
                "VideoObject" => v.add(Kind::Video, 0.9),
                "QAPage" | "Question" => v.add(Kind::Qna, 0.9),
                "FAQPage" => v.add(Kind::Qna, 0.35),
                "SearchResultsPage" => v.add(Kind::Search, 0.9),
                "WebApplication" | "SoftwareApplication" => v.add(Kind::WebApp, 0.3),
                _ => {}
            }
        }
    }
    if let Some(t) = d.meta.og_type.as_deref().map(lc) {
        if t == "article" {
            v.add(Kind::News, 0.4);
        } else if t.starts_with("video") {
            v.add(Kind::Video, 0.8);
        } else if t.starts_with("product") {
            v.add(Kind::Product, 0.8);
        }
    }
    if let Some(g) = d.meta.generator.as_deref().map(lc) {
        if g.contains("mediawiki") {
            v.add(Kind::Wiki, 0.9);
        } else if g.contains("dokuwiki") || g.contains("wiki") {
            v.add(Kind::Wiki, 0.6);
        }
    }
    if !d.meta.facts.is_empty() {
        v.add(Kind::Wiki, 0.3);
    }
    if d.video.as_ref().map_or(false, |x| x.dur >= 20.0) {
        v.add(Kind::Video, 0.5);
    }
}

fn fmt_mins(m: u32) -> String {
    if m >= 60 {
        if m % 60 == 0 { format!("{} h", m / 60) } else { format!("{} h {} min", m / 60, m % 60) }
    } else {
        format!("{m} min")
    }
}

fn money(amount: &str, currency: Option<&str>) -> String {
    let sym = match currency.map(|c| c.to_uppercase()).as_deref() {
        Some("USD") => "$".to_string(),
        Some("EUR") => "\u{20ac}".to_string(),
        Some("GBP") => "\u{a3}".to_string(),
        Some("JPY") | Some("CNY") => "\u{a5}".to_string(),
        Some("PHP") => "\u{20b1}".to_string(),
        Some(c) => format!("{c} "),
        None => String::new(),
    };
    format!("{sym}{amount}")
}

/// "2026-10-02T06:00:00Z" -> "2 h ago"; an older date as "Mar 2, 2025"
fn ago(iso: &str) -> Option<String> {
    use chrono::{DateTime, NaiveDate, Utc};
    let now = DateTime::from_timestamp_millis(now_ms())?;
    let then = DateTime::parse_from_rfc3339(iso).map(|d| d.with_timezone(&Utc)).ok().or_else(|| {
        let day = iso.get(..10)?;
        NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?.and_hms_opt(12, 0, 0).map(|d| d.and_utc())
    })?;
    let mins = (now - then).num_minutes();
    Some(if mins < -5 {
        then.format("%b %-d, %Y").to_string()
    } else if mins < 2 {
        "just now".to_string()
    } else if mins < 60 {
        format!("{mins} min ago")
    } else if mins < 24 * 60 {
        format!("{} h ago", mins / 60)
    } else if mins < 48 * 60 {
        "yesterday".to_string()
    } else if mins < 30 * 24 * 60 {
        format!("{} d ago", mins / (24 * 60))
    } else {
        then.format("%b %-d, %Y").to_string()
    })
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn rating_text(r: &crate::ext::Rating) -> Option<String> {
    let v = r.value?;
    Some(match r.count {
        Some(c) if c > 0 => format!("{v:.1} \u{b7} {} reviews", thousands(c)),
        _ => format!("{v:.1}"),
    })
}

/// Put a field in, or replace what a looser reading said: the page's own words win.
fn put(f: &mut Vec<PageField>, key: &'static str, value: String) {
    if let Some(x) = f.iter_mut().find(|x| x.key == key) {
        x.value = value;
        x.rough = false;
        return;
    }
    let at = f.iter().position(|x| x.key == "Progress").unwrap_or(f.len());
    f.insert(at, PageField { key, value, rough: false });
}

fn ld_of<'a>(d: &'a crate::ext::ExtData, ty: &str) -> Option<&'a crate::ext::LdItem> {
    d.ld.iter().find(|i| i.is(ty))
}

/// What the page's own metadata and video add to a reading of its text.
fn enrich(kind: Kind, d: &crate::ext::ExtData, f: &mut Vec<PageField>, main: &mut String, sub: &mut String) {
    match kind {
        Kind::Recipe => {
            if let Some(r) = ld_of(d, "Recipe") {
                let time = r.total.or_else(|| match (r.prep, r.cook) {
                    (Some(a), Some(b)) => Some(a + b),
                    (a, b) => a.or(b),
                });
                if let Some(t) = time {
                    put(f, "Time", match &r.yields {
                        Some(y) => format!("{} \u{b7} {}", fmt_mins(t), y),
                        None => fmt_mins(t),
                    });
                    if sub == "Recipe" {
                        *sub = fmt_mins(t);
                    }
                } else if let Some(y) = &r.yields {
                    put(f, "Serves", y.clone());
                }
                if let Some(n) = r.ingredients {
                    put(f, "Ingredients", format!("{n} items"));
                }
                if let Some(t) = r.rating.as_ref().and_then(rating_text) {
                    put(f, "Rating", t);
                }
                if !r.steps.is_empty() && !f.iter().any(|x| x.key == "Step now") {
                    put(f, "Steps", format!("{}", r.steps.len()));
                }
            }
        }
        Kind::Walkthrough => {
            if let Some(h) = ld_of(d, "HowTo") {
                if let Some(t) = h.total {
                    put(f, "Time", fmt_mins(t));
                }
                if !h.steps.is_empty() && !f.iter().any(|x| x.key == "Step") {
                    put(f, "Steps", format!("{}", h.steps.len()));
                }
            }
        }
        Kind::Wiki => {
            if !d.meta.facts.is_empty() {
                let facts: Vec<String> = d.meta.facts.iter().take(3).map(|(k, v)| format!("{k}: {}", short(v, 28))).collect();
                put(f, "Facts", facts.join(" \u{b7} "));
            }
        }
        Kind::News => {
            let art = d.ld.iter().find(|i| i.is("NewsArticle") || i.is("Article") || i.is("BlogPosting") || i.is("ReportageNewsArticle"));
            let by = d.meta.author.clone().or_else(|| art.and_then(|a| a.author.clone()));
            if let Some(b) = by {
                put(f, "Byline", short(b.trim().trim_start_matches("By ").trim_start_matches("by "), 40));
            }
            let when = d.meta.published.clone().or_else(|| art.and_then(|a| a.published.clone())).and_then(|p| ago(&p));
            if let Some(w) = &when {
                put(f, "Published", w.clone());
            }
            let outlet = d.meta.site.clone().or_else(|| art.and_then(|a| a.publisher.clone()));
            if let Some(o) = &outlet {
                put(f, "Outlet", o.clone());
            }
            if let Some(sec) = d.meta.section.clone().or_else(|| art.and_then(|a| a.section.clone())) {
                put(f, "Section", short(&sec, 30));
            }
            if d.words > 200 {
                let mins = ((d.words as f32 / 230.0).ceil() as u32).max(1);
                put(f, "Read time", format!("about {mins} min"));
            }
            if let Some(o) = &outlet {
                *sub = match &when {
                    Some(w) => format!("{o} \u{b7} {w}"),
                    None => o.clone(),
                };
            }
        }
        Kind::Video => {
            let ld = ld_of(d, "VideoObject");
            if let Some(a) = ld.and_then(|v| v.author.clone()).or_else(|| d.meta.author.clone()) {
                put(f, "Channel", short(&a, 40));
            }
            if let Some(v) = d.video.as_ref().filter(|v| v.dur > 0.0) {
                let left = (v.dur - v.cur).max(0.0);
                put(f, "Time", format!("{} left of {}", fmt_clock(left), fmt_clock(v.dur)));
                let state = if v.ended { "Ended".to_string() } else if v.paused { "Paused".to_string() } else if (v.rate - 1.0).abs() > 0.01 { format!("Playing at {}\u{d7}", (v.rate * 100.0).round() / 100.0) } else { "Playing".to_string() };
                put(f, "State", state);
                *sub = format!("{} left", fmt_clock(left));
            } else if let Some(m) = ld.and_then(|v| v.length) {
                put(f, "Length", fmt_mins(m));
            }
        }
        Kind::Product => {
            let p = ld_of(d, "Product").or_else(|| ld_of(d, "ProductGroup"));
            let offer = p.and_then(|x| x.offer.as_ref());
            let price = offer.and_then(|o| o.price.as_deref().map(|a| money(a, o.currency.as_deref()))).or_else(|| d.meta.price.as_deref().map(|a| money(a, d.meta.currency.as_deref())));
            if let Some(pr) = price {
                put(f, "Price", pr.clone());
                *sub = pr;
            }
            if let Some(a) = offer.and_then(|o| o.availability.as_deref()) {
                let text = match a {
                    "InStock" | "InStoreOnly" | "OnlineOnly" => "In stock",
                    "OutOfStock" | "SoldOut" => "Out of stock",
                    "PreOrder" | "PreSale" => "Pre-order",
                    "BackOrder" => "Back-ordered",
                    "LimitedAvailability" => "Few left",
                    _ => "",
                };
                if !text.is_empty() {
                    put(f, "Stock", text.to_string());
                }
            }
            if let Some(t) = p.and_then(|x| x.rating.as_ref()).and_then(rating_text) {
                put(f, "Rating", t);
            }
            if let Some(b) = p.and_then(|x| x.brand.clone()) {
                put(f, "Brand", short(&b, 30));
            }
        }
        Kind::Qna => {
            let q = d.ld.iter().find(|i| i.is("Question") || i.is("QAPage"));
            if let Some(n) = q.and_then(|x| x.answers) {
                put(f, "Answers", format!("{n} {}", if n == 1 { "answer" } else { "answers" }));
                *sub = format!("{n} {}", if n == 1 { "answer" } else { "answers" });
            }
            if let Some(v) = q.and_then(|x| x.votes) {
                put(f, "Votes", format!("{v:+}"));
            }
            if let Some(a) = q.and_then(|x| x.accepted.clone()).filter(|a| !a.is_empty()) {
                put(f, "Accepted", format!("\u{201c}{}\u{201d}", short(&a, 70)));
            }
        }
        _ => {}
    }
    let _ = main;
}

// ---- privacy ------------------------------------------------------------------------------------------

/// A private or incognito window is never read.
pub fn is_private_title(raw_title: &str) -> bool {
    let t = lc(raw_title);
    has_any(&t, &["inprivate", "incognito", "private browsing", "private window"])
}

/// Sites that are never read: banking, mail, health and password managers, plus the user's own list
/// (comma separated domains or words).
pub const BLOCK_HOSTS: &[&str] = &[
    "mail.google.com", "outlook.live.com", "outlook.office.com", "outlook.office365.com", "mail.yahoo.com", "proton.me", "protonmail.com",
    "paypal.com", "chase.com", "bankofamerica.com", "wellsfargo.com", "citi.com", "capitalone.com", "americanexpress.com", "hsbc.com",
    "mychart.com", "mychart.org", "lastpass.com", "1password.com", "bitwarden.com", "my.1password.com", "accounts.google.com",
    "login.microsoftonline.com", "login.live.com", "wise.com", "revolut.com", "coinbase.com", "binance.com", "bdo.com.ph", "bpi.com.ph", "unionbankph.com",
];
/// a site whose name contains one of these is never read
pub const BLOCK_HOST_WORDS: &[&str] = &["bank", "banking", "health", "patient", "medical", "clinic"];
/// the address is not always known: a title containing one of these betrays the page
pub const BLOCK_TITLE_WORDS: &[&str] = &["inbox (", " - inbox", "online banking", "internet banking", "sign in to your account", "password manager", "my chart", "patient portal"];

pub fn is_blocked(url: Option<&str>, title: &str) -> bool {
    let u = url.and_then(parse_url);
    if let Some(u) = &u {
        if BLOCK_HOSTS.iter().any(|h| host_is(u, h)) || has_any(&u.host, BLOCK_HOST_WORDS) {
            return true;
        }
    }
    let t = lc(title);
    has_any(&t, BLOCK_TITLE_WORDS) || t.ends_with(" - gmail") || t.ends_with(" - outlook")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(kind: NodeKind, name: &str, top: f32) -> Node {
        Node { kind, name: name.to_string(), left: 0.0, top, right: 100.0, bottom: top + 20.0, block: false }
    }

    fn page(nodes: Vec<Node>) -> Page {
        Page { url: None, nodes, view: (100.0, 900.0) }
    }

    #[test]
    fn steps_are_read_from_headings() {
        assert_eq!(step_of("Step 3: Add the block"), Some((3, "Add the block".to_string())));
        assert_eq!(step_of("3. Add the block"), Some((3, "Add the block".to_string())));
        assert_eq!(step_of("2) Sear"), Some((2, "Sear".to_string())));
        assert_eq!(step_of("3.5 million people"), None);
        assert_eq!(step_of("Introduction"), None);
    }

    #[test]
    fn clock_pairs() {
        assert_eq!(clock_pair("0:34 / 4:12"), Some((34.0, 252.0)));
        assert_eq!(clock_pair("1:02:03 of 2:00:00"), Some((3723.0, 7200.0)));
        assert_eq!(clock_pair("and / or"), None);
        assert_eq!(fmt_clock(252.0), "4:12");
    }

    #[test]
    fn a_wiki_address_alone_is_enough() {
        let ev = Evidence { url: Some("https://en.wikipedia.org/wiki/Zeppelin"), title: "Zeppelin - Wikipedia", page: None, ext: None };
        let k = read(&ev).expect("kind");
        assert_eq!(k.id, "wiki");
        assert!(k.confidence >= 0.9);
    }

    #[test]
    fn an_unknown_page_claims_nothing() {
        let ev = Evidence { url: Some("https://example.com/"), title: "Example Domain", page: None, ext: None };
        assert!(read(&ev).is_none());
    }

    #[test]
    fn a_walkthrough_is_found_by_its_steps_and_knows_where_you_are() {
        let nodes = vec![
            n(NodeKind::Heading(1), "Put Caddy in front of Node", 120.0),
            n(NodeKind::Heading(2), "Step 1: Install Caddy", -900.0),
            n(NodeKind::Heading(2), "Step 2: Write the Caddyfile", -300.0),
            n(NodeKind::Heading(2), "Step 3: Add the site block", 200.0),
            n(NodeKind::Text, "Open your Caddyfile and add a block for your domain, then reload.", 240.0),
            n(NodeKind::Heading(2), "Step 4: Reload the config", 1400.0),
            n(NodeKind::Text, "a", 1500.0), n(NodeKind::Text, "b", 1600.0), n(NodeKind::Text, "c", 1700.0), n(NodeKind::Text, "d", 1800.0),
        ];
        let p = page(nodes);
        let ev = Evidence { url: Some("https://docs.example.dev/tutorials/reverse-proxy"), title: "Reverse proxy a Node app - Caddy", page: Some(&p), ext: None };
        let k = read(&ev).expect("kind");
        assert_eq!(k.id, "walkthrough");
        // where you are on the page is not followed: the card names the guide and how many steps it has
        assert_eq!(k.main, "Put Caddy in front of Node");
        assert_eq!(k.sub, "4 steps");
        assert!(k.fields.iter().any(|f| f.key == "Steps" && f.value == "4"));
        assert!(k.fields.iter().any(|f| f.key == "Starts with" && f.value == "Install Caddy"));
    }

    #[test]
    fn a_video_page_reads_the_clock() {
        let nodes = vec![
            n(NodeKind::Heading(1), "Building a wooden clock, part 3", 120.0),
            n(NodeKind::Button, "Pause (k)", 600.0),
            n(NodeKind::Text, "15:36 / 28:10", 610.0),
            n(NodeKind::Button, "Subscribe", 700.0),
            n(NodeKind::Link, "Up next", 800.0), n(NodeKind::Text, "x", 900.0), n(NodeKind::Text, "y", 910.0), n(NodeKind::Text, "z", 920.0),
        ];
        let p = page(nodes);
        let ev = Evidence { url: None, title: "Building a wooden clock, part 3 - Makers - YouTube", page: Some(&p), ext: None };
        let k = read(&ev).expect("kind");
        assert_eq!(k.id, "video");
        assert!(k.fields.iter().any(|f| f.key == "Time" && f.value == "12:34 left of 28:10"), "{:?}", k.fields);
        assert!(k.fields.iter().any(|f| f.key == "State" && f.value == "Playing"));
    }

    #[test]
    fn private_and_blocked() {
        assert!(is_private_title("New InPrivate tab - Microsoft Edge"));
        assert!(is_private_title("New Tab - Google Chrome (Incognito)"));
        assert!(!is_private_title("Rust docs - Google Chrome"));
        assert!(is_blocked(Some("https://mail.google.com/mail/u/0/"), "Inbox"));
        assert!(is_blocked(Some("https://online.mybank.com/login"), "Welcome"));
        assert!(is_blocked(None, "Inbox (3) - me@example.com - Gmail"));
        assert!(!is_blocked(Some("https://en.wikipedia.org/wiki/Bank"), "Bank - Wikipedia"));
    }

    fn ext_data(json: &str) -> crate::ext::ExtData {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        crate::ext::ExtData {
            meta: serde_json::from_value(v.get("meta").cloned().unwrap_or_default()).unwrap_or_default(),
            ld: serde_json::from_value(v.get("ld").cloned().unwrap_or_default()).unwrap_or_default(),
            video: v.get("video").and_then(|x| serde_json::from_value(x.clone()).ok()),
            words: v.get("words").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            guide: None,
        }
    }

    #[test]
    fn a_walkthrough_with_a_guide_hands_the_card_its_key_and_the_section() {
        let mut d = ext_data("{}");
        let g: crate::ext::GuideLite = serde_json::from_str(r#"{"key":"k1","heads":["8/8 - 8/13","8/8"]}"#).unwrap();
        d.guide = Some(g);
        let ev = Evidence { url: Some("https://gamefaqs.gamespot.com/ps4/1-x/faqs/2/8-8-8-13"), title: "8/8 - 8/13 - Persona 5 Strikers Walkthrough & Guide - GameFAQs", page: None, ext: Some(&d) };
        let k = read(&ev).expect("kind");
        assert_eq!(k.id, "walkthrough");
        // the key is made when the extension's message is taken in; here none was made
        assert!(k.guide.is_some());
        assert_eq!(k.peek.as_deref(), Some("8/8 - 8/13"));
    }

    #[test]
    fn a_page_that_says_it_is_a_recipe_is_one() {
        let d = ext_data(r#"{"ld":[{"type":["Recipe"],"name":"Soup","total":45,"yield":"4 servings","ingredients":5,"steps":["a","b","c"],"rating":{"value":4.7,"count":1204}}]}"#);
        let ev = Evidence { url: Some("https://x.example/some/page"), title: "Soup", page: None, ext: Some(&d) };
        let k = read(&ev).expect("kind");
        assert_eq!(k.id, "recipe");
        let get = |key: &str| k.fields.iter().find(|f| f.key == key).map(|f| f.value.clone());
        assert_eq!(get("Time").as_deref(), Some("45 min \u{b7} 4 servings"));
        assert_eq!(get("Ingredients").as_deref(), Some("5 items"));
        assert_eq!(get("Rating").as_deref(), Some("4.7 \u{b7} 1,204 reviews"));
        assert_eq!(k.sub, "45 min");
    }

    #[test]
    fn a_product_page_gives_the_price_from_its_offer() {
        let d = ext_data(r#"{"meta":{"ogType":"product"},"ld":[{"type":["Product"],"name":"Aurora","brand":"Aurora","offer":{"price":"129.00","currency":"USD","availability":"InStock"},"rating":{"value":4.6,"count":2310}}]}"#);
        let ev = Evidence { url: Some("https://shop.example/p/aurora"), title: "Aurora ANC Headphones - $129 | Shop", page: None, ext: Some(&d) };
        let k = read(&ev).expect("kind");
        assert_eq!(k.id, "product");
        let get = |key: &str| k.fields.iter().find(|f| f.key == key).map(|f| f.value.clone());
        assert_eq!(get("Price").as_deref(), Some("$129.00"));
        assert_eq!(get("Stock").as_deref(), Some("In stock"));
        assert_eq!(get("Rating").as_deref(), Some("4.6 \u{b7} 2,310 reviews"));
        assert_eq!(k.sub, "$129.00");
    }

    #[test]
    fn the_players_own_clock_wins_for_video() {
        let d = ext_data(r#"{"video":{"cur":936,"dur":1690,"paused":false,"ended":false,"rate":1.25},"ld":[{"type":["VideoObject"],"author":"Makers Channel"}]}"#);
        let ev = Evidence { url: Some("https://v.example/watch?v=1"), title: "Building a clock", page: None, ext: Some(&d) };
        let k = read(&ev).expect("kind");
        assert_eq!(k.id, "video");
        let get = |key: &str| k.fields.iter().find(|f| f.key == key).map(|f| f.value.clone());
        assert_eq!(get("Time").as_deref(), Some("12:34 left of 28:10"));
        assert_eq!(get("State").as_deref(), Some("Playing at 1.25\u{d7}"));
        assert_eq!(get("Channel").as_deref(), Some("Makers Channel"));
        assert_eq!(k.sub, "12:34 left");
    }

    #[test]
    fn dates_read_as_how_long_ago() {
        set_now(1_800_000_000_000);
        let now = chrono::DateTime::from_timestamp_millis(now_ms()).unwrap();
        assert_eq!(ago(&(now - chrono::Duration::hours(2)).to_rfc3339()).as_deref(), Some("2 h ago"));
        assert_eq!(ago(&(now - chrono::Duration::minutes(30)).to_rfc3339()).as_deref(), Some("30 min ago"));
        assert_eq!(ago(&(now - chrono::Duration::days(3)).to_rfc3339()).as_deref(), Some("3 d ago"));
        assert_eq!(ago("not a date"), None);
        assert_eq!(thousands(1204), "1,204");
    }
}
