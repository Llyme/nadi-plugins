//! The Downloads plugin's own program: what is being downloaded right now (the Chromium browsers and Firefox, from their history and
//! their partial files; Steam, its files growing; qBittorrent, its torrents), a line of JSON every second and a half (`{ "kind":
//! "downloads", observed, ended }`). It opens or shows a finished download it told of, when the plugin asks (`{ "cmd": "open" |
//! "reveal", "path" }`). Nothing leaves the PC. It ends when NADI closes its input.
//!
//! Nothing here talks to those apps; it watches what they leave on disk:
//!
//! * Chromium browsers (Chrome, Edge, Brave, Vivaldi, Opera): a growing `.crdownload` file, named and sized through the
//!   browser's own history database (read in place, read-only).
//! * Firefox: a growing `<name>.part` file.
//! * Steam: files growing under `steamapps/downloading`, named from the app's `appmanifest_*.acf`.
//! * qBittorrent: torrents that are unfinished and not paused (from its `BT_backup` folder) whose files changed in the last
//!   few seconds.
//!
//! The plugin is told what each look saw (`observed`) and, for one that stopped showing up, whether it finished or was given up
//! (`ended`). What it does with that (a card, a pill, how long a finished one stays) is its own.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// a file touched this recently counts as "being downloaded"
const ACTIVE_S: u64 = 15;
/// not seen for this long: check whether it finished
const GONE_S: f64 = 4.0;
/// not seen and not finished for this long: it was cancelled or paused
const DROP_S: f64 = 20.0;

/// what one poll saw for one active download
struct Obs {
    id: String,
    source: String,
    kind: &'static str,
    name: String,
    received: u64,
    total: Option<u64>,
    open_path: Option<String>,
    meta: String,
    /// bytes per second, when the source knows it (else the module works it out from progress)
    speed: Option<f64>,
}

// ---------------------------------------------------------------------------
// shared helpers

fn secs_since(t: SystemTime) -> u64 {
    SystemTime::now().duration_since(t).map_or(0, |d| d.as_secs())
}

fn recent(t: SystemTime) -> bool {
    secs_since(t) <= ACTIVE_S
}

fn downloads_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join("Downloads"))
}

/// files in `dir` (not recursive) ending in one of `exts`, modified lately: (path, size)
fn partials(dir: &Path, exts: &[&str]) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let p = e.path();
            let ext = p.extension()?.to_string_lossy().to_lowercase();
            if !exts.contains(&ext.as_str()) {
                return None;
            }
            let m = e.metadata().ok()?;
            recent(m.modified().ok()?).then(|| (p, m.len()))
        })
        .collect()
}

/// newest modification time, total size and file count under `path` (bounded walk)
fn scan_tree(path: &Path, depth: u32, budget: &mut u32) -> (Option<SystemTime>, u64) {
    let Ok(meta) = std::fs::metadata(path) else { return (None, 0) };
    if meta.is_file() {
        return (meta.modified().ok(), meta.len());
    }
    let mut newest: Option<SystemTime> = None;
    let mut size = 0u64;
    if depth == 0 {
        return (None, 0);
    }
    let Ok(rd) = std::fs::read_dir(path) else { return (None, 0) };
    for e in rd.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let (t, s) = scan_tree(&e.path(), depth - 1, budget);
        size += s;
        newest = newest.max(t);
    }
    (newest, size)
}

// ---------------------------------------------------------------------------
// Chromium browsers

const CHROMIUM: &[(&str, &str)] = &[
    ("Chrome", "chrome.exe"),
    ("Edge", "msedge.exe"),
    ("Brave", "brave.exe"),
    ("Vivaldi", "vivaldi.exe"),
    ("Opera", "opera.exe"),
];

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/._~:-".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The browser keeps its history locked, so it is read in place without taking
/// any lock (`immutable`). A read that lands mid-write just fails; the next poll
/// tries again.
fn open_live(path: &Path) -> Option<rusqlite::Connection> {
    let uri = format!("file:///{}?immutable=1", pct_encode(&path.to_string_lossy().replace('\\', "/")));
    rusqlite::Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()
}

