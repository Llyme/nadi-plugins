//! What kind of page is open (a walkthrough, a wiki article, news, a recipe, a question and its answers, an API page, a product, a
//! search, a web app), and what is worth saying about it: read from what the browser extension describes of the page. A plugin asks
//! with `nadi.page_read(page)`; it is pure, and kept here (not in the plugin's script) because it is a lot of careful reading.

pub mod ext;
pub mod kind;
pub mod page;
pub mod text;

use ext::{ExtData, ExtMeta, ExtVideo, GuideLite, LdItem};
use kind::Evidence;
use page::{Node, NodeKind, Page};
use serde_json::{json, Value};

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

/// "github.com" from an address (no "www.")
pub fn domain_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    (!host.is_empty()).then(|| host.to_lowercase())
}

/// A page as the browser extension described it (`{ url, title, nodes, vh, meta, ld, video, words, guide, image }`), read: its kind
/// (null when it is none worth a card), its domain, the opening of its text and its buttons that are safe to press. A page that is never
/// read (banking, mail, health, a sign-in) answers `{ blocked: true }`.
pub fn read(p: &Value, now_ms: u64) -> Value {
    let url = p["url"].as_str().unwrap_or("");
    let title = p["title"].as_str().unwrap_or("");
    if kind::is_blocked(Some(url), title) {
        return json!({ "blocked": true });
    }
    kind::set_now(now_ms);
    let page = page_of(p);
    let data = ext_of(p);
    let ev = Evidence { url: Some(url), title, page: Some(&page), ext: Some(&data) };
    let k = kind::read(&ev);
    let preview = text::preview_from_ext(&page);
    json!({
        "kind": k.map(|k| json!({
            "id": k.id, "label": k.label, "peek": k.peek, "main": k.main, "sub": k.sub,
            "fields": k.fields.iter().map(|f| json!({ "key": f.key, "value": f.value, "rough": f.rough })).collect::<Vec<_>>(),
            "guide": k.guide.map(|g| json!({ "key": g.key, "text": g.text })),
        })),
        "domain": domain_of(url),
        "lead": preview.as_ref().and_then(|p| p.lead.clone()),
        "buttons": preview.map(|p| p.buttons.into_iter().map(|b| b.label).collect::<Vec<_>>()).unwrap_or_default(),
        "image": p["image"],
    })
}

/// Is a window title a private window's?
pub fn is_private_title(title: &str) -> bool {
    kind::is_private_title(title)
}

/// What the browser extension is told: the sites never read, the buttons safe to press, whether pictures are sent.
pub fn config(images: bool) -> Value {
    json!({ "hosts": kind::BLOCK_HOSTS, "hostWords": kind::BLOCK_HOST_WORDS, "titleWords": kind::BLOCK_TITLE_WORDS, "safe": text::SAFE_BUTTONS, "images": images })
}
