//! Work: what you work on. Which kind of program has your focus (coding, writing, design, messaging, media, gaming, browsing),
//! per-day totals, a card for each kind you used lately, the coding card (the file, the project and its git state) and a pill
//! for the session you are in.
//!
//! A real "session" (the one that gets a pill) is confirmed only once a category has held steady for a dwell period: tab
//! switches and alt-tabs should not start a new session every couple of seconds. Ending an active session asks for a longer
//! grace period than starting one: a brief glance at Explorer or Slack should not kill it.
//!
//! It is told every two seconds which windows are open (`processes`, with their programs) and how long the keyboard and the mouse
//! have been left alone (`idle`), keeps the day totals of the last 30 days in its `storage`, finds the folder of the project open
//! in the editor in the editors' own `storage.json` (`files`) and asks `git` about it (`run`).

use nadi_plugin::*;
use std::cell::RefCell;
use std::collections::HashMap;

const IDLE_THRESHOLD_S: f64 = 90.0;
const CONFIRM_MS: u64 = 8_000;
const LEAVE_GRACE_MS: u64 = 45_000;
const NON_SESSION: &[&str] = &["idle", "unknown", "other"];
/// a category still counts as "going on" this long after it was last focused: coding and browsing can both show a card
/// when you are bouncing between them
const RECENT_ACTIVE_MS: u64 = 30_000;
const KEEP_DAYS: usize = 30;
const FLUSH_MS: u64 = 30_000;
const GIT_EVERY_MS: u64 = 6_000;
const MAX_FILES: usize = 5;

const LANGUAGE_EXT: &[(&str, &str)] = &[
    ("py", "Python"), ("pyw", "Python"),
    ("js", "JavaScript"), ("jsx", "JavaScript"), ("mjs", "JavaScript"), ("cjs", "JavaScript"),
    ("ts", "TypeScript"), ("tsx", "TypeScript"),
    ("java", "Java"), ("kt", "Kotlin"), ("kts", "Kotlin"),
    ("c", "C"), ("h", "C"),
    ("cpp", "C++"), ("cc", "C++"), ("cxx", "C++"), ("hpp", "C++"), ("hh", "C++"),
    ("cs", "C#"), ("go", "Go"), ("rs", "Rust"), ("rb", "Ruby"), ("php", "PHP"),
    ("swift", "Swift"), ("m", "Objective-C"), ("mm", "Objective-C"),
    ("scala", "Scala"), ("dart", "Dart"), ("lua", "Lua"), ("r", "R"), ("jl", "Julia"),
    ("hs", "Haskell"), ("ex", "Elixir"), ("exs", "Elixir"), ("erl", "Erlang"),
    ("clj", "Clojure"), ("sql", "SQL"), ("html", "HTML"), ("htm", "HTML"),
    ("css", "CSS"), ("scss", "SCSS"), ("sass", "Sass"), ("less", "Less"),
    ("json", "JSON"), ("yaml", "YAML"), ("yml", "YAML"), ("toml", "TOML"),
    ("xml", "XML"), ("md", "Markdown"), ("sh", "Shell"), ("bash", "Shell"),
    ("zsh", "Shell"), ("ps1", "PowerShell"), ("bat", "Batch"), ("vue", "Vue"),
    ("svelte", "Svelte"), ("proto", "Protobuf"), ("graphql", "GraphQL"),
    ("gql", "GraphQL"), ("ipynb", "Jupyter Notebook"),
];

/// window-title segments that are the editor's own name, not a project
const EDITOR_APP_NAMES: &[&str] = &[
    "visual studio code", "visual studio code - insiders", "code", "vs code",
    "cursor", "sublime text", "notepad++", "android studio", "pycharm",
    "intellij idea", "webstorm", "clion", "rider", "goland", "phpstorm",
    "vim", "neovim",
];