fn chromium_active(dirs: &[PathBuf], tracked: bool) -> Vec<Obs> {
    let any_partial = dirs.iter().any(|d| !partials(d, &["crdownload", "opdownload"]).is_empty());
    if !any_partial && !tracked {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (source, stem) in CHROMIUM {
        let Some(hist) = history_path(stem) else { continue };
        let Some(conn) = open_live(&hist) else { continue };
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, current_path, target_path, received_bytes, total_bytes FROM downloads WHERE state = 0",
        ) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))
        }) else {
            continue;
        };
        for (id, current, target, received_db, total) in rows.flatten() {
            // a row left in "in progress" by a crashed browser has no growing file
            let Ok(meta) = std::fs::metadata(&current) else { continue };
            if !meta.modified().map_or(false, recent) {
                continue;
            }
            let name = Path::new(&target).file_name().map_or_else(|| "Download".into(), |n| n.to_string_lossy().to_string());
            out.push(Obs {
                id: format!("{stem}:{id}"),
                source: source.to_string(),
                kind: "browser",
                name,
                received: meta.len().max(received_db.max(0) as u64),
                total: (total > 0).then_some(total as u64),
                    open_path: Some(target),
                meta: stem.to_string(),
                speed: None,
            });
        }
    }
    out
}

/// state (1 = complete) and final path of one Chromium download
fn chromium_state(stem: &str, id: i64) -> Option<(i64, String)> {
    let hist = history_path(stem)?;
    let conn = open_live(&hist)?;
    conn.query_row("SELECT state, target_path FROM downloads WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok()
}

fn firefox_active(dirs: &[PathBuf]) -> Vec<Obs> {
    let mut out = Vec::new();
    for d in dirs {
        for (part, len) in partials(d, &["part"]) {
            let final_path = part.with_extension("");
            let name = final_path.file_name().map_or_else(|| "Download".into(), |n| n.to_string_lossy().to_string());
            out.push(Obs {
                id: format!("firefox:{}", part.to_string_lossy()),
                source: "Firefox".into(),
                kind: "browser",
                name,
                received: len,
                total: None,
                open_path: Some(final_path.to_string_lossy().to_string()),
                meta: "firefox".into(),
                speed: None,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Steam

fn steam_root() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
    let p: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(p.replace('/', "\\")))
}

/// `steamapps` folders of every Steam library
fn steam_libraries(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.join("steamapps")];
    if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for line in text.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            // \t"path"\t\t"D:\\Games\\Steam"
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

/// top-level `"key" "value"` pairs of an appmanifest
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

/// How fast a process writes to disk, smoothed. A downloading app writes what it
/// receives straight out, so this tracks its download speed -- and it costs
/// nothing to read, unlike walking its files.
#[derive(Default)]
struct IoMeter {
    pid: Option<u32>,
    last: Option<(Instant, u64)>,
    /// bytes per second, smoothed
    rate: f64,
    /// consecutive samples above the caller's threshold
    hits: u32,
}

impl IoMeter {
    fn sample(&mut self, threshold: f64) {
        let now = Instant::now();
        match self.pid.and_then(io_write_bytes) {
            Some(w) => {
                if let Some((t0, w0)) = self.last {
                    let dt = now.duration_since(t0).as_secs_f64();
                    if dt >= 0.5 {
                        let inst = w.saturating_sub(w0) as f64 / dt;
                        self.rate = self.rate * 0.5 + inst * 0.5;
                        self.last = Some((now, w));
                    }
                } else {
                    self.last = Some((now, w));
                }
            }
            None => {
                self.rate = 0.0;
                self.last = None;
            }
        }
        self.hits = if self.rate > threshold { self.hits + 1 } else { 0 };
    }
}

/// Steam keeps no live progress on disk: its manifests only change when a download
/// starts, pauses or finishes. Only the speed can be seen.
#[derive(Default)]
struct SteamMeter {
    io: IoMeter,
    last_call: Option<Instant>,
    /// bytes counted per app since the island first saw it download
    counted: HashMap<String, f64>,
}

/// above this write rate qBittorrent is downloading something (idle, it writes next to nothing)
const QBT_MIN_RATE: f64 = 8_000.0;

/// above this write rate Steam is downloading (idle Steam writes next to nothing)
const STEAM_MIN_RATE: f64 = 300_000.0;

fn io_write_bytes(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetProcessIoCounters, OpenProcess, IO_COUNTERS, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut c = IO_COUNTERS::default();
        let ok = GetProcessIoCounters(h, &mut c).is_ok();
        let _ = CloseHandle(h);
        ok.then_some(c.WriteTransferCount)
    }
}

fn steam_active(root: &Path, m: &mut SteamMeter) -> Vec<Obs> {
    let now = Instant::now();
    m.io.sample(STEAM_MIN_RATE);
    let call_dt = m.last_call.map_or(0.0, |t| now.duration_since(t).as_secs_f64()).min(5.0);
    m.last_call = Some(now);
    if m.io.hits < 2 {
        return Vec::new();
    }
    let rate = m.io.rate;

    // which app: one whose manifest says "update running" (flag 1024); if several are
    // queued, the one whose download folder changed last
    let mut best: Option<(SystemTime, String, PathBuf, HashMap<String, String>)> = None;
    for lib in steam_libraries(root) {
        let Ok(rd) = std::fs::read_dir(&lib) else { continue };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            let Some(appid) = file.strip_prefix("appmanifest_").and_then(|r| r.strip_suffix(".acf")) else { continue };
            let acf = read_acf(&e.path());
            let flags = acf.get("StateFlags").and_then(|f| f.parse::<u32>().ok()).unwrap_or(0);
            if flags & 1024 == 0 {
                continue;
            }
            let touched = [lib.join("downloading").join(appid), e.path()]
                .iter()
                .filter_map(|p| std::fs::metadata(p).ok()?.modified().ok())
                .max()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            if best.as_ref().map_or(true, |(t, ..)| touched > *t) {
                best = Some((touched, appid.to_string(), lib.clone(), acf));
            }
        }
    }
    let Some((_, appid, lib, acf)) = best else { return Vec::new() };
    let total = acf.get("BytesToDownload").and_then(|v| v.parse::<u64>().ok()).filter(|t| *t > 0);
    let base = acf.get("BytesDownloaded").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let counted = m.counted.entry(appid.clone()).or_insert(base);
    *counted += rate * call_dt;
    let received = total.map_or(*counted as u64, |t| (*counted as u64).min(t));
    vec![Obs {
        id: format!("steam:{appid}"),
        source: "Steam".into(),
        kind: "steam",
        name: acf.get("name").cloned().unwrap_or_else(|| format!("App {appid}")),
        received,
        total,
        open_path: None,
        meta: format!("{}|{appid}", lib.to_string_lossy()),
        speed: Some(rate),
    }]
}

/// a Steam download is done when its app is installed and no longer updating
fn steam_finished(meta: &str) -> bool {
    let Some((lib, appid)) = meta.rsplit_once('|') else { return false };
    read_acf(&Path::new(lib).join(format!("appmanifest_{appid}.acf")))
        .get("StateFlags")
        .and_then(|f| f.parse::<u32>().ok())
        .map_or(false, |f| f & 4 != 0 && f & 1024 == 0)
}

// ---------------------------------------------------------------------------
// qBittorrent

#[derive(Clone)]
enum B {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<B>),
    Dict(Vec<(Vec<u8>, B)>),
}

fn bparse(b: &[u8], i: &mut usize) -> Option<B> {
    match *b.get(*i)? {
        b'i' => {
            let end = b[*i..].iter().position(|c| *c == b'e')? + *i;
            let n = std::str::from_utf8(&b[*i + 1..end]).ok()?.parse().ok()?;
            *i = end + 1;
            Some(B::Int(n))
        }
        b'l' => {
            *i += 1;
            let mut v = Vec::new();
            while *b.get(*i)? != b'e' {
                v.push(bparse(b, i)?);
            }
            *i += 1;
            Some(B::List(v))
        }
        b'd' => {
            *i += 1;
            let mut v = Vec::new();
            while *b.get(*i)? != b'e' {
                let Some(B::Bytes(k)) = bparse(b, i) else { return None };
                v.push((k, bparse(b, i)?));
            }
            *i += 1;
            Some(B::Dict(v))
        }
        b'0'..=b'9' => {
            let colon = b[*i..].iter().position(|c| *c == b':')? + *i;
            let len: usize = std::str::from_utf8(&b[*i..colon]).ok()?.parse().ok()?;
            let start = colon + 1;
            let end = start.checked_add(len)?;
            if end > b.len() {
                return None;
            }
            *i = end;
            // (only kept up to a size: resume files carry one byte per piece)
            Some(B::Bytes(if len <= 4_000_000 { b[start..end].to_vec() } else { Vec::new() }))
        }
        _ => None,
    }
}

impl B {
    fn get(&self, key: &str) -> Option<&B> {
        match self {
            B::Dict(v) => v.iter().find(|(k, _)| k == key.as_bytes()).map(|(_, v)| v),
            _ => None,
        }
    }
    fn int(&self) -> Option<i64> {
        if let B::Int(n) = self { Some(*n) } else { None }
    }
    fn text(&self) -> Option<String> {
        if let B::Bytes(b) = self { String::from_utf8(b.clone()).ok().filter(|s| !s.is_empty()) } else { None }
    }
}

#[derive(Clone)]
struct Torrent {
    hash: String,
    name: String,
    /// bytes of the files that are wanted
    total: u64,
    content: PathBuf,
    /// qBittorrent's resume file: the only place real progress is written
    resume: PathBuf,
    /// stopped by the user, as of the resume file's last save
    paused: bool,
}

type TorrentCache = HashMap<String, (SystemTime, SystemTime, Option<Torrent>)>;

fn read_bencode(path: &Path) -> Option<B> {
    let bytes = std::fs::read(path).ok()?;
    let mut i = 0;
    bparse(&bytes, &mut i)
}

/// (share of the wanted pieces downloaded, finished?) from a resume file's piece map.
/// qBittorrent preallocates its files, so their size says nothing about progress.
fn resume_progress(r: &B) -> (f64, bool) {
    let finished = r.get("qBt-seedStatus").and_then(B::int).unwrap_or(0) != 0;
    let have = match r.get("pieces") {
        Some(B::Bytes(b)) => b.as_slice(),
        _ => &[],
    };
    if have.is_empty() {
        return (if finished { 1.0 } else { 0.0 }, finished);
    }
    let wanted = match r.get("piece_priority") {
        Some(B::Bytes(b)) => b.as_slice(),
        _ => &[],
    };
    let (mut need, mut got) = (0u64, 0u64);
    for (i, b) in have.iter().enumerate() {
        if wanted.get(i).map_or(false, |p| *p == 0) {
            continue; // in a file the user skipped
        }
        need += 1;
        got += (b & 1) as u64;
    }
    let frac = if need == 0 { 1.0 } else { got as f64 / need as f64 };
    (frac, finished || frac >= 0.9995)
}

/// Torrents that still miss wanted pieces. Parsed once, and again only when their
/// resume file changes (qBittorrent rewrites it when a torrent is paused, resumed or done).
fn refresh_torrents(dir: &Path, cache: &mut TorrentCache) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut alive: Vec<String> = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().map_or(true, |x| x != "fastresume") {
            continue;
        }
        let Some(hash) = p.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
        alive.push(hash.clone());
        let torrent_path = dir.join(format!("{hash}.torrent"));
        let m1 = e.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
        let m2 = std::fs::metadata(&torrent_path).ok().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
        if cache.get(&hash).map_or(false, |(a, b, _)| *a == m1 && *b == m2) {
            continue;
        }
        let entry = (|| {
            let resume = read_bencode(&p)?;
            if resume_progress(&resume).1 {
                return None;
            }
            let save = resume.get("qBt-savePath").and_then(B::text).or_else(|| resume.get("save_path").and_then(B::text))?;
            let meta = read_bencode(&torrent_path)?;
            let info = meta.get("info")?;
            let name = info.get("name.utf-8").and_then(B::text).or_else(|| info.get("name").and_then(B::text))?;
            // files the user switched to "do not download" (priority 0) are not part of it
            let priorities: Vec<i64> = match resume.get("file_priority") {
                Some(B::List(v)) => v.iter().map(|p| p.int().unwrap_or(1)).collect(),
                _ => Vec::new(),
            };
            let total = match info.get("length").and_then(B::int) {
                Some(n) => n,
                None => match info.get("files") {
                    Some(B::List(files)) => files
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| priorities.get(*i).map_or(true, |p| *p != 0))
                        .filter_map(|(_, f)| f.get("length").and_then(B::int))
                        .sum(),
                    _ => 0,
                },
            };
            let content = PathBuf::from(save.replace('/', "\\")).join(&name);
            let paused = resume.get("paused").and_then(B::int).unwrap_or(0) != 0;
            Some(Torrent { hash: hash.clone(), name, total: total.max(0) as u64, content, resume: p.clone(), paused })
        })();
        cache.insert(hash, (m1, m2, entry));
    }
    cache.retain(|k, _| alive.contains(k));
}

