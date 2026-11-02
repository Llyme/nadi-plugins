//! The Claude Code sessions running on this machine.
//!
//! Claude Code keeps a small registry file per running session in `~/.claude/sessions/<pid>.json` (process id, session id,
//! folder, where it runs, busy/idle/waiting) and the conversation itself in `~/.claude/projects/<folder>/<session id>.jsonl`.
//! Both are read here (the registry is a handful of tiny files, and a transcript is only parsed again when it changed), and the
//! module is told what each session is doing: its title, the prompt it answers, the latest tool calls, how full its context is,
//! what it cost, and which window it runs in. What the module makes of that (cards, a pill when one needs you) is its own.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// how much of the end of a transcript is read: enough for the latest turns, not the whole history
const TAIL_BYTES: u64 = 400_000;

#[derive(Serialize, Clone)]
pub struct FeedItem {
    icon: &'static str,
    text: String,
}

/// a step of the plan a session keeps (its to-do list): `status` is "done", "now" or "next"
#[derive(Serialize, Clone)]
pub struct Todo {
    status: &'static str,
    text: String,
}

/// a file a session changed, with the lines added and taken out
#[derive(Serialize, Clone)]
pub struct FileChange {
    name: String,
    added: u64,
    removed: u64,
}

/// a call that has begun and has no result yet: what a session that waits for you waits to do
#[derive(Serialize, Clone)]
pub struct Pending {
    tool: String,
    icon: &'static str,
    detail: String,
}

#[derive(Clone, Default)]
struct Parsed {
    title: Option<String>,
    prompt: Option<String>,
    model: Option<String>,
    context_tokens: u64,
    cost_usd: f64,
    lines_added: u64,
    lines_removed: u64,
    /// the newest tool calls, newest first
    feed: Vec<FeedItem>,
    /// what the model is doing at the end of the transcript
    last: Option<FeedItem>,
    branch: String,
    /// "normal", "plan"...: how the session is set to ask
    mode: String,
    effort: String,
    todos: Vec<Todo>,
    /// when the prompt that began this turn was written, ms since 1970
    turn_started: u64,
    thinking_ms: u64,
    reply: String,
    files: Vec<FileChange>,
    agents: u32,
    /// tool calls that came back failed (the model goes on from them: they are not the turn failing)
    errors: u32,
    commands: u32,
    pending: Option<Pending>,
    /// how the last turn ended, when it did not end well: "error" (the model's service failed), "interrupted" (stopped by the
    /// person); "" otherwise
    ended: String,
}

#[derive(Clone)]
struct Host {
    label: String,
    icon: &'static str,
    exe: Option<String>,
}

struct Cache {
    parsed: HashMap<PathBuf, (SystemTime, u64, Parsed)>,
    paths: HashMap<String, (PathBuf, Instant)>,
    hosts: HashMap<String, Host>,
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Cache { parsed: HashMap::new(), paths: HashMap::new(), hosts: HashMap::new() }))
}

/// where Claude Code keeps its files: this Windows account, and each running WSL distro
#[derive(Clone)]
struct Root {
    /// the `.claude` folder
    dir: PathBuf,
    /// the distro and its file system as Windows sees it (`\\wsl.localhost\<distro>`); None = Windows
    wsl: Option<(String, PathBuf)>,
}

struct Reg {
    root: Root,
    reg: Value,
}

#[derive(Default)]
struct WslCache {
    at: Option<std::time::Instant>,
    roots: Vec<Root>,
    busy: bool,
}

/// The `.claude` folders of the WSL distros that are running now. Reading a stopped distro's file
/// system would start it, so only running ones are looked at. The list is found by a short
/// `wsl.exe` call on a background thread (at most every 15 s); until it answers, there are none.
fn wsl_roots() -> Vec<Root> {
    static W: OnceLock<Mutex<WslCache>> = OnceLock::new();
    let m = W.get_or_init(|| Mutex::new(WslCache::default()));
    let mut w = m.lock().unwrap();
    let stale = w.at.map_or(true, |t| t.elapsed() > Duration::from_secs(15));
    if stale && !w.busy {
        w.busy = true;
        std::thread::spawn(|| {
            let roots = scan_wsl();
            let mut w = W.get().unwrap().lock().unwrap();
            w.roots = roots;
            w.at = Some(std::time::Instant::now());
            w.busy = false;
        });
    }
    w.roots.clone()
}