const BROWSER_EXES: &[&str] = &["chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "arc"];

const APP_CATEGORY: &[(&str, &str)] = &[
    ("code", "coding"), ("code - insiders", "coding"), ("codium", "coding"),
    ("devenv", "coding"), ("pycharm64", "coding"), ("idea64", "coding"),
    ("clion64", "coding"), ("webstorm64", "coding"), ("rider64", "coding"),
    ("goland64", "coding"), ("phpstorm64", "coding"), ("sublime_text", "coding"),
    ("notepad++", "coding"), ("vim", "coding"), ("nvim", "coding"), ("cursor", "coding"),
    ("windowsterminal", "coding"), ("cmd", "coding"), ("powershell", "coding"),
    ("pwsh", "coding"), ("wt", "coding"), ("git-bash", "coding"), ("mintty", "coding"),
    ("androidstudio64", "coding"), ("postman", "coding"), ("dbeaver", "coding"),
    ("insomnia", "coding"), ("docker desktop", "coding"), ("gitkraken", "coding"),
    ("winword", "writing"), ("notion", "writing"), ("obsidian", "writing"),
    ("typora", "writing"), ("onenote", "writing"),
    ("figma", "design"), ("figmaagent", "design"), ("photoshop", "design"),
    ("illustrator", "design"), ("blender", "design"), ("inkscape", "design"),
    ("slack", "communication"), ("discord", "communication"), ("teams", "communication"),
    ("outlook", "communication"), ("thunderbird", "communication"), ("zoom", "communication"),
    ("spotify", "media"), ("vlc", "media"),
    ("steam", "gaming"), ("epicgameslauncher", "gaming"), ("battle.net", "gaming"),
];

const TITLE_KEYWORDS: &[(&str, &[&str])] = &[
    ("coding", &["github.com", "gitlab.com", "stackoverflow.com", "localhost", "developer.mozilla", "docs.python.org", "leetcode", "bitbucket.org", "visual studio code", "readthedocs"]),
    ("writing", &["docs.google.com", "notion.so", "overleaf.com"]),
    ("communication", &["mail.google.com", "outlook.office.com", "discord.com", "web.whatsapp.com", "slack.com", "web.telegram.org"]),
    ("media", &["youtube.com", "netflix.com", "twitch.tv", "reddit.com", "spotify.com"]),
];

// ---------------------------------------------------------------------------------------------------- reading titles

/// (project, language) guessed from an editor's window title, best-effort. Most editors title their window
/// "file.ext - project - App Name", with an optional leading bullet on an unsaved file.
fn parse_coding_context(title: &str) -> (Option<String>, Option<String>) {
    if title.is_empty() {
        return (None, None);
    }
    let normalized = title.replace(" — ", " - ").replace(" – ", " - ");
    let parts: Vec<String> = normalized
        .split(" - ")
        .map(|p| p.trim_matches(|c: char| c == ' ' || c == '●' || c == '•' || c == '∙').to_string())
        .filter(|p| !p.is_empty() && !EDITOR_APP_NAMES.contains(&p.to_lowercase().as_str()))
        .collect();
    let Some(filename) = parts.first() else {
        return (None, None);
    };
    let project = if parts.len() >= 2 { parts.last().cloned() } else { None };
    let language = filename.rsplit_once('.').and_then(|(_, ext)| {
        let ext = ext.to_lowercase();
        LANGUAGE_EXT.iter().find(|(e, _)| *e == ext).map(|(_, l)| l.to_string())
    });
    (project, language)
}

/// (file, unsaved) from an editor window title: "● main.rs - project - Visual Studio Code"
fn parse_editor_file(title: &str) -> (Option<String>, bool) {
    let normalized = title.replace(" — ", " - ").replace(" – ", " - ");
    let first = normalized.split(" - ").next().unwrap_or("").trim();
    let unsaved = first.starts_with('●') || first.starts_with('•') || first.starts_with('∙');
    let file = first.trim_matches(|c: char| c == ' ' || c == '●' || c == '•' || c == '∙').to_string();
    let is_editor = EDITOR_APP_NAMES.contains(&file.to_lowercase().as_str());
    // the first title segment is not always a file: chat, settings and preview tabs title themselves ("Settings"). Accept only
    // something that looks like a file name.
    let looks_like_file = file.chars().count() <= 80
        && !file.ends_with('…')
        && (file.rsplit_once('.').map_or(false, |(stem, ext)| !stem.is_empty() && (1..=8).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()))
            || ["makefile", "dockerfile", "license", "readme", "procfile"].contains(&file.to_lowercase().as_str()));
    ((!file.is_empty() && !is_editor && looks_like_file).then_some(file), unsaved)
}