/// per torrent: the resume file the progress was last read from, how many bytes
/// were estimated on top of it since (the resume file is only saved now and then),
/// and whether its files were being written when last looked at
struct TorrentTrack {
    saved_at: Option<SystemTime>,
    extra: f64,
    last: Instant,
    checked: Instant,
    writing: bool,
}

/// files written this recently count as "writing" (qBittorrent flushes its write
/// cache in bursts, so a slow torrent can go quiet for a while)
const TORRENT_WRITING_S: u64 = 120;
/// walking a torrent's files is the costly part: not more often than this
const TORRENT_CHECK_S: f64 = 3.0;

/// Every torrent that is unfinished and not paused: what qBittorrent lists as
/// downloading, stalled or queued. Progress comes from the resume file; the ones
/// whose files are being written also get the client's write rate (shared between
/// them) as their speed, and their progress is carried forward with it until the
/// next save corrects it.
fn torrents_active(cache: &TorrentCache, track: &mut HashMap<String, TorrentTrack>, rate: f64) -> Vec<Obs> {
    let now = Instant::now();
    track.retain(|k, _| cache.contains_key(k));
    let mut listed: Vec<&Torrent> = Vec::new();
    for t in cache.values().filter_map(|(_, _, t)| t.as_ref()).filter(|t| !t.paused) {
        let tr = track.entry(t.hash.clone()).or_insert(TorrentTrack {
            saved_at: None,
            extra: 0.0,
            last: now,
            checked: now - Duration::from_secs(60),
            writing: false,
        });
        if now.duration_since(tr.checked).as_secs_f64() >= TORRENT_CHECK_S {
            let mut budget = 2000;
            let (newest, _) = scan_tree(&t.content, 5, &mut budget);
            tr.writing = newest.map_or(false, |m| secs_since(m) <= TORRENT_WRITING_S);
            tr.checked = now;
        }
        listed.push(t);
    }
    let writing = listed.iter().filter(|t| track.get(&t.hash).map_or(false, |tr| tr.writing)).count();
    let share = if writing == 0 { 0.0 } else { rate / writing as f64 };
    let mut out = Vec::new();
    for t in listed {
        let Some(r) = read_bencode(&t.resume) else { continue };
        let saved = std::fs::metadata(&t.resume).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        let frac = resume_progress(&r).0;
        let tr = track.get_mut(&t.hash).unwrap();
        if tr.saved_at != Some(saved) {
            tr.saved_at = Some(saved);
            tr.extra = 0.0; // a fresh save: the resume file is right again
        }
        let dt = now.duration_since(tr.last).as_secs_f64().min(5.0);
        tr.last = now;
        let speed = if tr.writing { share } else { 0.0 };
        tr.extra += speed * dt;
        let total = t.total as f64;
        let received = if t.total > 0 { (frac * total + tr.extra).min(total * 0.999) } else { 0.0 };
        out.push(Obs {
            id: format!("qbt:{}", t.hash),
            source: "qBittorrent".into(),
            kind: "torrent",
            name: t.name.clone(),
            received: received as u64,
            total: (t.total > 0).then_some(t.total),
            open_path: Some(t.content.to_string_lossy().to_string()),
            meta: t.resume.to_string_lossy().to_string(),
            speed: Some(speed),
        });
    }
    out
}