/// `wsl.exe` prints UTF-16 when it writes to a pipe
fn decode_wsl_output(b: &[u8]) -> String {
    if b.contains(&0) {
        let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else {
        String::from_utf8_lossy(b).into_owned()
    }
}

fn running_distros(out: &str) -> Vec<String> {
    out.lines()
        .map(|l| l.trim_matches(|c: char| c == '\0' || c == '\u{feff}' || c.is_whitespace()))
        // the helper distros of Docker Desktop hold no sessions; a sentence is a message, not a name
        .filter(|l| !l.is_empty() && !l.contains(' ') && !l.starts_with("docker-desktop"))
        .map(str::to_string)
        .collect()
}

fn scan_wsl() -> Vec<Root> {
    use std::os::windows::process::CommandExt;
    let Some(win) = std::env::var_os("SystemRoot") else { return Vec::new() };
    let exe = PathBuf::from(win).join("System32").join("wsl.exe");
    if !exe.is_file() {
        return Vec::new();
    }
    let Ok(out) = std::process::Command::new(exe).args(["-l", "--running", "-q"]).creation_flags(0x0800_0000).output() else { return Vec::new() };
    let mut roots = Vec::new();
    for name in running_distros(&decode_wsl_output(&out.stdout)) {
        let Some(base) = [format!(r"\\wsl.localhost\{name}"), format!(r"\\wsl$\{name}")].into_iter().map(PathBuf::from).find(|b| b.join("home").is_dir() || b.join("root").is_dir()) else { continue };
        let mut homes = vec![base.join("root")];
        if let Ok(rd) = std::fs::read_dir(base.join("home")) {
            homes.extend(rd.flatten().map(|e| e.path()));
        }
        for h in homes {
            let dir = h.join(".claude");
            if dir.join("sessions").is_dir() {
                roots.push(Root { dir, wsl: Some((name.clone(), base.clone())) });
            }
        }
    }
    roots
}

fn roots() -> Vec<Root> {
    let mut v: Vec<Root> = claude_dir().map(|dir| Root { dir, wsl: None }).into_iter().collect();
    v.extend(wsl_roots());
    v
}

/// is the process of a session still there? In WSL it is the distro's /proc that knows (a Linux
/// process id means nothing to Windows); when /proc cannot be read, a session that changed
/// within the last few hours counts.
fn session_alive(root: &Root, pid: u32, reg: &Value) -> bool {
    let Some((_, base)) = &root.wsl else { return crate::pid_alive(pid) };
    let proc = base.join("proc");
    if proc.join(pid.to_string()).exists() {
        return true;
    }
    if proc.join("1").exists() {
        return false;
    }
    let updated = reg.get("statusUpdatedAt").or_else(|| reg.get("startedAt")).and_then(Value::as_u64).unwrap_or(0);
    now_ms().saturating_sub(updated) < 6 * 3600 * 1000
}

/// the registry file of every Claude Code session on this machine (and in its running WSL distros)
fn registries(alive_only: bool) -> Vec<Reg> {
    let mut out = Vec::new();
    for root in roots() {
        let Ok(rd) = std::fs::read_dir(root.dir.join("sessions")) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().map_or(true, |x| x != "json") {
                continue;
            }
            let Some(reg) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
            let Some(pid) = reg.get("pid").and_then(Value::as_u64) else { continue };
            if alive_only && !session_alive(&root, pid as u32, &reg) {
                continue;
            }
            out.push(Reg { root: root.clone(), reg });
        }
    }
    out
}

