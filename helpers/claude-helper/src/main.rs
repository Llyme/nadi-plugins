//! The Claude plugin's own program. It reads the Claude Code sessions running on this machine (from Claude Code's own files) and
//! tells the plugin, a line of JSON every two seconds (`{ "kind": "claude", claude, claude_hooks }`). It answers what a session asks
//! (`{ "cmd": "answer", id, behavior, text, answers }`), and puts its hooks into Claude Code's settings or takes them out when the
//! person presses the button for it (`{ "cmd": "hooks", action, permission, stop, wait_s }`). Run by Claude Code as a hook
//! (`--claude-hook permission|stop`), it hands what the session asks to the running program and prints the answer. Nothing leaves
//! the PC. It ends when NADI closes its input.

mod hook;
mod sessions;

use serde_json::{json, Value};
use std::path::PathBuf;

/// Where it keeps what it needs (NADI names it; run by Claude Code as a hook, it is found the same way).
pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("NADI_PLUGIN_DATA") {
        return Some(PathBuf::from(d));
    }
    let d = PathBuf::from(std::env::var_os("APPDATA")?).join("NADI").join("plugin-data").join("claude");
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

/// Is this process still running?
pub fn pid_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok();
        let _ = CloseHandle(h);
        ok && code == 259
    }
}

fn main() {
    // (run by Claude Code as its hook)
    if let Some(code) = hook::client_from_args() {
        std::process::exit(code);
    }
    hook::start();
    // (what the plugin asks; NADI closes its input as it stops the plugin: so it ends too)
    std::thread::spawn(|| {
        let stdin = std::io::stdin();
        let mut line = String::new();
        while stdin.read_line(&mut line).map_or(false, |n| n > 0) {
            if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                let s = |k: &str, n: usize| v[k].as_str().unwrap_or("").chars().take(n).collect::<String>();
                match v["cmd"].as_str() {
                    Some("answer") => {
                        let (id, behavior, text) = (s("id", 40), s("behavior", 10), s("text", 2000));
                        let answers: serde_json::Map<String, Value> = v["answers"].as_object().map(|m| m.iter().take(8).filter_map(|(k, a)| a.as_str().map(|a| (k.chars().take(500).collect(), Value::String(a.chars().take(1000).collect())))).collect()).unwrap_or_default();
                        if !id.is_empty() {
                            hook::answer_with(&id, &behavior, &text, (!answers.is_empty()).then_some(Value::Object(answers)));
                        }
                    }
                    Some("hooks") => {
                        let action = s("action", 10);
                        if matches!(action.as_str(), "install" | "remove") {
                            let flag = |k: &str| v[k].as_bool().unwrap_or(false);
                            hook::set_up(&action, flag("permission"), flag("stop"), v["wait_s"].as_u64().unwrap_or(20));
                        }
                    }
                    _ => {}
                }
            }
            line.clear();
        }
        std::process::exit(0);
    });
    loop {
        let now = json!({ "kind": "claude", "claude": sessions::sessions(), "claude_hooks": hook::status() });
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        if writeln!(out, "{now}").and_then(|_| out.flush()).is_err() {
            return;
        }
        drop(out);
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