/// qBittorrent saves the resume file the moment a torrent completes
fn torrent_finished(meta: &str) -> bool {
    read_bencode(Path::new(meta)).map_or(false, |r| resume_progress(&r).1)
}

/// pid of the process with exactly this executable name
fn process_pid(sys: &sysinfo::System, name: &str) -> Option<u32> {
    sys.processes_by_exact_name(std::ffi::OsStr::new(name)).next().map(|p| p.pid().as_u32())
}

fn qbittorrent_running(sys: &mut sysinfo::System) -> bool {
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let found = sys.processes_by_name(std::ffi::OsStr::new("qbittorrent")).next().is_some();
    found
}

// ---------------------------------------------------------------------------
// the sensor

/// what a download that has been seen needs, to be asked about when it stops showing up
struct Tracked {
    kind: &'static str,
    meta: String,
    open_path: Option<String>,
    seen: Instant,
}

struct Sensor {
    sys: sysinfo::System,
    steam: SteamMeter,
    qbt: IoMeter,
    qbt_running: bool,
    qbt_checked: Instant,
    torrents: TorrentCache,
    torrent_track: HashMap<String, TorrentTrack>,
    torrents_at: Instant,
    tracked: HashMap<String, Tracked>,
}

impl Default for Sensor {
    fn default() -> Self {
        let long_ago = Instant::now() - Duration::from_secs(60);
        Sensor {
            sys: sysinfo::System::new(),
            steam: SteamMeter::default(),
            qbt: IoMeter::default(),
            qbt_running: false,
            qbt_checked: long_ago,
            torrents: HashMap::new(),
            torrent_track: HashMap::new(),
            torrents_at: long_ago,
            tracked: HashMap::new(),
        }
    }
}