/// Claude Code's folder of this account (`NADI_CLAUDE_DIR` says another one: for trying the island out without touching the real one)
pub fn claude_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("NADI_CLAUDE_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".claude"))
}

fn basename(p: &str) -> String {
    p.rsplit(['\\', '/']).find(|s| !s.is_empty()).unwrap_or(p).to_string()
}

fn short(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        format!("{}\u{2026}", s.chars().take(n).collect::<String>())
    }
}

/// a tool call as one line with an icon: what the model is doing right now
fn tool_item(name: &str, input: &Value) -> FeedItem {
    let file = |key: &str| input.get(key).and_then(Value::as_str).map(basename).unwrap_or_default();
    match name {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => FeedItem { icon: "pen", text: format!("Editing {}", file("file_path")) },
        "Read" => FeedItem { icon: "eye", text: format!("Reading {}", file("file_path")) },
        "Bash" | "PowerShell" => {
            let cmd = input.get("command").and_then(Value::as_str).unwrap_or("");
            FeedItem { icon: "term", text: format!("Running {}", short(cmd, 38)) }
        }
        "Grep" | "Glob" => FeedItem { icon: "search", text: "Searching the code".into() },
        "Task" | "Agent" => FeedItem { icon: "sparkle", text: "Running an agent".into() },
        "WebFetch" | "WebSearch" => FeedItem { icon: "globe", text: "Searching the web".into() },
        "TodoWrite" | "ExitPlanMode" => FeedItem { icon: "sparkle", text: "Planning".into() },
        n if n.starts_with("mcp__") => FeedItem { icon: "sparkle", text: format!("Using {}", n.rsplit("__").next().unwrap_or(n)) },
        n => FeedItem { icon: "sparkle", text: n.to_string() },
    }
}

