//! Answering Claude Code from the island.
//!
//! Claude Code runs a command ("a hook") when a session asks for a permission and when a turn is about to end, and waits for what it
//! prints. Here that command is the Claude plugin's own program (`claude-helper.exe --claude-hook permission`): it tells the running island what the session asks
//! (over a socket that only this PC reaches, with a key kept in a file only this account reads), and waits a short while for the person
//! to answer on the card. An answer is printed in the shape Claude Code reads (allow, deny with a few words, or the words to go on
//! with); no answer, no island, or any trouble prints nothing, and Claude Code asks as it always did.
//!
//! The hooks are put into Claude Code's own settings (`~/.claude/settings.json`) only when the person presses the button for it, and
//! taken out the same way. What was there is kept: only the entries that run this command are touched, and the file as it was is
//! copied once beside it (`settings.json.nadi-backup`).

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// what marks an entry of the settings as ours
const MARK: &str = "--claude-hook";
const FILE: &str = "claude-hook.json";
/// the most a request may say
const MAX_LINE: u64 = 256 * 1024;
/// how long the island waits for an answer unless it is told otherwise
const WAIT_S: u64 = 20;

pub struct Answer {
    pub behavior: String,
    pub text: String,
    /// a question's answers (each question, the choice made): given to Claude with the question
    pub answers: Option<Value>,
}

struct Pending {
    session: String,
    kind: &'static str,
    tool: String,
    icon: &'static str,
    detail: String,
    /// a question Claude asks you (its questions, each with its choices)
    questions: Value,
    at: Instant,
    tx: mpsc::Sender<Answer>,
}

struct Hub {
    port: u16,
    pending: Mutex<HashMap<String, Pending>>,
    wait: AtomicU64,
    next: AtomicU64,
    token: String,
}

static HUB: OnceLock<Hub> = OnceLock::new();
static ERROR: Mutex<String> = Mutex::new(String::new());

fn file() -> Option<PathBuf> {
    // (a test never touches the file of an island that is running)
    if cfg!(test) {
        return None;
    }
    Some(crate::data_dir()?.join(FILE))
}

/// where the settings of Claude Code are (a test says another place)
static SETTINGS: Mutex<Option<PathBuf>> = Mutex::new(None);