/// Did a download that no longer shows up finish? (`Some(path)`: yes, and where it is.)
fn finished(t: &Tracked, id: &str) -> Option<Option<String>> {
    let done = match t.kind {
        "steam" => steam_finished(&t.meta),
        "torrent" => torrent_finished(&t.meta),
        _ if t.meta == "firefox" => {
            let fin = t.open_path.as_deref().map_or(false, |p| Path::new(p).exists());
            let part_left = id.strip_prefix("firefox:").map_or(false, |p| Path::new(p).exists());
            fin && !part_left
        }
        _ => match id.split_once(':').and_then(|(stem, id)| Some((stem.to_string(), id.parse::<i64>().ok()?))) {
            Some((stem, id)) => match chromium_state(&stem, id) {
                Some((1, target)) => return Some(Some(target)),
                _ => false,
            },
            None => false,
        },
    };
    done.then(|| t.open_path.clone())
}

impl Sensor {
    /// One look. `{ "observed": [...], "ended": [...] }`: what is downloading now, and what stopped showing up (finished, with
    /// the place of the file; or given up).
    fn poll(&mut self) -> Value {
        let now = Instant::now();
        let dirs: Vec<PathBuf> = downloads_dir().into_iter().collect();
        let tracked_browser = self.tracked.values().any(|t| t.kind == "browser");

        let mut seen: Vec<Obs> = Vec::new();
        seen.extend(chromium_active(&dirs, tracked_browser));
        seen.extend(firefox_active(&dirs));
        if self.qbt_checked.elapsed() > Duration::from_secs(10) {
            self.qbt_running = qbittorrent_running(&mut self.sys);
            self.steam.io.pid = process_pid(&self.sys, "steam.exe");
            self.qbt.pid = process_pid(&self.sys, "qbittorrent.exe");
            self.qbt_checked = now;
        }
        if let Some(root) = steam_root() {
            seen.extend(steam_active(&root, &mut self.steam));
        }
        if self.qbt_running {
            if self.torrents_at.elapsed() > Duration::from_secs(20) {
                if let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("qBittorrent").join("BT_backup")) {
                    refresh_torrents(&dir, &mut self.torrents);
                }
                self.torrents_at = now;
            }
            self.qbt.sample(QBT_MIN_RATE);
            seen.extend(torrents_active(&self.torrents, &mut self.torrent_track, self.qbt.rate));
        }