/// the text of a user line that is something the person typed (not a tool result, a system
/// message or a task notification)
fn human_prompt(v: &Value) -> Option<String> {
    if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if let Some(kind) = v.pointer("/origin/kind").and_then(Value::as_str) {
        if kind != "human" {
            return None;
        }
    }
    let content = v.pointer("/message/content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    let t = text.trim();
    // injected context (<system-reminder>, <command-name> ...) is not a prompt
    (!t.is_empty() && !t.starts_with('<')).then(|| t.to_string())
}

/// `2026-10-19T13:03:41.560Z` as ms since 1970 (Claude Code writes its times so)
fn iso_ms(s: &str) -> Option<u64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let clock = rest.trim_end_matches('Z').split(['+', '-']).next()?;
    let mut t = clock.split(':');
    let (h, mi): (i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    let sec: f64 = t.next()?.parse().ok()?;
    // (days from the civil date: the same count the calendar of the plugins uses)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(((days * 24 + h) * 60 + mi) * 60_000 + (sec * 1000.0) as i64).ok()
}

/// how many lines a piece of text has (an empty one has none)
fn line_count(s: &str) -> u64 {
    s.lines().count() as u64
}

fn read_tail(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    // a tail starts mid-line: drop the cut-off first line
    Some(if start > 0 { text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default() } else { text })
}

/// a file a call changed: added to what is known of it
fn touch(files: &mut Vec<FileChange>, path: &str, added: u64, removed: u64) {
    let name = basename(path);
    match files.iter_mut().find(|f| f.name == name) {
        Some(f) => {
            f.added += added;
            f.removed += removed;
        }
        None => files.push(FileChange { name, added, removed }),
    }
}

fn parse_transcript(text: &str) -> Parsed {
    let mut p = Parsed::default();
    let mut calls: Vec<FeedItem> = Vec::new();
    // (the calls that have begun and have no result: id, tool, what it is given)
    let mut open: Vec<(String, String, Value)> = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(b) = v.get("gitBranch").and_then(Value::as_str).filter(|b| !b.is_empty()) {
            p.branch = b.to_string();
        }
        match v.get("type").and_then(Value::as_str) {
            Some("mode") => p.mode = v.get("mode").and_then(Value::as_str).unwrap_or("").to_string(),
            Some("ai-title") => p.title = v.get("aiTitle").and_then(Value::as_str).map(str::to_string).or(p.title.take()),
            Some("cost-state") => {
                p.cost_usd = v.get("totalCostUSD").and_then(Value::as_f64).unwrap_or(p.cost_usd);
                p.lines_added = v.get("totalLinesAdded").and_then(Value::as_u64).unwrap_or(p.lines_added);
                p.lines_removed = v.get("totalLinesRemoved").and_then(Value::as_u64).unwrap_or(p.lines_removed);
            }
            Some("assistant") if v.get("isSidechain").and_then(Value::as_bool) != Some(true) => {
                let Some(m) = v.get("message") else { continue };
                // (an answer that is the service's error: the turn ended on it; any other answer: it goes on)
                p.ended = if v.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true) { "error".into() } else { String::new() };
                if let Some(model) = m.get("model").and_then(Value::as_str) {
                    p.model = Some(model.to_string());
                }
                if let Some(e) = v.get("effort").and_then(Value::as_str).filter(|e| !e.is_empty()) {
                    p.effort = e.to_string();
                }
                if let Some(t) = v.get("thinkingDurationMs").and_then(Value::as_u64) {
                    p.thinking_ms = t;
                }
                if let Some(u) = m.get("usage") {
                    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    let total = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens");
                    if total > 0 {
                        p.context_tokens = total;
                    }
                }
                if let Some(blocks) = m.get("content").and_then(Value::as_array) {
                    for b in blocks {
                        match b.get("type").and_then(Value::as_str) {
                            Some("tool_use") => {
                                let name = b.get("name").and_then(Value::as_str).unwrap_or("tool");
                                let input = b.get("input").unwrap_or(&Value::Null);
                                let item = tool_item(name, input);
                                calls.push(item.clone());
                                p.last = Some(item);
                                open.push((b.get("id").and_then(Value::as_str).unwrap_or("").to_string(), name.to_string(), input.clone()));
                                let text_of = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
                                match name {
                                    "Bash" | "PowerShell" => p.commands += 1,
                                    "Edit" => touch(&mut p.files, text_of("file_path"), line_count(text_of("new_string")), line_count(text_of("old_string"))),
                                    "Write" => touch(&mut p.files, text_of("file_path"), line_count(text_of("content")), 0),
                                    "MultiEdit" => {
                                        for e in input.get("edits").and_then(Value::as_array).into_iter().flatten() {
                                            let t = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("");
                                            touch(&mut p.files, text_of("file_path"), line_count(t("new_string")), line_count(t("old_string")));
                                        }
                                    }
                                    "TodoWrite" => {
                                        p.todos = input
                                            .get("todos")
                                            .and_then(Value::as_array)
                                            .into_iter()
                                            .flatten()
                                            .map(|t| Todo {
                                                status: match t.get("status").and_then(Value::as_str) {
                                                    Some("completed") => "done",
                                                    Some("in_progress") => "now",
                                                    _ => "next",
                                                },
                                                text: short(t.get("content").and_then(Value::as_str).unwrap_or(""), 80),
                                            })
                                            .collect();
                                    }
                                    _ => {}
                                }
                            }
                            Some("thinking") => p.last = Some(FeedItem { icon: "sparkle", text: "Thinking".into() }),
                            Some("text") => {
                                p.last = Some(FeedItem { icon: "sparkle", text: "Replying".into() });
                                if let Some(t) = b.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                                    p.reply = short(t, 260);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            // a tool result arrived: the model is working on it
            Some("user") if v.get("isSidechain").and_then(Value::as_bool) != Some(true) => {
                // (a call has its result: it is no longer waited on; one that failed is counted)
                for b in v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                    if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let id = b.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
                        open.retain(|(i, _, _)| i != id);
                        if b.get("is_error").and_then(Value::as_bool) == Some(true) {
                            p.errors += 1;
                        }
                    }
                }
                // (the person stopped it)
                let stopped = |t: &str| t.trim_start().starts_with("[Request interrupted");
                let interrupted = match v.pointer("/message/content") {
                    Some(Value::String(t)) => stopped(t),
                    Some(Value::Array(a)) => a.iter().any(|b| b.get("text").and_then(Value::as_str).is_some_and(stopped)),
                    _ => false,
                };
                if interrupted {
                    p.ended = "interrupted".into();
                } else if v.get("toolUseResult").is_some() {
                    p.last = Some(FeedItem { icon: "sparkle", text: "Thinking".into() });
                } else if let Some(text) = human_prompt(&v) {
                    p.ended = String::new();
                    // the prompt the model is answering: a queued one only lands here once it is dequeued
                    p.prompt = Some(short(&text, 220));
                    if let Some(t) = v.get("timestamp").and_then(Value::as_str).and_then(iso_ms) {
                        p.turn_started = t;
                    }
                }
            }
            _ => {}
        }
    }
    p.feed = calls.into_iter().rev().take(3).collect();
    p.agents = open.iter().filter(|(_, n, _)| matches!(n.as_str(), "Task" | "Agent")).count() as u32;
    p.pending = open.last().map(|(_, name, input)| {
        let item = tool_item(name, input);
        let detail = match name.as_str() {
            "Bash" | "PowerShell" => short(input.get("command").and_then(Value::as_str).unwrap_or(""), 200),
            _ => item.text.clone(),
        };
        Pending { tool: name.clone(), icon: item.icon, detail }
    });
    p
}

fn transcript_path(session_id: &str, claude: &Path, registry: &Value) -> Option<PathBuf> {
    if let Some(p) = registry.get("logPath").and_then(Value::as_str).map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let mut c = cache().lock().unwrap();
    if let Some((p, _)) = c.paths.get(session_id).filter(|(p, at)| p.is_file() && at.elapsed().as_secs() < 20) {
        return Some(p.clone());
    }
    // the same session can have a transcript in more than one project folder (a renamed or
    // moved project keeps the old copy): the one written last is the live one
    let name = format!("{session_id}.jsonl");
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for dir in std::fs::read_dir(claude.join("projects")).ok()?.flatten() {
        let p = dir.path().join(&name);
        if let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) {
            if best.as_ref().map_or(true, |(t, _)| m > *t) {
                best = Some((m, p));
            }
        }
    }
    let (_, p) = best?;
    c.paths.insert(session_id.to_string(), (p.clone(), Instant::now()));
    Some(p)
}

fn parsed(path: &Path) -> Parsed {
    let Ok(meta) = std::fs::metadata(path) else { return Parsed::default() };
    let (mtime, len) = (meta.modified().unwrap_or(UNIX_EPOCH), meta.len());
    if let Some((m, l, p)) = cache().lock().unwrap().parsed.get(path) {
        if *m == mtime && *l == len {
            return p.clone();
        }
    }
    let p = read_tail(path).map(|t| parse_transcript(&t)).unwrap_or_default();
    cache().lock().unwrap().parsed.insert(path.to_path_buf(), (mtime, len, p.clone()));
    p
}

/// the window a session runs in: VS Code, Cursor, Windows Terminal... found by walking up the
/// process tree once per session (cached)
fn host_of(root: &Root, pid: u32, entrypoint: &str) -> Host {
    let key = match &root.wsl {
        Some((distro, _)) => format!("wsl:{distro}:{pid}"),
        None => pid.to_string(),
    };
    if let Some(h) = cache().lock().unwrap().hosts.get(&key) {
        return h.clone();
    }
    let (label, icon) = match entrypoint {
        "claude-vscode" => ("VS Code", "code"),
        "cli" => ("Terminal", "term"),
        e if e.contains("desktop") => ("Claude Desktop", "sparkle"),
        _ => ("Claude Code", "sparkle"),
    };
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::new().with_exe(UpdateKind::OnlyIfNotSet));
    let mut exe: Option<String> = None;
    if root.wsl.is_some() {
        // the process tree is Linux's, out of sight: the window is whichever editor or terminal
        // of that kind is running on Windows (the window is then picked by the project's name)
        let want: &[&str] = if icon == "code" {
            &["code.exe", "cursor.exe", "code - insiders.exe", "windsurf.exe"]
        } else {
            &["windowsterminal.exe", "wezterm-gui.exe", "alacritty.exe"]
        };
        exe = sys
            .processes()
            .values()
            .find(|p| want.contains(&p.name().to_string_lossy().to_lowercase().as_str()))
            .and_then(|p| p.exe().map(|e| e.to_string_lossy().to_string()));
    } else {
        let mut cur = sysinfo::Pid::from_u32(pid);
        for _ in 0..8 {
            let Some(parent) = sys.process(cur).and_then(|p| p.parent()) else { break };
            let Some(pp) = sys.process(parent) else { break };
            let name = pp.name().to_string_lossy().to_lowercase();
            if ["code.exe", "cursor.exe", "windowsterminal.exe", "code - insiders.exe", "windsurf.exe"].contains(&name.as_str()) {
                exe = pp.exe().map(|p| p.to_string_lossy().to_string());
                // keep walking: the topmost window process is the one with the windows
                cur = parent;
                continue;
            }
            if exe.is_none() && ["pwsh.exe", "powershell.exe", "cmd.exe"].contains(&name.as_str()) {
                exe = pp.exe().map(|p| p.to_string_lossy().to_string());
            }
            cur = parent;
        }
    }
    let host = Host { label: label.to_string(), icon, exe };
    cache().lock().unwrap().hosts.insert(key, host.clone());
    host
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}