fn categorize(path: &str, title: &str) -> &'static str {
    if path.is_empty() {
        return "other";
    }
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path).to_lowercase();
    let base = file.strip_suffix(".exe").unwrap_or(&file);
    if BROWSER_EXES.contains(&base) {
        let title_l = title.to_lowercase();
        for (category, keywords) in TITLE_KEYWORDS {
            if keywords.iter().any(|k| title_l.contains(k)) {
                return category;
            }
        }
        return "browsing";
    }
    APP_CATEGORY.iter().find(|(exe, _)| *exe == base).map(|(_, cat)| *cat).unwrap_or("other")
}

fn label_for(category: &str) -> &'static str {
    match category {
        "coding" => "Coding",
        "writing" => "Writing",
        "design" => "Designing",
        "communication" => "Messaging",
        "media" => "Watching/Listening",
        "gaming" => "Gaming",
        "browsing" => "Browsing",
        "other" => "On the Computer",
        _ => "Away",
    }
}

/// the line icon a kind of work wears
fn glyph_for(category: &str) -> &'static str {
    match category {
        "coding" => "code",
        "writing" => "pen",
        "design" => "sparkle",
        "communication" => "mail",
        "media" => "music",
        "gaming" => "gamepad",
        "browsing" => "globe",
        _ => "dot",
    }
}

fn exe_display_name(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    file.strip_suffix(".exe").or(file.strip_suffix(".EXE")).unwrap_or(file).to_string()
}

/// "1h 5m", "12m", "<1m"
fn duration(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        "<1m".into()
    }
}

/// file:///c%3A/Workspace/x -> C:\Workspace\x
fn uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file:///")?;
    let raw = rest.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                bytes.push(v);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    Some(String::from_utf8(bytes).ok()?.replace('/', "\\"))
}

/// the folders an editor's `storage.json` knows about
fn editor_folders(text: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let ws = &v["windowsState"];
    let mut uris: Vec<String> = Vec::new();
    if let Some(f) = ws["lastActiveWindow"]["folder"].as_str() {
        uris.push(f.to_string());
    }
    if let Some(list) = ws["openedWindows"].as_array() {
        uris.extend(list.iter().filter_map(|w| w["folder"].as_str().map(str::to_string)));
    }
    if let Some(map) = v["profileAssociations"]["workspaces"].as_object() {
        uris.extend(map.keys().cloned());
    }
    uris.iter().filter_map(|u| uri_to_path(u)).collect()
}

// -------------------------------------------------------------------------------------------------------- git state

#[derive(Clone, Debug, Default, PartialEq)]
struct FileChange {
    name: String,
    path: String,
    added: u32,
    deleted: u32,
    /// 'M' modified, 'A' new
    status: char,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ProjectInfo {
    branch: String,
    files: Vec<FileChange>,
    changed: usize,
    added: u32,
    deleted: u32,
}

/// "<added>\t<deleted>\t<path>" lines from `git diff --numstat` ("-" for binary)
fn parse_numstat(text: &str) -> Vec<(u32, u32, String)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let a = it.next()?;
            let d = it.next()?;
            let path = it.next()?.trim_matches('"').to_string();
            Some((a.parse().unwrap_or(0), d.parse().unwrap_or(0), path))
        })
        .collect()
}

fn project_info(branch: &str, diff: &str, untracked: &str) -> ProjectInfo {
    let mut files: Vec<FileChange> = parse_numstat(diff)
        .into_iter()
        .map(|(added, deleted, path)| FileChange { name: path.rsplit('/').next().unwrap_or(&path).to_string(), path, added, deleted, status: 'M' })
        .collect();
    // the biggest changes first
    files.sort_by_key(|f| std::cmp::Reverse(f.added + f.deleted));
    // (brand-new files: their lines are not counted here, a plugin cannot read them)
    let fresh = untracked.lines().filter(|l| !l.trim().is_empty()).take(300).map(|p| {
        let path = p.trim_matches('"').to_string();
        FileChange { name: path.rsplit('/').next().unwrap_or(&path).to_string(), path, added: 0, deleted: 0, status: 'A' }
    });
    files.extend(fresh);
    let changed = files.len();
    let (added, deleted) = files.iter().fold((0, 0), |(a, d), f| (a + f.added, d + f.deleted));
    files.truncate(MAX_FILES);
    ProjectInfo { branch: branch.to_string(), files, changed, added, deleted }
}