        let mut observed: Vec<Value> = Vec::new();
        for o in seen {
            self.tracked.insert(o.id.clone(), Tracked { kind: o.kind, meta: o.meta, open_path: o.open_path, seen: now });
            let art = o.id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).and_then(steam_art).map(|p| p.display().to_string());
            observed.push(json!({ "id": o.id, "source": o.source, "kind": o.kind, "name": o.name, "received": o.received, "total": o.total, "speed": o.speed, "art": art }));
        }
        // downloads that stopped showing up: finished, or given up
        let mut ended: Vec<Value> = Vec::new();
        let mut done: Vec<String> = Vec::new();
        for (id, t) in self.tracked.iter() {
            let gone = now.duration_since(t.seen).as_secs_f64();
            if gone < GONE_S {
                continue;
            }
            if let Some(path) = finished(t, id) {
                ended.push(json!({ "id": id, "finished": true, "path": path }));
                done.push(id.clone());
            } else if gone >= DROP_S {
                ended.push(json!({ "id": id, "finished": false }));
                done.push(id.clone());
            }
        }
        for id in done {
            self.tracked.remove(&id);
        }
        json!({ "observed": observed, "ended": ended })
    }
}


// ---------------------------------------------------------------------------------------------------- Chromium's history, found


/// Newest `History` file among a Chromium browser's profiles.
fn history_path(exe: &str) -> Option<PathBuf> {
    let file = exe.rsplit(['\\', '/']).next()?.to_lowercase();
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
    let data_dir = match file.strip_suffix(".exe").unwrap_or(&file) {
        "chrome" => local.join("Google/Chrome/User Data"),
        "msedge" => local.join("Microsoft/Edge/User Data"),
        "brave" => local.join("BraveSoftware/Brave-Browser/User Data"),
        "vivaldi" => local.join("Vivaldi/User Data"),
        "opera" => roaming?.join("Opera Software/Opera Stable"),
        _ => return None,
    };
    let mut best: Option<(SystemTime, PathBuf)> = None;
    let mut consider = |p: PathBuf| {
        if let Ok(t) = std::fs::metadata(&p).and_then(|m| m.modified()) {
            if best.as_ref().map_or(true, |(bt, _)| t > *bt) {
                best = Some((t, p));
            }
        }
    };
    consider(data_dir.join("History")); // Opera keeps it at the top
    if let Ok(rd) = std::fs::read_dir(&data_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == "Default" || name.starts_with("Profile ") {
                consider(e.path().join("History"));
            }
        }
    }
    best.map(|(_, p)| p)
}

