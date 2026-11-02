// (what the bridge and the reading keep of their own that this program does not use)
#![allow(dead_code)]

//! The NADI Chrome plugin's own program: the bridge to the browser extension (a WebSocket on this PC only), and the reading of the
//! pages it describes (what kind each is, what is worth saying about it). It tells the plugin, a line of JSON when there is something
//! new (`{ "kind": "browser", browser: { browsers, tabs, open, moves, pages } }`, each page with `read`: what it is, and its guide's
//! words in full). The plugin says which pages to follow (`{ "cmd": "watch", pages }`), what the extension may read (`{ "cmd":
//! "config", images }`), and asks for a button pressed, a tab sent on, or a heading brought up (`{ "cmd": "press" | "go" |
//! "scroll", ... }`). A page's picture is kept as a file in its own folder, for the card to show. Nothing leaves the PC. It ends
//! when NADI closes its input.

mod bridge;
mod pagekind;

use base64::Engine;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn data_dir() -> PathBuf {
    std::env::var_os("NADI_PLUGIN_DATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// A page's picture (a `data:` address), kept as a file: where it is.
fn keep_picture(id: u64, data: &str) -> Option<String> {
    let (head, b64) = data.strip_prefix("data:")?.split_once(";base64,")?;
    let ext = match head {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "jpg",
    };
    let dir = data_dir().join("pictures");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{id}.{ext}"));
    if !path.is_file() {
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
        std::fs::write(&path, bytes).ok()?;
        // (only the latest few kept)
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut all: Vec<(std::time::SystemTime, PathBuf)> = rd.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
            all.sort();
            let n = all.len();
            for (_, p) in all.into_iter().take(n.saturating_sub(40)) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Some(path.display().to_string())
}

fn send(v: &Value) -> bool {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{v}").and_then(|_| out.flush()).is_ok()
}

fn main() {
    // (for checking: a page's reading, as the plugin is told it: `{ page, now }` in, its `read` out)
    if std::env::args().nth(1).as_deref() == Some("--read") {
        let mut input = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
        let v: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
        println!("{}", pagekind::read(&v["page"], v["now"].as_u64().unwrap_or(0)));
        return;
    }
    let ext = bridge::state().clone();
    let feed = Arc::new(Mutex::new(bridge::Feed::default()));
    // (what the plugin asks; NADI closes its input as it stops the plugin: so it ends too)
    let (e2, f2) = (ext.clone(), feed.clone());
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        while stdin.read_line(&mut line).map_or(false, |n| n > 0) {
            if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
                match v["cmd"].as_str() {
                    Some("watch") => {
                        f2.lock().unwrap().watch = v["pages"].as_array().into_iter().flatten().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1)?.as_str()?.to_string()))).collect();
                    }
                    Some("config") => {
                        // (what the extensions are told to read; the link is served from now on)
                        e2.serve();
                        e2.set_config(bridge::config_json(&pagekind::config(v["images"].as_bool().unwrap_or(true))));
                    }
                    Some(op @ ("press" | "go" | "scroll")) => {
                        let op = op.to_string();
                        let (browser, title, label, url, heading) = (s("browser"), s("title"), s("label"), s("url"), s("heading"));
                        std::thread::spawn(move || {
                            let ext = bridge::state();
                            if ext.covers(&browser) {
                                let _ = match op.as_str() {
                                    "press" => ext.press(&browser, &label),
                                    "scroll" => ext.scroll(&browser, &title, &heading),
                                    _ => ext.navigate(&browser, &title, &url),
                                };
                            }
                        });
                    }
                    _ => {}
                }
            }
            line.clear();
        }
        std::process::exit(0);
    });
    loop {
        let polled = feed.lock().unwrap().poll(&ext);
        if let Some(mut b) = polled {
            // (each page read here: what kind it is; its guide's words in full; its picture kept as a file)
            if let Some(pages) = b["pages"].as_array_mut() {
                for p in pages.iter_mut() {
                    let mut read = pagekind::read(p, now_ms());
                    if let Some(id) = p["image"].as_u64() {
                        let path = ext.image(id).and_then(|d| keep_picture(id, &d));
                        read["image"] = json!(path);
                    }
                    p["read"] = read;
                    if let Some(key) = p["guide"]["key"].as_str().map(str::to_string) {
                        if let Some(g) = ext.guide(&key) {
                            p["guide"] = serde_json::to_value(&g).unwrap_or(Value::Null);
                        }
                    }
                }
            }
            if !send(&json!({ "kind": "browser", "browser": b })) {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