// ------------------------------------------------------------------------------------------------------------ state

struct Recent {
    last_seen: u64,
    started_at: u64,
    app_name: String,
    path: String,
}

#[derive(Default)]
struct Git {
    /// the project it asked about, and when
    project: String,
    at: u64,
    pending: u8,
    branch: Option<(i64, String)>,
    diff: String,
    untracked: String,
}

struct State {
    /// the day totals: { "days": { "2026-10-11": { "categories": {}, "projects": {}, "languages": {} } } }
    stats: Value,
    dirty: bool,
    last_flush: u64,
    recent: HashMap<String, Recent>,
    candidate: String,
    candidate_since: u64,
    candidate_app: String,
    confirmed: Option<String>,
    confirmed_app: String,
    confirmed_since: u64,
    last_tick: u64,
    active_project: String,
    coding_file: Option<String>,
    coding_unsaved: bool,
    /// the icon of each program (told once)
    icons: HashMap<String, String>,
    /// the folders each editor knows, by the file they came from
    folders: HashMap<String, Vec<String>>,
    git: Git,
    project: Option<ProjectInfo>,
}

impl Default for State {
    fn default() -> Self {
        State {
            stats: json!({ "days": {} }),
            dirty: false,
            last_flush: 0,
            recent: HashMap::new(),
            candidate: "idle".into(),
            candidate_since: 0,
            candidate_app: String::new(),
            confirmed: None,
            confirmed_app: String::new(),
            confirmed_since: 0,
            last_tick: 0,
            active_project: String::new(),
            coding_file: None,
            coding_unsaved: false,
            icons: HashMap::new(),
            folders: HashMap::new(),
            git: Git::default(),
            project: None,
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn day_key(ms: u64, tz: i32) -> String {
    let c = civil(ms as i64, tz);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

fn bump(stats: &mut Value, day: &str, kind: &str, key: &str, secs: f64) {
    let slot = &mut stats["days"][day][kind][key];
    *slot = json!(slot.as_f64().unwrap_or(0.0) + secs);
}

fn today_secs(stats: &Value, day: &str, kind: &str, key: &str) -> f64 {
    stats["days"][day][kind][key].as_f64().unwrap_or(0.0)
}

impl State {
    /// One observation: `elapsed` seconds spent in `category`.
    fn observe(&mut self, ev: &Event, category: &str, elapsed: f64, title: &str, app_name: &str, path: &str) {
        let now = ev.now;
        if category == "coding" {
            let (project, _) = parse_coding_context(title);
            let (file, unsaved) = parse_editor_file(title);
            // a tab that is not a file (chat, settings) keeps showing the last real file
            if file.is_some() {
                self.coding_file = file;
                self.coding_unsaved = unsaved;
            }
            self.active_project = project.or_else(|| (!app_name.is_empty()).then(|| app_name.to_string())).unwrap_or_else(|| "your editor".into());
        }
        if elapsed > 0.0 {
            let day = day_key(now, ev.tz_offset);
            bump(&mut self.stats, &day, "categories", category, elapsed);
            if category == "coding" {
                let (project, language) = parse_coding_context(title);
                if let Some(p) = project {
                    bump(&mut self.stats, &day, "projects", &p, elapsed);
                }
                if let Some(l) = language {
                    bump(&mut self.stats, &day, "languages", &l, elapsed);
                }
            }
            self.dirty = true;
        }
        // "other" (unrecognized apps) still counts toward stats but never gets a card
        if category == "other" {
            return;
        }
        let prev_started = self.recent.get(category).filter(|r| now.saturating_sub(r.last_seen) <= RECENT_ACTIVE_MS).map(|r| r.started_at);
        let prev_name = self.recent.get(category).map(|r| r.app_name.clone()).unwrap_or_default();
        self.recent.insert(
            category.to_string(),
            Recent { last_seen: now, started_at: prev_started.unwrap_or(now), app_name: if app_name.is_empty() { prev_name } else { app_name.to_string() }, path: path.to_string() },
        );
    }

    /// the stats as they are kept: only the last 30 days
    fn kept(&mut self) -> Value {
        if let Some(days) = self.stats["days"].as_object_mut() {
            while days.len() > KEEP_DAYS {
                let Some(oldest) = days.keys().next().cloned() else { break };
                days.remove(&oldest);
            }
        }
        self.stats.clone()
    }

    /// where the open project is on disk: the folder of an editor whose last part is its name
    fn project_dir(&self, name: &str) -> Option<String> {
        self.folders.values().flatten().find(|p| p.rsplit('\\').next().map_or(false, |n| n.eq_ignore_ascii_case(name))).cloned()
    }
}

// ------------------------------------------------------------------------------------------------------- the answer

fn card_of(st: &State, ev: &Event) -> (Option<Card>, Vec<Card>) {
    let now = ev.now;
    let day = day_key(now, ev.tz_offset);
    let mut recent: Vec<(&String, &Recent)> = st.recent.iter().filter(|(_, r)| now.saturating_sub(r.last_seen) <= RECENT_ACTIVE_MS).collect();
    recent.sort_by_key(|(_, r)| std::cmp::Reverse(r.started_at));
    let mut cards: Vec<Card> = Vec::new();
    for (category, r) in recent {
        if category == "coding" {
            let session = st.confirmed.as_deref() == Some("coding");
            let project = if st.active_project.is_empty() { "Coding".to_string() } else { st.active_project.clone() };
            let mut card = Card::new().id("coding").icon("code").rank(40).title(project.clone());
            if let Some(icon) = st.icons.get(&r.path) {
                card = card.tile(icon.clone());
            }
            card = card.sub(if session { "Editing" } else { "Not at the keyboard" });
            // what is in front: the file, in its language
            if let Some(file) = &st.coding_file {
                let language = file.rsplit_once('.').and_then(|(_, ext)| {
                    let ext = ext.to_lowercase();
                    LANGUAGE_EXT.iter().find(|(e, _)| *e == ext).map(|(_, l)| *l)
                });
                let unsaved = if st.coding_unsaved { " ●" } else { "" };
                card = card.text(match language {
                    Some(l) => format!("{l} · {file}{unsaved}"),
                    None => format!("{file}{unsaved}"),
                });
            }
            match &st.project {
                Some(p) if p.changed > 0 => {
                    card = card.peek(format!("+{} −{}", p.added, p.deleted));
                    let peak = p.files.iter().map(|f| f.added + f.deleted).max().unwrap_or(1).max(1) as f32;
                    let rows: Vec<(String, String, Option<f32>)> = p
                        .files
                        .iter()
                        .map(|f| {
                            let sum = f.added + f.deleted;
                            let sub = if f.status == 'A' { "new".to_string() } else { format!("+{} −{}", f.added, f.deleted) };
                            (f.name.clone(), sub, (sum > 0).then(|| (sum as f32 / peak).max(0.1)))
                        })
                        .collect();
                    card = card.list(&rows);
                    if p.changed > p.files.len() {
                        card = card.text(format!("+{} more", p.changed - p.files.len()));
                    }
                    card = card.badge(format!("{} · {} files", p.branch, p.changed));
                }
                Some(p) => {
                    card = card.peek("No changes").badge(p.branch.clone());
                }
                None => {
                    // not a git project: the time spent
                    card = card.peek(duration((today_secs(&st.stats, &day, "categories", "coding") * 1000.0) as u64));
                }
            }
            cards.push(card);
        } else if category != "browsing" {
            let label = label_for(category);
            let name = if r.app_name.is_empty() { label.to_string() } else { r.app_name.clone() };
            let mut card = Card::new().id(category).icon(glyph_for(category)).rank(50).title(name).sub(format!("{label} for {}", duration(now.saturating_sub(r.started_at))));
            if let Some(icon) = st.icons.get(&r.path) {
                card = card.tile(icon.clone());
            }
            cards.push(card.peek(duration(now.saturating_sub(r.started_at))));
        }
    }
    let mut it = cards.into_iter();
    let first = it.next();
    (first, it.collect())
}

fn pill_of(st: &State, ev: &Event) -> Option<Pill> {
    let category = st.confirmed.as_deref()?;
    if NON_SESSION.contains(&category) || category == "browsing" {
        return None;
    }
    let mut pill = Pill::new().icon(glyph_for(category)).title(label_for(category)).sub(st.confirmed_app.clone()).right(duration(ev.now.saturating_sub(st.confirmed_since))).rank(10);
    if let Some(icon) = st.recent.get(category).and_then(|r| st.icons.get(&r.path)) {
        pill = pill.image(icon.clone());
    }
    Some(pill)
}

/// one look at the windows: the work session, the cards, the pill
fn look(st: &mut State, ev: &Event) -> Reply {
    let now = ev.now;
    // (the icons of the programs come the first time only)
    for w in &ev.windows {
        if let Some(icon) = &w.icon {
            st.icons.insert(w.path.clone(), icon.clone());
        }
    }
    // every visible window keeps its category's card alive, focused or not (a game or an editor on another monitor). elapsed 0:
    // the stats and the session pill still follow the focus.
    let mut seen: Vec<&'static str> = Vec::new();
    for w in ev.windows.iter().filter(|w| !w.tool) {
        let cat = categorize(&w.path, &w.title);
        if cat == "other" || seen.contains(&cat) {
            continue;
        }
        seen.push(cat);
        let name = if w.name.is_empty() { exe_display_name(&w.path) } else { w.name.clone() };
        st.observe(ev, cat, 0.0, &w.title, &name, &w.path);
    }

    let front = ev.windows.iter().find(|w| w.front && !w.tool);
    let mut title = String::new();
    let mut path = String::new();
    let (new_state, app_name): (String, String) = if ev.idle_s >= IDLE_THRESHOLD_S {
        ("idle".into(), String::new())
    } else {
        match front {
            // (nothing in front while the island is open: it is the island that has the focus; it does not count)
            None if ev.hub => return Reply::nothing(),
            None => ("unknown".into(), String::new()),
            Some(w) => {
                title = w.title.clone();
                path = w.path.clone();
                let name = if w.name.is_empty() { exe_display_name(&w.path) } else { w.name.clone() };
                (categorize(&w.path, &w.title).to_string(), name)
            }
        }
    };

    // stats are counted at every look, not gated behind the session dwell: a tracker should not lose the first seconds of
    // every switch
    let elapsed = (now.saturating_sub(st.last_tick) as f64 / 1000.0).min(6.0);
    st.last_tick = now;
    if new_state != "idle" && new_state != "unknown" {
        st.observe(ev, &new_state, elapsed, &title, &app_name, &path);
    }

    if new_state != st.candidate {
        st.candidate = new_state.clone();
        st.candidate_since = now;
        st.candidate_app = app_name.clone();
    }
    let in_session = st.confirmed.as_deref().map_or(false, |c| !NON_SESSION.contains(&c));
    let dwell = if in_session && NON_SESSION.contains(&new_state.as_str()) { LEAVE_GRACE_MS } else { CONFIRM_MS };
    if now.saturating_sub(st.candidate_since) >= dwell && st.confirmed.as_deref() != Some(new_state.as_str()) {
        st.confirmed = Some(new_state.clone());
        st.confirmed_app = st.candidate_app.clone();
        st.confirmed_since = now;
    }

    let (first, more) = card_of(st, ev);
    let mut r = match first {
        Some(c) => Reply::card(c),
        None => Reply::clear(),
    };
    r = r.more_cards(more);
    r = match pill_of(st, ev) {
        Some(p) => r.pill(p),
        None => r.no_pill(),
    };

    // git, for the project in the editor: only while the island is open (nobody sees it otherwise)
    if ev.hub && !st.active_project.is_empty() && st.git.pending == 0 && now.saturating_sub(st.git.at) >= GIT_EVERY_MS {
        if let Some(dir) = st.project_dir(&st.active_project.clone()) {
            st.git = Git { project: st.active_project.clone(), at: now, pending: 3, ..Git::default() };
            let dir = dir.as_str();
            r = r
                .run("branch", &["git", "-C", dir, "rev-parse", "--abbrev-ref", "HEAD"])
                .run("diff", &["git", "-C", dir, "diff", "--numstat", "--no-renames", "HEAD"])
                .run("untracked", &["git", "-C", dir, "ls-files", "--others", "--exclude-standard"]);
        } else {
            st.project = None;
        }
    }

    // what is kept: now and then, not at every look
    if st.dirty && now.saturating_sub(st.last_flush) >= FLUSH_MS {
        st.last_flush = now;
        st.dirty = false;
        r = r.store(st.kept());
    }
    r
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        match ev.event.as_str() {
            "start" => {
                *st = State::default();
                if ev.store.get("days").is_some() {
                    st.stats = ev.store.clone();
                }
                st.last_flush = ev.now;
                Reply::nothing()
            }
            "file" => {
                st.folders.insert(ev.path.clone(), editor_folders(&ev.text));
                Reply::nothing()
            }
            "command" => {
                match ev.id.as_str() {
                    "branch" => st.git.branch = Some((ev.code, ev.stdout.clone())),
                    "diff" => st.git.diff = if ev.code == 0 { ev.stdout.clone() } else { String::new() },
                    "untracked" => st.git.untracked = if ev.code == 0 { ev.stdout.clone() } else { String::new() },
                    _ => return Reply::nothing(),
                }
                st.git.pending = st.git.pending.saturating_sub(1);
                if st.git.pending == 0 {
                    st.project = match &st.git.branch {
                        Some((0, branch)) if !branch.is_empty() => Some(project_info(branch, &st.git.diff, &st.git.untracked)),
                        _ => None, // not a git project
                    };
                }
                Reply::nothing()
            }
            "processes" => look(&mut st, ev),
            _ => Reply::nothing(),
        }
    })
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    fn win(path: &str, title: &str, front: bool) -> Window {
        Window { exe: path.rsplit('\\').next().unwrap().to_lowercase(), path: path.into(), name: String::new(), title: title.into(), front, ..Window::default() }
    }

    fn look_at(now: u64, windows: Vec<Window>, idle: f64, hub: bool) -> Event {
        Event { event: "processes".into(), now, windows, idle_s: idle, hub, ..Event::default() }
    }

    #[test]
    fn editor_file_only_when_it_looks_like_a_file() {
        assert_eq!(parse_editor_file("● main.rs - dynamic-island - Visual Studio Code"), (Some("main.rs".to_string()), true));
        assert_eq!(parse_editor_file("Cargo.toml - proj - Visual Studio Code").0.as_deref(), Some("Cargo.toml"));
        assert_eq!(parse_editor_file("Brief-show island queue … - dynamic-island - Visual Studio Code").0, None);
        assert_eq!(parse_editor_file("Settings - dynamic-island - Visual Studio Code").0, None);
    }

    #[test]
    fn parses_vscode_title() {
        let (p, l) = parse_coding_context("● main.rs - dynamic-island - Visual Studio Code");
        assert_eq!((p.as_deref(), l.as_deref()), (Some("dynamic-island"), Some("Rust")));
        assert_eq!(parse_coding_context("Untitled - Notepad++"), (None, None));
        assert_eq!(parse_coding_context(""), (None, None));
    }

    #[test]
    fn a_program_has_its_kind() {
        assert_eq!(categorize("C:\\Program Files\\Microsoft VS Code\\Code.exe", "x"), "coding");
        assert_eq!(categorize("C:\\x\\chrome.exe", "Pull request - github.com"), "coding");
        assert_eq!(categorize("C:\\x\\chrome.exe", "Some page"), "browsing");
        assert_eq!(categorize("C:\\x\\unknown.exe", "x"), "other");
        assert_eq!(categorize("", "x"), "other");
    }

    #[test]
    fn a_session_is_confirmed_after_the_dwell_and_left_after_the_grace() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let code = || vec![win("C:\\x\\Code.exe", "main.rs - proj - Visual Studio Code", true)];
        // the first look: not yet a session
        let r = handle(&look_at(10_000, code(), 0.0, false)).to_json();
        assert_eq!(r["pill"], Value::Null);
        // held for the dwell: it is
        handle(&look_at(14_000, code(), 0.0, false));
        let r = handle(&look_at(19_000, code(), 0.0, false)).to_json();
        assert_eq!(r["pill"]["title"], "Coding");
        assert_eq!(r["card"]["title"], "proj");
        assert_eq!(r["card"]["id"], "coding");
        // gone idle: the session holds for the grace, then ends
        let r = handle(&look_at(60_000, code(), 200.0, false)).to_json();
        assert_eq!(r["pill"]["title"], "Coding", "a glance away does not end it");
        handle(&look_at(100_000, code(), 200.0, false));
        let r = handle(&look_at(110_000, code(), 200.0, false)).to_json();
        assert_eq!(r["pill"], Value::Null);
    }

    #[test]
    fn a_browser_has_no_pill_and_the_time_is_kept() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let w = || vec![win("C:\\x\\chrome.exe", "Some page", true)];
        handle(&look_at(1_000, w(), 0.0, false));
        handle(&look_at(3_000, w(), 0.0, false));
        let r = handle(&look_at(40_000, w(), 0.0, false)).to_json();
        assert_eq!(r["pill"], Value::Null, "browsing has no pill: the page reader's cards show it");
        // the stats were counted and are kept after a while
        let r = handle(&look_at(80_000, w(), 0.0, false)).to_json();
        assert!(r["store"]["days"].as_object().map_or(false, |d| d.values().any(|v| v["categories"]["browsing"].as_f64().unwrap_or(0.0) > 10.0)), "{r}");
    }

