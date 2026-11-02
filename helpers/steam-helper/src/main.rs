//! The Steam plugin's own program: it reads Steam's own files on this PC (the games installed, the one running, where Steam keeps
//! each game's pictures) and tells the plugin, a line of JSON each time something changed (`{ "kind": "library", running, games,
//! steam_exe }`). Nothing is asked of Steam, and nothing leaves the PC. It ends when NADI closes its input.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// the library, read again at most this often (the running game is looked at every time)
const LIBRARY_EVERY: Duration = Duration::from_secs(20);

static CACHE: Mutex<Option<(Instant, Vec<Value>)>> = Mutex::new(None);

pub fn root() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
    let p: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(p.replace('/', "\\")))
}

/// The game Steam says runs now (its id; none: none).
fn running() -> Option<u32> {
    use winreg::enums::HKEY_CURRENT_USER;
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
    let id: u32 = key.get_value("RunningAppID").ok()?;
    (id != 0).then_some(id)
}

fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.join("steamapps")];
    if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for line in text.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 4 && parts[1] == "path" {
                let p = PathBuf::from(parts[3].replace("\\\\", "\\")).join("steamapps");
                if !libs.contains(&p) {
                    libs.push(p);
                }
            }
        }
    }
    libs.into_iter().filter(|p| p.is_dir()).collect()
}

fn read_acf(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 4 {
                map.entry(parts[1].to_string()).or_insert_with(|| parts[3].to_string());
            }
        }
    }
    map
}

/// Steam's own tools are not games
fn is_tool(id: u32, name: &str) -> bool {
    matches!(id, 228980 | 1070560 | 1391110 | 1493710 | 2180100 | 1628350 | 1826330) || name.contains("Redistributable") || name.starts_with("Proton") || name.starts_with("Steam Linux Runtime")
}

fn read_library() -> Vec<Value> {
    let Some(root) = root() else { return Vec::new() };
    let mut games = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for lib in libraries(&root) {
        let Ok(rd) = std::fs::read_dir(&lib) else { continue };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            let Some(id) = file.strip_prefix("appmanifest_").and_then(|r| r.strip_suffix(".acf")).and_then(|i| i.parse::<u32>().ok()) else { continue };
            let acf = read_acf(&e.path());
            let name = acf.get("name").cloned().unwrap_or_default();
            // (a library listed twice, written two ways: each game once)
            if name.is_empty() || is_tool(id, &name) || !seen.insert(id) {
                continue;
            }
            let n = |k: &str| acf.get(k).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let flags = n("StateFlags");
            let updating = flags & 1024 != 0;
            let progress = if updating && n("BytesToDownload") > 0 { json!((n("BytesDownloaded") as f64 / n("BytesToDownload") as f64).clamp(0.0, 1.0)) } else { Value::Null };
            // (its pictures, where Steam keeps them: a plugin shows them as "img:<path>")
            let at = |p: Option<PathBuf>| p.map(|p| p.display().to_string());
            games.push(json!({ "id": id, "name": name, "last_played": n("LastPlayed"), "size": n("SizeOnDisk"), "updating": updating, "progress": progress, "icon": at(icon(id)), "art": at(art(id)) }));
        }
    }
    games.sort_by(|a, b| b["last_played"].as_u64().cmp(&a["last_played"].as_u64()).then_with(|| a["name"].as_str().cmp(&b["name"].as_str())));
    games
}

/// The library as the plugins are told it: `{ running, games: [{ id, name, last_played, size, updating, progress, icon, art }] }`.
pub fn state() -> Value {
    let mut cache = CACHE.lock().unwrap();
    let fresh = cache.as_ref().is_some_and(|(at, _)| at.elapsed() < LIBRARY_EVERY);
    if !fresh {
        *cache = Some((Instant::now(), read_library()));
    }
    let games = cache.as_ref().map(|(_, g)| g.clone()).unwrap_or_default();
    // (Steam's own program: its icon is Steam's)
    let exe = root().map(|r| r.join("steam.exe")).filter(|p| p.is_file()).map(|p| p.display().to_string());
    json!({ "running": running(), "games": games, "steam_exe": exe })
}

/// Where Steam keeps a game's small icon (a picture named by its hash, in the game's folder; older Steam: `<id>_icon.jpg`).
pub fn icon(id: u32) -> Option<PathBuf> {
    let cache = root()?.join("appcache").join("librarycache");
    let old = cache.join(format!("{id}_icon.jpg"));
    if old.is_file() {
        return Some(old);
    }
    std::fs::read_dir(cache.join(id.to_string())).ok()?.flatten().map(|e| e.path()).find(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg")))
}

/// Where Steam keeps a game's cover picture (tall, else wide), when it has one.
pub fn art(id: u32) -> Option<PathBuf> {
    let cache = root()?.join("appcache").join("librarycache");
    for name in ["library_600x900.jpg", "library_capsule.jpg", "library_600x900_2x.jpg", "header.jpg", "library_header.jpg"] {
        // (older Steam: flat; newer: a folder for each game, sometimes a folder more inside it)
        let flat = cache.join(format!("{id}_{name}"));
        if flat.is_file() {
            return Some(flat);
        }
        let dir = cache.join(id.to_string());
        if dir.join(name).is_file() {
            return Some(dir.join(name));
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path().join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}


fn main() {
    // (NADI closes its input as it stops the plugin: so it ends too)
    std::thread::spawn(|| {
        let mut line = String::new();
        let stdin = std::io::stdin();
        while stdin.read_line(&mut line).map_or(false, |n| n > 0) {
            line.clear();
        }
        std::process::exit(0);
    });
    let mut last = Value::Null;
    loop {
        let mut now = state();
        now["kind"] = json!("library");
        if now != last {
            use std::io::Write;
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{now}").and_then(|_| out.flush()).is_err() {
                return;
            }
            last = now;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