/// Every running interactive session, as the module is told of it. A session can have more than one process, so the same
/// `id` may come twice: the module keeps the one that changed last.
pub fn sessions() -> Value {
    let mut out: Vec<Value> = Vec::new();
    for Reg { root, reg } in registries(true) {
        let (Some(pid), Some(id)) = (reg.get("pid").and_then(Value::as_u64), reg.get("sessionId").and_then(Value::as_str)) else { continue };
        let str_of = |k: &str| reg.get(k).and_then(Value::as_str).unwrap_or("");
        if !matches!(str_of("kind"), "interactive" | "") {
            continue;
        }
        let t = transcript_path(id, &root.dir, &reg).map(|p| parsed(&p)).unwrap_or_default();
        let started = reg.get("startedAt").and_then(Value::as_u64).unwrap_or(0);
        let host = host_of(&root, pid as u32, str_of("entrypoint"));
        out.push(json!({
            "id": id,
            "pid": pid,
            "status": str_of("status"),
            "waiting_for": reg.get("waitingFor").and_then(Value::as_str).unwrap_or(""),
            "started": started,
            "updated": reg.get("statusUpdatedAt").and_then(Value::as_u64).unwrap_or(started),
            "name": str_of("name"),
            "cwd": str_of("cwd"),
            "title": t.title,
            "prompt": t.prompt,
            "model": t.model,
            "context_tokens": t.context_tokens,
            "cost_usd": t.cost_usd,
            "lines_added": t.lines_added,
            "lines_removed": t.lines_removed,
            "feed": t.feed,
            "last": t.last,
            "branch": t.branch,
            "mode": t.mode,
            "effort": t.effort,
            "todos": t.todos,
            "turn_started": t.turn_started,
            "thinking_ms": t.thinking_ms,
            "reply": t.reply,
            "files": t.files,
            "agents": t.agents,
            "errors": t.errors,
            "ended": t.ended,
            "commands": t.commands,
            "pending": t.pending,
            "asking": crate::hook::asking(id),
            "host": { "label": host.label, "icon": host.icon, "exe": host.exe },
        }));
    }
    Value::Array(out)
}