    #[test]
    fn the_project_is_found_by_its_name_and_git_is_asked_while_the_island_is_open() {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let storage = json!({ "windowsState": { "lastActiveWindow": { "folder": "file:///c%3A/Workspace/proj" } } }).to_string();
        handle(&Event { event: "file".into(), path: "storage.json".into(), text: storage, ..Event::default() });
        let w = || vec![win("C:\\x\\Code.exe", "main.rs - proj - Visual Studio Code", true)];
        // the island is shut: git is not asked
        assert!(handle(&look_at(1_000, w(), 0.0, false)).to_json().get("run").is_none());
        let r = handle(&look_at(7_000, w(), 0.0, true)).to_json();
        let run = r["run"].as_array().unwrap();
        assert_eq!(run.len(), 3);
        assert_eq!(run[0]["line"][2], "c:\\Workspace\\proj");
        // not asked again at once
        assert!(handle(&look_at(9_000, w(), 0.0, true)).to_json().get("run").is_none());
        // the answers come back
        let cmd = |id: &str, code: i64, out: &str| Event { event: "command".into(), id: id.into(), code, stdout: out.into(), ..Event::default() };
        handle(&cmd("branch", 0, "main"));
        handle(&cmd("diff", 0, "12\t3\tsrc/a.rs\n-\t-\timg/x.png"));
        handle(&cmd("untracked", 0, "notes.md"));
        let r = handle(&look_at(11_000, w(), 0.0, true)).to_json();
        assert_eq!(r["card"]["peek"], "+12 −3");
        assert_eq!(r["card"]["body"][0]["text"], "Rust · main.rs");
        assert_eq!(r["card"]["body"][1]["rows"][0]["title"], "a.rs");
        assert_eq!(r["card"]["body"][1]["rows"][0]["sub"], "+12 −3");
        assert_eq!(r["card"]["body"][1]["rows"][2]["sub"], "new");
        assert_eq!(r["card"]["body"][2]["text"], "main · 3 files");
        // not a git project
        STATE.with(|s| s.borrow_mut().git.at = 0);
        handle(&look_at(30_000, w(), 0.0, true));
        handle(&cmd("branch", 128, ""));
        handle(&cmd("diff", 128, ""));
        handle(&cmd("untracked", 128, ""));
        assert!(STATE.with(|s| s.borrow().project.is_none()));
    }

    #[test]
    fn what_is_kept_comes_back_with_the_start() {
        let kept = json!({ "days": { "2026-10-10": { "categories": { "coding": 3600.0 } } } });
        handle(&Event { event: "start".into(), store: kept, ..Event::default() });
        assert_eq!(today_secs(&STATE.with(|s| s.borrow().stats.clone()), "2026-10-10", "categories", "coding"), 3600.0);
    }
}