fn settings_path() -> Option<PathBuf> {
    if let Some(p) = SETTINGS.lock().unwrap().clone() {
        return Some(p);
    }
    if cfg!(test) {
        return None;
    }
    Some(crate::sessions::claude_dir()?.join("settings.json"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ------------------------------------------------------------------------------------------------------------ the island's side

/// Listen for the hooks (once, when the island starts). The port and the key go into a file that the hook reads.
pub fn start() {
    if HUB.get().is_some() {
        return;
    }
    let mut key = [0u8; 24];
    if getrandom::getrandom(&mut key).is_err() {
        return;
    }
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else { return };
    let Ok(addr) = listener.local_addr() else { return };
    let token = hex(&key);
    let hub = Hub { port: addr.port(), pending: Mutex::new(HashMap::new()), wait: AtomicU64::new(WAIT_S), next: AtomicU64::new(1), token: token.clone() };
    if HUB.set(hub).is_err() {
        return;
    }
    write_file(addr.port(), &token, WAIT_S);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
}

fn write_file(port: u16, token: &str, wait_s: u64) {
    if let Some(p) = file() {
        let _ = std::fs::write(p, json!({ "port": port, "token": token, "wait_s": wait_s }).to_string());
    }
}

/// One hook, connected: what it says is held until it is answered or the wait is over.
fn serve(mut stream: TcpStream) {
    let Some(hub) = HUB.get() else { return };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    {
        let mut reader = BufReader::new((&stream).take(MAX_LINE));
        if reader.read_line(&mut line).is_err() {
            return;
        }
    }
    let Ok(req) = serde_json::from_str::<Value>(&line) else { return };
    if req["token"].as_str() != Some(hub.token.as_str()) {
        return;
    }
    let kind = if req["kind"] == "stop" { "stop" } else { "permission" };
    let session = req["session_id"].as_str().unwrap_or("").chars().take(80).collect::<String>();
    if session.is_empty() {
        return;
    }
    let tool = req["tool_name"].as_str().unwrap_or("").chars().take(60).collect::<String>();
    let (icon, detail) = describe(&tool, &req["tool_input"]);
    let questions = if tool == "AskUserQuestion" { questions_of(&req["tool_input"]) } else { Value::Null };
    let id = format!("r{}", hub.next.fetch_add(1, Ordering::Relaxed));
    let (tx, rx) = mpsc::channel();
    hub.pending.lock().unwrap().insert(id.clone(), Pending { session, kind, tool, icon, detail, questions, at: Instant::now(), tx });
    let waited = rx.recv_timeout(Duration::from_secs(hub.wait.load(Ordering::Relaxed)));
    hub.pending.lock().unwrap().remove(&id);
    let reply = match waited {
        Ok(a) => json!({ "answer": { "behavior": a.behavior, "text": a.text, "answers": a.answers } }),
        Err(_) => json!({}),
    };
    let _ = stream.write_all(format!("{reply}\n").as_bytes());
    let _ = stream.shutdown(Shutdown::Both);
}

/// what a call is to do, as one line with an icon: the command, the file
fn describe(tool: &str, input: &Value) -> (&'static str, String) {
    let text = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    let cut = |s: &str, n: usize| {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if s.chars().count() <= n {
            s
        } else {
            format!("{}\u{2026}", s.chars().take(n).collect::<String>())
        }
    };
    match tool {
        "Bash" | "PowerShell" => ("term", cut(text("command"), 200)),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => ("pen", cut(text("file_path"), 160)),
        "Read" => ("eye", cut(text("file_path"), 160)),
        "WebFetch" => ("globe", cut(text("url"), 160)),
        "WebSearch" => ("globe", cut(text("query"), 160)),
        // (a question: its first question's words)
        "AskUserQuestion" => ("ask", cut(input["questions"][0]["question"].as_str().unwrap_or(""), 160)),
        _ => ("sparkle", cut(&input.to_string(), 160)),
    }
}

/// A question's questions, each with its words, its short name and its choices (only what is shown).
fn questions_of(input: &Value) -> Value {
    let cut = |v: &Value, n: usize| v.as_str().unwrap_or("").chars().take(n).collect::<String>();
    Value::Array(
        input["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .take(4)
            .map(|q| {
                let options: Vec<Value> = q["options"].as_array().into_iter().flatten().take(6).map(|o| json!({ "label": cut(&o["label"], 80), "description": cut(&o["description"], 200) })).collect();
                json!({ "question": cut(&q["question"], 400), "header": cut(&q["header"], 40), "options": options, "multi": q["multiSelect"] == true })
            })
            .collect(),
    )
}

/// The request the island holds for a session: what the card shows and answers.
pub fn asking(session: &str) -> Value {
    let Some(hub) = HUB.get() else { return Value::Null };
    let map = hub.pending.lock().unwrap();
    match map.iter().filter(|(_, p)| p.session == session).min_by_key(|(_, p)| p.at) {
        Some((id, p)) => json!({ "id": id, "kind": p.kind, "tool": p.tool, "icon": p.icon, "detail": p.detail, "questions": p.questions, "age_ms": p.at.elapsed().as_millis() as u64 }),
        None => Value::Null,
    }
}

/// The same, with a question's answers (allowed: Claude is given them).
pub fn answer_with(id: &str, behavior: &str, text: &str, answers: Option<Value>) -> bool {
    let Some(hub) = HUB.get() else { return false };
    let text: String = text.trim().chars().take(2000).collect();
    let map = hub.pending.lock().unwrap();
    let Some(p) = map.get(id) else { return false };
    let behavior = match (p.kind, behavior) {
        ("permission", "allow") => "allow",
        ("permission", "deny") => "deny",
        ("stop", "reply") if !text.is_empty() => "reply",
        _ => return false,
    };
    p.tx.send(Answer { behavior: behavior.into(), text, answers: if behavior == "allow" { answers } else { None } }).is_ok()
}

// ------------------------------------------------------------------------------------------------------------ Claude Code's settings

fn read_settings() -> Result<Value, String> {
    let Some(p) = settings_path() else { return Err("no home folder".into()) };
    match std::fs::read_to_string(&p) {
        Ok(t) if t.trim().is_empty() => Ok(json!({})),
        Ok(t) => serde_json::from_str::<Value>(&t).map_err(|_| "Claude Code's settings file is not valid JSON, so NADI left it as it is.".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("could not read Claude Code's settings: {e}")),
    }
}

fn ours(entry: &Value) -> bool {
    entry["hooks"].as_array().is_some_and(|h| h.iter().any(|x| x["command"].as_str().is_some_and(|c| c.contains(MARK))))
}

/// Which hooks are in Claude Code's settings now, and what went wrong the last time.
pub fn status() -> Value {
    let has = |event: &str| read_settings().ok().is_some_and(|s| s["hooks"][event].as_array().is_some_and(|a| a.iter().any(ours)));
    json!({ "permission": has("PermissionRequest"), "stop": has("Stop"), "error": ERROR.lock().unwrap().clone() })
}

fn write_settings(v: &Value) -> Result<(), String> {
    let Some(p) = settings_path() else { return Err("no home folder".into()) };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // (what was there is kept, once)
    let backup = p.with_extension("json.nadi-backup");
    if p.is_file() && !backup.exists() {
        let _ = std::fs::copy(&p, &backup);
    }
    let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(&p, text).map_err(|e| format!("could not write Claude Code's settings: {e}"))
}

/// Put the hooks into (`"install"`) or take them out of (`"remove"`) Claude Code's settings. `permission` and `stop` say which. The
/// island then waits `wait_s` seconds for an answer.
pub fn set_up(action: &str, permission: bool, stop: bool, wait_s: u64) {
    let result = (|| -> Result<(), String> {
        let mut s = read_settings()?;
        let wait_s = wait_s.clamp(5, 300);
        let exe = std::env::current_exe().map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        if !s.is_object() {
            return Err("Claude Code's settings file is not a JSON object, so NADI left it as it is.".into());
        }
        let hooks = s.as_object_mut().unwrap().entry("hooks").or_insert_with(|| json!({}));
        let Some(hooks) = hooks.as_object_mut() else { return Err("the hooks of Claude Code's settings are not an object, so NADI left them as they are.".into()) };
        for (event, kind, wanted, matcher) in [("PermissionRequest", "permission", permission, true), ("Stop", "stop", stop, false)] {
            let list = hooks.entry(event).or_insert_with(|| json!([]));
            let Some(list) = list.as_array_mut() else { return Err(format!("the {event} hooks are not a list, so NADI left them as they are.")) };
            list.retain(|e| !ours(e));
            if action == "install" && wanted {
                let mut entry = json!({ "hooks": [{ "type": "command", "command": format!("\"{exe}\" {MARK} {kind}"), "timeout": wait_s + 15 }] });
                if matcher {
                    entry["matcher"] = json!("*");
                }
                list.push(entry);
            }
            if list.is_empty() {
                hooks.remove(event);
            }
        }
        if hooks.is_empty() {
            s.as_object_mut().unwrap().remove("hooks");
        }
        write_settings(&s)?;
        if let Some(hub) = HUB.get() {
            hub.wait.store(wait_s, Ordering::Relaxed);
            write_file(hub.port, &hub.token, wait_s);
        }
        Ok(())
    })();
    *ERROR.lock().unwrap() = result.err().unwrap_or_default();
}

// ------------------------------------------------------------------------------------------------------------ the hook's side

/// `nadi.exe --claude-hook permission|stop`: run by Claude Code, not by a person. Returns the exit code, or `None` when this is not it.
pub fn client_from_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(MARK) {
        return None;
    }
    let kind = if args.get(2).map(String::as_str) == Some("stop") { "stop" } else { "permission" };
    // (whatever happens, Claude Code is not held up for long, and is never told it failed)
    let _ = run_client(kind);
    Some(0)
}

fn run_client(kind: &str) -> Option<()> {
    let mut input = String::new();
    let _ = std::io::stdin().take(MAX_LINE).read_to_string(&mut input);
    let payload: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let info: Value = serde_json::from_str(&std::fs::read_to_string(file()?).ok()?).ok()?;
    let port = info["port"].as_u64()? as u16;
    let wait = info["wait_s"].as_u64().unwrap_or(WAIT_S).clamp(5, 300);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(wait + 5))).ok()?;
    let req = json!({
        "token": info["token"],
        "kind": kind,
        "session_id": payload["session_id"],
        "tool_name": payload["tool_name"],
        "tool_input": payload["tool_input"],
    });
    stream.write_all(format!("{req}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    let reply: Value = serde_json::from_str(&line).ok()?;
    let out = output_for(kind, &reply["answer"], &payload["tool_input"])?;
    println!("{out}");
    Some(())
}

/// What Claude Code reads for an answer; a question answered: its answers given with what it asked.
fn output_for(kind: &str, answer: &Value, input: &Value) -> Option<Value> {
    let mut out = output(kind, answer)?;
    if let (Some(answers), true) = (answer["answers"].as_object(), kind == "permission" && answer["behavior"] == "allow") {
        let mut updated = input.clone();
        if updated.is_object() {
            updated["answers"] = Value::Object(answers.clone());
            out["hookSpecificOutput"]["decision"]["updatedInput"] = updated;
        }
    }
    Some(out)
}

/// What Claude Code reads for an answer.
fn output(kind: &str, answer: &Value) -> Option<Value> {
    let behavior = answer["behavior"].as_str()?;
    let text = answer["text"].as_str().unwrap_or("");
    match (kind, behavior) {
        ("permission", "allow") => Some(json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } })),
        ("permission", "deny") => {
            let message = if text.is_empty() { "Denied from the NADI island." } else { text };
            Some(json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "deny", "message": message } } }))
        }
        ("stop", "reply") if !text.is_empty() => Some(json!({ "decision": "block", "reason": text })),
        _ => None,
    }
}