// --------------------------------------------------------------------------------------------------------------- Steam's covers

/// Where Steam keeps a game's cover picture (tall, else wide), when it has one.
fn steam_art(id: u32) -> Option<PathBuf> {
    let cache = steam_root()?.join("appcache").join("librarycache");
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


// ------------------------------------------------------------------------------------------------------------------------ its run

fn open(path: &str, reveal: bool) {
    if reveal {
        let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn();
        return;
    }
    use windows::core::HSTRING;
    unsafe {
        let _ = windows::Win32::UI::Shell::ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(path), None, None, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    }
}

fn main() {
    // (what was finished and told of: only those are opened or shown when asked)
    let told: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>> = Default::default();
    let asked = told.clone();
    // (what the plugin asks; NADI closes its input as it stops the plugin: so it ends too)
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        while stdin.read_line(&mut line).map_or(false, |n| n > 0) {
            if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                let path = v["path"].as_str().unwrap_or("");
                if asked.lock().unwrap().contains(path) {
                    match v["cmd"].as_str() {
                        Some("open") => open(path, false),
                        Some("reveal") => open(path, true),
                        _ => {}
                    }
                }
            }
            line.clear();
        }
        std::process::exit(0);
    });
    let mut sensor = Sensor::default();
    loop {
        let mut seen = sensor.poll();
        {
            let mut t = told.lock().unwrap();
            for e in seen["ended"].as_array().into_iter().flatten() {
                if let Some(p) = e["path"].as_str() {
                    t.insert(p.to_string());
                }
            }
            if t.len() > 200 {
                t.clear();
            }
        }
        seen["kind"] = json!("downloads");
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        if writeln!(out, "{seen}").and_then(|_| out.flush()).is_err() {
            return;
        }
        drop(out);
        std::thread::sleep(Duration::from_millis(1500));
    }
}
