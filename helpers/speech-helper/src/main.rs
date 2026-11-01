//! Speech's helper. It hears the microphone and what the computer plays, and writes down what is said as it is said (a streaming model,
//! English: words within a fraction of a second, as live captions do), with whose voice each line was (a speaker model). It keeps each
//! session: its lines (Markdown, and JSON lines) and its sound (one WAV for each source). When a session ends, it goes over it again
//! carefully, at low priority (Whisper, the whole of each line, with what was said before it; and every voice of the session grouped at
//! once), and writes the better words and speakers back. A line can be played back from the sound kept.
//!
//! NADI runs it for the Speech plugin and carries lines of JSON both ways:
//!
//! Voices are told apart in both sources (a microphone may be one person, or a meeting room).
//!
//! in:  {"cmd":"listen","mic":true,"system":true,"model":"small","keep_days":7}   {"cmd":"stop"}
//!      {"cmd":"name","speaker":"s2","name":"Kai"}   {"cmd":"play","session":..,"source":"mic"|"system","start":s,"end":s}
//!      {"cmd":"open_sessions"}   {"cmd":"quit"}
//! out: {"kind":"status","state":"downloading"|"listening"|"idle"|"checking"|"error","what":..,"progress":0..1,"text":..}
//!      {"kind":"draft","id":..,"text":..,"speaker":..,"name":..,"source":..}   (what is being said, so far)
//!      {"kind":"line","uid":..,"id":..,"text":..,"speaker":..,"name":..,"at":ms,"time":..,"date":..,"source":..,"session":..,
//!       "start":s,"end":s}
//!      {"kind":"drop","id":..,"source":..}   (a draft that was nothing)
//!      {"kind":"revised","session":..,"lines":[{"uid":..,"text":..,"speaker":..,"name":..}]}   (the careful pass)
//!
//! Everything it needs (the models) and everything it keeps is in the folder NADI gives it (`NADI_PLUGIN_DATA`).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde_json::{json, Value};
use sherpa_rs::speaker_id::{EmbeddingExtractor, ExtractorConfig};
use sherpa_rs_sys as sys;
use std::ffi::{CStr, CString};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const RATE: u32 = 16_000;
const VOICE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx";
/// a voice not named yet is shown as one of these (by its number), easy to tell apart
const ANIMALS: [&str; 16] = ["\u{1F98A}", "\u{1F43C}", "\u{1F438}", "\u{1F989}", "\u{1F427}", "\u{1F419}", "\u{1F984}", "\u{1F42F}", "\u{1F430}", "\u{1F43B}", "\u{1F41D}", "\u{1F422}", "\u{1F98B}", "\u{1F433}", "\u{1F428}", "\u{1F994}"];
/// two pieces of speech this alike are the same voice
const SAME_VOICE: f32 = 0.5;
/// a voice is told from so much speech at least (seconds); a new voice, from more
const VOICE_MIN: f32 = 1.5;
const NEW_VOICE_MIN: f32 = 2.5;
/// sound older than this is not written live (it is kept, and the careful pass has it): the captions never fall behind
const STALE: Duration = Duration::from_millis(1200);

fn animal(id: &str) -> String {
    let n: usize = id.trim_start_matches('s').parse().unwrap_or(1);
    ANIMALS[(n.max(1) - 1) % ANIMALS.len()].to_string()
}

static OUT: Mutex<()> = Mutex::new(());

/// One line to NADI.
fn say(v: Value) {
    let _g = OUT.lock().unwrap();
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{v}");
    let _ = o.flush();
}

fn status(state: &str, extra: Value) {
    let mut v = json!({ "kind": "status", "state": state });
    if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
        o.extend(e);
    }
    say(v);
}

fn data_dir() -> PathBuf {
    std::env::var_os("NADI_PLUGIN_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data"))
}

fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// This thread at low priority (the careful pass: it never slows what is heard live, nor anything else).
fn low_priority() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_LOWEST};
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST);
    }
}

/// A file it needs, downloaded the first time (its progress told as it comes).
fn fetch(url: &str, to: &Path, what: &str) -> Result<(), String> {
    if to.is_file() && std::fs::metadata(to).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(());
    }
    let _ = std::fs::create_dir_all(to.parent().unwrap_or(Path::new(".")));
    status("downloading", json!({ "what": what, "progress": 0.0 }));
    let resp = ureq::get(url).call().map_err(|e| format!("{what}: {e}"))?;
    let total: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let part = to.with_extension("part");
    let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut r = resp.into_reader();
    let mut buf = vec![0u8; 1 << 16];
    let (mut got, mut told) = (0u64, 0.0f64);
    loop {
        let n = r.read(&mut buf).map_err(|e| format!("{what}: {e}"))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        if total > 0 {
            let p = got as f64 / total as f64;
            if p - told >= 0.02 {
                told = p;
                status("downloading", json!({ "what": what, "progress": p }));
            }
        }
    }
    drop(f);
    std::fs::rename(&part, to).map_err(|e| e.to_string())
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Source {
    Mic,
    System,
}

fn source_name(s: Source) -> &'static str {
    if s == Source::Mic {
        "mic"
    } else {
        "system"
    }
}

/// Sound from one source, as 16 kHz mono, in pieces, with when it was heard.
type Chunk = (Source, Vec<f32>, Instant);

/// Start hearing a device (the microphone, or what an output plays): its sound, made 16 kHz mono, sent on.
fn hear(device: &cpal::Device, config: cpal::SupportedStreamConfig, source: Source, tx: Sender<Chunk>) -> Option<cpal::Stream> {
    let channels = config.channels() as usize;
    let step = config.sample_rate().0 as f64 / RATE as f64;
    let carry = Arc::new(Mutex::new((Vec::<f32>::new(), 0.0f64)));
    let push = move |mono: Vec<f32>| {
        let mut c = carry.lock().unwrap();
        c.0.extend(mono);
        let mut out = Vec::with_capacity((c.0.len() as f64 / step) as usize + 1);
        let mut t = c.1;
        while (t as usize) + 1 < c.0.len() {
            let i = t as usize;
            let f = (t - i as f64) as f32;
            out.push(c.0[i] * (1.0 - f) + c.0[i + 1] * f);
            t += step;
        }
        let used = (t as usize).min(c.0.len());
        c.0.drain(..used);
        c.1 = t - used as f64;
        if !out.is_empty() {
            let _ = tx.send((source, out, Instant::now()));
        }
    };
    let err = |e| eprintln!("stream: {e}");
    let cfg: cpal::StreamConfig = config.clone().into();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(&cfg, move |d: &[f32], _: &_| push(d.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect()), err, None),
        cpal::SampleFormat::I16 => device.build_input_stream(&cfg, move |d: &[i16], _: &_| push(d.chunks(channels).map(|f| f.iter().map(|x| *x as f32 / 32768.0).sum::<f32>() / channels as f32).collect()), err, None),
        _ => return None,
    }
    .ok()?;
    stream.play().ok()?;
    Some(stream)
}

// ------------------------------------------------------------------------------------------------------------------------- voices

/// a person keeps this many prints of their voice at most (how they sound on one microphone, in one room, over one call)
const PRINTS: usize = 8;
/// a voice this like a print it has is that print (learnt into it); less like it, though still theirs, is a new print of them
const SAME_PRINT: f32 = 0.7;

/// One voice it knows: its id, the name and animal it was given, whether it is the person using this computer, its prints, and how
/// often it was heard.
struct Voice {
    id: String,
    name: String,
    animal: String,
    me: bool,
    prints: Vec<(Vec<f32>, u32)>,
    heard: u32,
}

/// The voices it knows (kept between runs: a person named once is known again in any session, on either source).
struct Voices {
    path: PathBuf,
    list: Vec<Voice>,
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut d, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        d += x * y;
        na += x * x;
        nb += y * y;
    }
    d / (na.sqrt() * nb.sqrt()).max(1e-6)
}

fn floats(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|x| x as f32)).collect()).unwrap_or_default()
}

impl Voice {
    /// How like this voice a print is: like its likest print.
    fn like(&self, e: &[f32]) -> (usize, f32) {
        self.prints.iter().enumerate().map(|(i, p)| (i, cosine(&p.0, e))).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or((0, -1.0))
    }

    /// Heard again: learnt into its likest print, or kept as a print of its own when it sounds new for them.
    fn learn(&mut self, e: &[f32]) {
        let (i, s) = self.like(e);
        if !self.prints.is_empty() && (s >= SAME_PRINT || self.prints.len() >= PRINTS) {
            if let Some(p) = self.prints.get_mut(i) {
                let n = p.1.min(50) as f32;
                for (a, b) in p.0.iter_mut().zip(e) {
                    *a = (*a * n + b) / (n + 1.0);
                }
                p.1 += 1;
            }
        } else {
            self.prints.push((e.to_vec(), 1));
        }
        self.heard += 1;
    }

    fn shown(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else {
            self.face()
        }
    }

    fn face(&self) -> String {
        if self.animal.is_empty() {
            animal(&self.id)
        } else {
            self.animal.clone()
        }
    }
}

impl Voices {
    fn load(path: PathBuf) -> Voices {
        let list = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| {
                let mut prints: Vec<(Vec<f32>, u32)> = v["prints"].as_array().map(|a| a.iter().map(|p| (floats(&p["v"]), p["n"].as_u64().unwrap_or(1) as u32)).filter(|p| !p.0.is_empty()).collect()).unwrap_or_default();
                if prints.is_empty() {
                    prints.push((floats(&v["voice"]), 1));
                }
                Some(Voice {
                    id: v["id"].as_str()?.to_string(),
                    name: v["name"].as_str().unwrap_or("").to_string(),
                    animal: v["animal"].as_str().unwrap_or("").to_string(),
                    me: v["me"] == true,
                    prints,
                    heard: v["heard"].as_u64().unwrap_or(1) as u32,
                })
            })
            .collect();
        Voices { path, list }
    }

    fn save(&self) {
        let v: Vec<Value> = self
            .list
            .iter()
            .map(|x| json!({ "id": x.id, "name": x.name, "animal": x.animal, "me": x.me, "heard": x.heard, "prints": x.prints.iter().map(|p| json!({ "v": p.0, "n": p.1 })).collect::<Vec<_>>() }))
            .collect();
        let _ = std::fs::write(&self.path, serde_json::to_string(&v).unwrap_or_default());
    }

    /// What the plugin shows of them: each voice's id, name, animal, and whether it is you.
    fn info(&self) -> Value {
        json!(self.list.iter().map(|x| json!({ "id": x.id, "name": x.name, "animal": x.face(), "me": x.me, "heard": x.heard })).collect::<Vec<_>>())
    }

    fn label(&self, i: usize) -> (String, String) {
        (self.list[i].id.clone(), self.list[i].shown())
    }

    fn label_of(&self, id: &str) -> Option<(String, String)> {
        self.list.iter().position(|v| v.id == id).map(|i| self.label(i))
    }

    fn best(&self, e: &[f32]) -> Option<(usize, f32)> {
        self.list.iter().enumerate().map(|(i, v)| (i, v.like(e).1)).max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The voice most like this one, when it is alike enough (no new one).
    fn known(&self, e: &[f32]) -> Option<(String, String)> {
        self.best(e).filter(|(_, s)| *s >= SAME_VOICE).map(|(i, _)| self.label(i))
    }

    fn next_id(&self) -> String {
        let n = self.list.iter().filter_map(|v| v.id.trim_start_matches('s').parse::<usize>().ok()).max().unwrap_or(0);
        format!("s{}", n + 1)
    }

    /// Voices heard once, never named, and not among `keep`: strays (a short noisy clip), forgotten.
    fn forget_strays(&mut self, keep: &std::collections::HashSet<String>) {
        self.list.retain(|v| !v.name.is_empty() || v.me || !v.animal.is_empty() || v.heard > 1 || keep.contains(&v.id));
        self.save();
    }

    /// The voice most like this one (at least so alike), else a new one; learning it a little more each time.
    fn who(&mut self, e: &[f32]) -> (String, String) {
        let i = match self.best(e) {
            Some((i, s)) if s >= SAME_VOICE => {
                self.list[i].learn(e);
                i
            }
            _ => {
                let id = self.next_id();
                self.list.push(Voice { id, name: String::new(), animal: String::new(), me: false, prints: vec![(e.to_vec(), 1)], heard: 1 });
                self.list.len() - 1
            }
        };
        self.save();
        self.label(i)
    }

    fn name(&mut self, id: &str, name: &str) {
        if let Some(v) = self.list.iter_mut().find(|v| v.id == id) {
            v.name = name.trim().chars().take(40).collect();
            self.save();
        }
    }

    fn set_animal(&mut self, id: &str, a: &str) {
        if let Some(v) = self.list.iter_mut().find(|v| v.id == id) {
            v.animal = a.chars().take(8).collect();
            self.save();
        }
    }

    /// This voice is the person using this computer (only one is).
    fn set_me(&mut self, id: &str, on: bool) {
        for v in self.list.iter_mut() {
            v.me = if v.id == id { on } else { v.me && !on };
        }
        self.save();
    }

    /// Two voices that are one person: `from`'s prints go to `into` (its name, animal and mark kept, else `from`'s), `from` is gone.
    fn merge(&mut self, into: &str, from: &str) -> bool {
        if into == from {
            return false;
        }
        let Some(fi) = self.list.iter().position(|v| v.id == from) else { return false };
        if !self.list.iter().any(|v| v.id == into) {
            return false;
        }
        let f = self.list.remove(fi);
        let t = self.list.iter_mut().find(|v| v.id == into).unwrap();
        t.prints.extend(f.prints);
        t.prints.sort_by(|a, b| b.1.cmp(&a.1));
        t.prints.truncate(PRINTS);
        t.heard += f.heard;
        if t.name.is_empty() {
            t.name = f.name;
        }
        if t.animal.is_empty() {
            t.animal = f.animal;
        }
        t.me |= f.me;
        self.save();
        true
    }
}

/// Every session's lines of one speaker given another's label (a voice renamed, or two made one); its Markdown made again. Lines of
/// the session `only`, if given; the lines of each session changed are said back (`revised`).
fn relabel(from: &str, to: &(String, String), only: Option<&str>) {
    let Ok(rd) = std::fs::read_dir(sessions_dir()) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let mut lines: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let mut changed = false;
        for l in lines.iter_mut().filter(|l| l["speaker"] == from) {
            l["speaker"] = json!(to.0);
            l["name"] = json!(to.1);
            changed = true;
        }
        if !changed {
            continue;
        }
        let _ = std::fs::write(&p, lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
        write_md(&name, &lines);
        let revised: Vec<Value> = lines.iter().map(|l| json!({ "uid": l["uid"], "text": l["text"], "speaker": l["speaker"], "name": l["name"] })).collect();
        say(json!({ "kind": "revised", "session": name, "lines": revised }));
    }
}

/// A session's Markdown, made again from its lines (its heading kept).
fn write_md(name: &str, lines: &[Value]) {
    let md = sessions_dir().join(format!("{name}.md"));
    let head = std::fs::read_to_string(&md).ok().and_then(|t| t.lines().next().map(str::to_string)).unwrap_or_else(|| format!("# {name}"));
    let body: String = lines.iter().map(|l| format!("**{}** ({}): {}\n\n", l["name"].as_str().unwrap_or(""), l["clock"].as_str().unwrap_or(""), l["text"].as_str().unwrap_or(""))).collect();
    let _ = std::fs::write(&md, format!("{head}\n\n{body}"));
}

/// The sessions kept, newest first: when, how long, who was in them, their first words.
/// A session's lines: its JSON lines, else (one kept before those were) read from its Markdown, `**who** (hh:mm:ss): words`.
fn session_lines(name: &str) -> Vec<Value> {
    let dir = sessions_dir();
    if let Ok(text) = std::fs::read_to_string(dir.join(format!("{name}.jsonl"))) {
        return text.lines().filter_map(|l| serde_json::from_str(l).ok()).take(3000).collect();
    }
    let Ok(text) = std::fs::read_to_string(dir.join(format!("{name}.md"))) else { return Vec::new() };
    let day = name.get(..10).and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let mut out = Vec::new();
    for l in text.lines() {
        let Some(rest) = l.strip_prefix("**") else { continue };
        let Some((who, rest)) = rest.split_once("** (") else { continue };
        let Some((clock, words)) = rest.split_once("): ") else { continue };
        let at = day
            .zip(chrono::NaiveTime::parse_from_str(clock, "%H:%M:%S").ok())
            .and_then(|(d, t)| chrono::TimeZone::from_local_datetime(&chrono::Local, &d.and_time(t)).single())
            .map(|t| t.timestamp_millis());
        out.push(json!({ "speaker": format!("old:{who}"), "name": who, "text": words.trim(), "clock": clock, "at": at }));
        if out.len() >= 3000 {
            break;
        }
    }
    out
}

fn sessions_list() -> Value {
    let Ok(rd) = std::fs::read_dir(sessions_dir()) else { return json!([]) };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl" || x == "md"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        .collect();
    names.sort();
    names.dedup();
    names.reverse();
    let mut out = Vec::new();
    for name in names.into_iter().take(120) {
        let lines = session_lines(&name);
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else { continue };
        let mut people: Vec<String> = Vec::new();
        for l in &lines {
            if let Some(s) = l["speaker"].as_str() {
                if !people.iter().any(|p| p == s) {
                    people.push(s.to_string());
                }
            }
        }
        let secs = (last["end"].as_f64().unwrap_or(0.0)).max((last["at"].as_i64().unwrap_or(0) - first["at"].as_i64().unwrap_or(0)) as f64 / 1000.0);
        let words: String = first["text"].as_str().unwrap_or("").chars().take(80).collect();
        out.push(json!({ "name": name, "at": first["at"], "secs": secs, "people": people, "first": words, "audio": sessions_dir().join(format!("{name} system.wav")).is_file() || sessions_dir().join(format!("{name} mic.wav")).is_file() }));
    }
    json!(out)
}

// ----------------------------------------------------------------------------------------------------------------------- words

/// What Whisper writes for sound that is not speech.
fn is_noise(t: &str) -> bool {
    let raw = t.trim();
    let l = raw.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    l.is_empty() || raw.starts_with('[') || raw.starts_with('(') || raw.starts_with('*') || ["thank you", "thanks for watching", "you", "bye", "music"].contains(&l.as_str())
}

/// Its words without the trailing-off dots Whisper puts in ("so then..." is "so then"; "well... yes" is "well, yes").
fn tidy(t: &str) -> String {
    let mut s = t.replace('\u{2026}', "...");
    while s.contains("....") {
        s = s.replace("....", "...");
    }
    let s = s.trim().trim_end_matches("...").trim_start_matches("...").trim().replace("... ", ", ").replace("...", " ");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[allow(dead_code)]
/// Write down a piece of sound with Whisper, carefully, with what was said just before it (`before`).
fn transcribe(ctx: &WhisperContext, samples: &[f32], threads: i32, before: &str) -> String {
    let Ok(mut state) = ctx.create_state() else { return String::new() };
    let mut p = FullParams::new(SamplingStrategy::BeamSearch { beam_size: 3, patience: 1.0 });
    p.set_language(Some("en"));
    p.set_n_threads(threads);
    p.set_suppress_blank(true);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_special(false);
    p.set_print_timestamps(false);
    p.set_no_timestamps(true);
    p.set_no_speech_thold(0.6);
    if !before.is_empty() {
        p.set_initial_prompt(before);
    }
    let secs = samples.len() as f32 / RATE as f32;
    p.set_audio_ctx(((secs * 50.0) as i32 + 64).clamp(256, 1500));
    if state.full(p, samples).is_err() {
        return String::new();
    }
    let n = state.full_n_segments().unwrap_or(0);
    let text = (0..n).filter_map(|i| state.full_get_segment_text(i).ok()).collect::<Vec<_>>().join(" ");
    if is_noise(&text) {
        String::new()
    } else {
        tidy(&text)
    }
}

/// Its words as words are written ("HELLO THERE I AM" is "Hello there I am").
fn pretty(t: &str) -> String {
    let words: Vec<String> = t
        .split_whitespace()
        .map(|w| {
            let l = w.to_lowercase();
            if l == "i" || l.starts_with("i'") {
                format!("I{}", &l[1..])
            } else {
                l
            }
        })
        .collect();
    let s = words.join(" ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

// ----------------------------------------------------------------------------------------------------------------- the live side

const STREAM_HF: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26/resolve/main/";
const STREAM_FILES: [(&str, &str); 4] = [
    ("encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx", "encoder.onnx"),
    ("decoder-epoch-99-avg-1-chunk-16-left-128.onnx", "decoder.onnx"),
    ("joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx", "joiner.onnx"),
    ("tokens.txt", "tokens.txt"),
];
/// a draft at most this often, per source
const DRAFT_GAP: Duration = Duration::from_millis(100);
/// what a line keeps of its sound, at most (for its voice)
const LINE_MAX: usize = (RATE as usize) * 30;

/// The streaming recognizer (one, for both sources), made in the thread that uses it.
struct Recognizer {
    r: *const sys::SherpaOnnxOnlineRecognizer,
    _keep: Vec<CString>,
}

impl Recognizer {
    fn new(dir: &Path) -> Option<Recognizer> {
        let c = |s: String| CString::new(s).ok();
        let path = |f: &str| c(dir.join(f).to_string_lossy().to_string());
        let (enc, dec, join, tok) = (path("encoder.onnx")?, path("decoder.onnx")?, path("joiner.onnx")?, path("tokens.txt")?);
        let (greedy, cpu) = (c("greedy_search".into())?, c("cpu".into())?);
        unsafe {
            let mut cfg: sys::SherpaOnnxOnlineRecognizerConfig = std::mem::zeroed();
            cfg.feat_config.sample_rate = RATE as i32;
            cfg.feat_config.feature_dim = 80;
            cfg.model_config.transducer.encoder = enc.as_ptr();
            cfg.model_config.transducer.decoder = dec.as_ptr();
            cfg.model_config.transducer.joiner = join.as_ptr();
            cfg.model_config.tokens = tok.as_ptr();
            cfg.model_config.num_threads = 2;
            cfg.model_config.provider = cpu.as_ptr();
            cfg.decoding_method = greedy.as_ptr();
            cfg.max_active_paths = 4;
            // (a line ends: after 2 s of nothing said, after 0.6 s of quiet once something was, or at 12 s)
            cfg.enable_endpoint = 1;
            cfg.rule1_min_trailing_silence = 2.0;
            cfg.rule2_min_trailing_silence = 0.6;
            cfg.rule3_min_utterance_length = 12.0;
            let r = sys::SherpaOnnxCreateOnlineRecognizer(&cfg);
            if r.is_null() {
                return None;
            }
            Some(Recognizer { r, _keep: vec![enc, dec, join, tok, greedy, cpu] })
        }
    }

    fn stream(&self) -> *const sys::SherpaOnnxOnlineStream {
        unsafe { sys::SherpaOnnxCreateOnlineStream(self.r) }
    }

    fn hear(&self, s: *const sys::SherpaOnnxOnlineStream, samples: &[f32]) {
        unsafe {
            sys::SherpaOnnxOnlineStreamAcceptWaveform(s, RATE as i32, samples.as_ptr(), samples.len() as i32);
            while sys::SherpaOnnxIsOnlineStreamReady(self.r, s) == 1 {
                sys::SherpaOnnxDecodeOnlineStream(self.r, s);
            }
        }
    }

    fn text(&self, s: *const sys::SherpaOnnxOnlineStream) -> String {
        unsafe {
            let res = sys::SherpaOnnxGetOnlineStreamResult(self.r, s);
            if res.is_null() {
                return String::new();
            }
            let t = if (*res).text.is_null() { String::new() } else { CStr::from_ptr((*res).text).to_string_lossy().to_string() };
            sys::SherpaOnnxDestroyOnlineRecognizerResult(res);
            t
        }
    }

    fn ended(&self, s: *const sys::SherpaOnnxOnlineStream) -> bool {
        unsafe { sys::SherpaOnnxOnlineStreamIsEndpoint(self.r, s) == 1 }
    }

    fn reset(&self, s: *const sys::SherpaOnnxOnlineStream) {
        unsafe { sys::SherpaOnnxOnlineStreamReset(self.r, s) }
    }
}

/// The session now: its name, its Markdown, and whether its sound is kept.
#[derive(Clone)]
struct Session {
    name: String,
    md: PathBuf,
    record: bool,
}

/// A line that has ended: its source, its number, its sound, its words, and where it is in the session's sound (seconds).
struct Line {
    src: Source,
    id: u64,
    samples: Vec<f32>,
    text: String,
    start: f64,
    end: f64,
}

/// The session's sound being kept: one WAV for each source, and how much of each so far.
struct Recorder {
    name: String,
    writers: [Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>; 2],
    count: [u64; 2],
}

impl Recorder {
    fn none() -> Recorder {
        Recorder { name: String::new(), writers: [None, None], count: [0, 0] }
    }

    fn open(s: &Session) -> Recorder {
        let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let w = |src: &str| if s.record { hound::WavWriter::create(sessions_dir().join(format!("{} {src}.wav", s.name)), spec).ok() } else { None };
        Recorder { name: s.name.clone(), writers: [w("mic"), w("system")], count: [0, 0] }
    }

    fn write(&mut self, k: usize, samples: &[f32]) {
        if let Some(w) = self.writers[k].as_mut() {
            for x in samples {
                let _ = w.write_sample((x.clamp(-1.0, 1.0) * 32767.0) as i16);
            }
        }
        self.count[k] += samples.len() as u64;
    }

    /// Its sound finished (whole files), and handed to the careful pass.
    fn close(&mut self, checks: &Sender<String>) {
        let mut kept = false;
        for w in self.writers.iter_mut() {
            if let Some(w) = w.take() {
                kept |= w.finalize().is_ok();
            }
        }
        if kept && !self.name.is_empty() {
            let _ = checks.send(self.name.clone());
        }
    }
}

/// The live side: each source's sound into the streaming model, its words out as drafts as they are said, each line, when it ends, on
/// to be kept; and the session's sound written as it comes. Sound that came too late for the captions is kept, not written live.
fn live(rx: Receiver<Chunk>, lines: Sender<Line>, session: Arc<Mutex<Option<Session>>>, who: Arc<Mutex<[(String, String); 2]>>, checks: Sender<String>) {
    let Ok(first) = rx.recv() else { return };
    let mut waiting = Some(first);
    let dir = data_dir().join("models").join("stream");
    for (remote, local) in STREAM_FILES {
        if let Err(e) = fetch(&format!("{STREAM_HF}{remote}"), &dir.join(local), "Live model") {
            status("error", json!({ "text": e }));
            return;
        }
    }
    let Some(rec) = Recognizer::new(&dir) else {
        status("error", json!({ "text": "The live model could not be loaded." }));
        return;
    };
    status("listening", json!({}));
    let streams = [rec.stream(), rec.stream()];
    let mut bufs: [Vec<f32>; 2] = Default::default();
    let mut starts = [0u64; 2];
    let mut said: [String; 2] = Default::default();
    let mut ids = [0u64; 2];
    let mut drafted = [false; 2];
    let mut last = [Instant::now(); 2];
    let mut recorder = Recorder::none();
    loop {
        let next = match waiting.take() {
            Some(c) => Ok(c),
            None => rx.recv_timeout(Duration::from_millis(200)),
        };
        // (the session: a new one, its sound kept from here; none, the last one closed)
        let now = session.lock().unwrap().clone();
        match &now {
            Some(s) if s.name != recorder.name => {
                recorder.close(&checks);
                let _ = std::fs::create_dir_all(sessions_dir());
                recorder = Recorder::open(s);
                for k in 0..2 {
                    rec.reset(streams[k]);
                    bufs[k].clear();
                    said[k].clear();
                    starts[k] = 0;
                }
            }
            None if !recorder.name.is_empty() => {
                recorder.close(&checks);
                recorder = Recorder::none();
            }
            _ => {}
        }
        let Ok(c) = next else { continue };
        let mut pending = vec![c];
        while let Ok(c) = rx.try_recv() {
            pending.push(c);
        }
        let mut touched = [false; 2];
        for (src, samples, at) in pending {
            let k = if src == Source::Mic { 0 } else { 1 };
            recorder.write(k, &samples);
            if at.elapsed() > STALE {
                // (too late for the captions: what was being heard is let go, and the live words start again from now)
                if !bufs[k].is_empty() || drafted[k] {
                    rec.reset(streams[k]);
                    bufs[k].clear();
                    said[k].clear();
                    if drafted[k] {
                        say(json!({ "kind": "drop", "id": ids[k], "source": source_name(src) }));
                        drafted[k] = false;
                        ids[k] += 1;
                    }
                }
                starts[k] = recorder.count[k];
                continue;
            }
            if bufs[k].is_empty() {
                starts[k] = recorder.count[k] - samples.len() as u64;
            }
            rec.hear(streams[k], &samples);
            bufs[k].extend_from_slice(&samples);
            if bufs[k].len() > LINE_MAX {
                let over = bufs[k].len() - LINE_MAX;
                bufs[k].drain(..over);
                starts[k] += over as u64;
            }
            touched[k] = true;
        }
        for (k, src) in [(0, Source::Mic), (1, Source::System)] {
            if !touched[k] {
                continue;
            }
            let text = rec.text(streams[k]);
            if rec.ended(streams[k]) {
                let id = ids[k];
                if text.trim().is_empty() {
                    if drafted[k] {
                        say(json!({ "kind": "drop", "id": id, "source": source_name(src) }));
                    }
                } else {
                    let start = starts[k] as f64 / RATE as f64;
                    let end = start + bufs[k].len() as f64 / RATE as f64;
                    let _ = lines.send(Line { src, id, samples: std::mem::take(&mut bufs[k]), text: pretty(&text), start, end });
                }
                rec.reset(streams[k]);
                bufs[k].clear();
                said[k].clear();
                drafted[k] = false;
                ids[k] += 1;
                continue;
            }
            if text != said[k] && !text.trim().is_empty() && last[k].elapsed() >= DRAFT_GAP {
                said[k] = text.clone();
                last[k] = Instant::now();
                drafted[k] = true;
                let (speaker, name) = who.lock().unwrap()[k].clone();
                say(json!({ "kind": "draft", "id": ids[k], "text": pretty(&text), "speaker": speaker, "name": name, "source": source_name(src) }));
            }
            // (quiet, nothing said: nothing kept of it)
            if text.trim().is_empty() && bufs[k].len() > (RATE as usize) * 3 {
                let over = bufs[k].len() - RATE as usize;
                bufs[k].drain(..over);
                starts[k] += over as u64;
            }
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------- keeping lines

/// The keeping side: each line that ended, whose it was, kept (Markdown, and JSON lines for the careful pass), and said.
fn keep(rx: Receiver<Line>, asks: Receiver<String>, session: Arc<Mutex<Option<Session>>>, who: Arc<Mutex<[(String, String); 2]>>, voices: Arc<Mutex<Voices>>) {
    let voice_path = data_dir().join("models").join("voice.onnx");
    let mut voice_model = None;
    let mut readied = false;
    let mut n = 0u64;
    let mut session_name = String::new();
    let mut told: std::collections::HashSet<String> = voices.lock().unwrap().list.iter().map(|v| v.id.clone()).collect();
    loop {
        while let Ok(_a) = asks.try_recv() {}
        let Ok(line) = rx.recv_timeout(Duration::from_millis(300)) else { continue };
        if !readied {
            readied = true;
            if fetch(VOICE_URL, &voice_path, "Voice model").is_ok() {
                voice_model = EmbeddingExtractor::new(ExtractorConfig { model: voice_path.to_string_lossy().to_string(), num_threads: Some(1), ..Default::default() }).ok();
            }
            status("listening", json!({}));
        }
        let k = if line.src == Source::Mic { 0 } else { 1 };
        // (whose voice, from either source: a microphone may be one person, or a room of them)
        let (speaker, name) = {
            let secs = line.samples.len() as f32 / RATE as f32;
            let e = (secs >= VOICE_MIN).then(|| voice_model.as_mut().and_then(|m| m.compute_speaker_embedding(line.samples.clone(), RATE).ok())).flatten();
            let mut v = voices.lock().unwrap();
            match e {
                Some(e) if secs >= NEW_VOICE_MIN => v.who(&e),
                Some(e) => v.known(&e).unwrap_or_else(|| who.lock().unwrap()[k].clone()),
                None => who.lock().unwrap()[k].clone(),
            }
        };
        who.lock().unwrap()[k] = (speaker.clone(), name.clone());
        if !told.contains(&speaker) {
            told.insert(speaker.clone());
            say(json!({ "kind": "voices", "list": voices.lock().unwrap().info() }));
        }
        let at = chrono::Local::now();
        let sess = session.lock().unwrap().clone();
        let sname = sess.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        if sname != session_name {
            session_name = sname.clone();
            n = 0;
        }
        n += 1;
        let uid = format!("{sname}#{n}");
        let time = at.format("%-I:%M %p").to_string();
        let date = at.format("%A, %-d %B %Y").to_string();
        let src = source_name(line.src);
        if let Some(s) = &sess {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&s.md) {
                let _ = writeln!(f, "**{name}** ({}): {}\n", at.format("%H:%M:%S"), line.text);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(sessions_dir().join(format!("{}.jsonl", s.name))) {
                let _ = writeln!(f, "{}", json!({ "uid": uid, "text": line.text, "speaker": speaker, "name": name, "at": at.timestamp_millis(), "clock": at.format("%H:%M:%S").to_string(), "time": time, "date": date, "source": src, "start": line.start, "end": line.end }));
            }
        }
        say(json!({ "kind": "line", "uid": uid, "id": line.id, "text": line.text, "speaker": speaker, "name": name, "at": at.timestamp_millis(), "time": time, "date": date, "source": src, "session": sess.map(|s| s.name), "start": line.start, "end": line.end }));
    }
}

// -------------------------------------------------------------------------------------------------------------- the careful pass

/// A session's sound, one source, 16 kHz mono.
fn read_wav(path: &Path) -> Option<Vec<f32>> {
    let mut r = hound::WavReader::open(path).ok()?;
    Some(r.samples::<i16>().filter_map(|s| s.ok()).map(|s| s as f32 / 32768.0).collect())
}

/// A session's sound made louder, plainly: one gain for the whole file, so its loudest moment (a stray click aside) sits just under the
/// top. Nothing is squeezed: quiet stays quiet next to loud, as it was, only all of it louder. Written back.
fn louder(path: &Path) {
    let Some(mut all) = read_wav(path) else { return };
    let mut mags: Vec<f32> = all.iter().map(|x| x.abs()).collect();
    if mags.is_empty() {
        return;
    }
    // (its peak, but for the loudest few samples in a hundred thousand: a click, not the speech)
    let at = ((mags.len() as f64 * 0.99999) as usize).min(mags.len() - 1);
    let (_, peak, _) = mags.select_nth_unstable_by(at, |a, b| a.total_cmp(b));
    let peak = *peak;
    if peak < 1e-4 {
        return; // (silence)
    }
    let gain = (0.89 / peak).min(8.0);
    if gain < 1.2 {
        return;
    }
    for x in all.iter_mut() {
        *x = (*x * gain).clamp(-0.99, 0.99);
    }
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let part = path.with_extension("part");
    let Ok(mut w) = hound::WavWriter::create(&part, spec) else { return };
    for x in &all {
        let _ = w.write_sample((x * 32767.0) as i16);
    }
    if w.finalize().is_ok() {
        let _ = std::fs::rename(&part, path);
    }
}

/// Whisper over a stretch of speech (up to about half a minute), with what was said before it: each word, and when it was said
/// (seconds into the stretch).
fn words_of(ctx: &WhisperContext, samples: &[f32], threads: i32, before: &str) -> Vec<(f64, f64, String)> {
    let Ok(mut state) = ctx.create_state() else { return Vec::new() };
    let mut p = FullParams::new(SamplingStrategy::BeamSearch { beam_size: 4, patience: 1.0 });
    p.set_language(Some("en"));
    p.set_n_threads(threads);
    p.set_suppress_blank(true);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_special(false);
    p.set_print_timestamps(false);
    p.set_token_timestamps(true);
    p.set_no_speech_thold(0.6);
    if !before.is_empty() {
        p.set_initial_prompt(before);
    }
    if state.full(p, samples).is_err() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for seg in 0..state.full_n_segments().unwrap_or(0) {
        let whole = state.full_get_segment_text_lossy(seg).unwrap_or_default();
        if is_noise(&whole) {
            continue;
        }
        for t in 0..state.full_n_tokens(seg).unwrap_or(0) {
            let text = state.full_get_token_text_lossy(seg, t).unwrap_or_default();
            if text.starts_with("[_") || text.starts_with("<|") || text.is_empty() {
                continue;
            }
            let Ok(d) = state.full_get_token_data(seg, t) else { continue };
            out.push((d.t0 as f64 / 100.0, (d.t0 + d.t1) as f64 / 200.0, text));
        }
    }
    out
}

/// Stretches of a source's lines to write down together: in order, each at most so long, a long quiet between lines starting another.
fn stretches(lines: &[Value], source: &str) -> Vec<Vec<usize>> {
    let mut idx: Vec<usize> = (0..lines.len()).filter(|&i| lines[i]["source"] == source && lines[i]["start"].is_number()).collect();
    idx.sort_by(|&a, &b| lines[a]["start"].as_f64().unwrap_or(0.0).total_cmp(&lines[b]["start"].as_f64().unwrap_or(0.0)));
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in idx {
        let (s, e) = (lines[i]["start"].as_f64().unwrap_or(0.0), lines[i]["end"].as_f64().unwrap_or(0.0));
        let join = out.last().is_some_and(|g| {
            let first = lines[g[0]]["start"].as_f64().unwrap_or(0.0);
            let last = lines[*g.last().unwrap()]["end"].as_f64().unwrap_or(0.0);
            e - first <= 28.0 && s - last <= 3.0
        });
        if join {
            out.last_mut().unwrap().push(i);
        } else {
            out.push(vec![i]);
        }
    }
    out
}

/// Go over a session again, carefully. Its sound made louder; then each stretch of speech written down by Whisper as a whole (with
/// what was said before it), its words placed in their lines by when they were said; the voices of what the computer played grouped
/// over the whole session (the same voice one speaker, however it was matched live). The better words and speakers are written back
/// (its JSON lines, and its Markdown, made again) and said.
fn check(name: &str, model: &str, voices: &Arc<Mutex<Voices>>) {
    let dir = sessions_dir();
    for src in ["mic", "system"] {
        louder(&dir.join(format!("{name} {src}.wav")));
    }
    let jl = dir.join(format!("{name}.jsonl"));
    let Ok(text) = std::fs::read_to_string(&jl) else { return };
    let mut lines: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    if lines.is_empty() {
        return;
    }
    let sound = [read_wav(&dir.join(format!("{name} mic.wav"))), read_wav(&dir.join(format!("{name} system.wav")))];
    if sound.iter().all(|s| s.is_none()) {
        return;
    }
    status("checking", json!({ "session": name }));
    let piece = |src: usize, a: f64, b: f64| -> Option<Vec<f32>> {
        let s = sound[src].as_ref()?;
        let a = (a.max(0.0) * RATE as f64) as usize;
        let b = ((b * RATE as f64) as usize).min(s.len());
        (b > a + RATE as usize / 4).then(|| s[a..b].to_vec())
    };
    let models = data_dir().join("models");
    let file = models.join(format!("ggml-{model}.bin"));
    // the words
    if fetch(&format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin"), &file, "Speech model").is_ok() {
        if let Ok(ctx) = WhisperContext::new_with_params(&file.to_string_lossy(), WhisperContextParameters::default()) {
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 4) as i32;
            for (k, src) in ["mic", "system"].into_iter().enumerate() {
                let mut before = String::new();
                for g in stretches(&lines, src) {
                    let from = (lines[g[0]]["start"].as_f64().unwrap_or(0.0) - 0.3).max(0.0);
                    let to = lines[*g.last().unwrap()]["end"].as_f64().unwrap_or(0.0) + 0.4;
                    let Some(samples) = piece(k, from, to) else { continue };
                    let words = words_of(&ctx, &samples, threads, &before);
                    if words.is_empty() {
                        continue;
                    }
                    // (each word to the line it was said in, else the nearest)
                    let mut got: Vec<Vec<(f64, String)>> = vec![Vec::new(); g.len()];
                    for (t0, t, w) in &words {
                        let at = from + t;
                        let j = (0..g.len())
                            .min_by(|&x, &y| {
                                let d = |i: usize| {
                                    let (s, e) = (lines[g[i]]["start"].as_f64().unwrap_or(0.0), lines[g[i]]["end"].as_f64().unwrap_or(0.0));
                                    if at < s { s - at } else if at > e { at - e } else { 0.0 }
                                };
                                d(x).total_cmp(&d(y))
                            })
                            .unwrap_or(0);
                        got[j].push((from + t0, w.clone()));
                    }
                    for (j, i) in g.iter().enumerate() {
                        let raw: String = got[j].iter().map(|w| w.1.as_str()).collect();
                        let t = tidy(&raw);
                        if t.is_empty() || is_noise(&t) {
                            continue;
                        }
                        lines[*i]["text"] = json!(t);
                        // (when each word began, from the line's start, and where it ends in its words: to light them as it plays)
                        let mut starts: Vec<f64> = Vec::new();
                        for (k, (at, w)) in got[j].iter().enumerate() {
                            if k == 0 || w.starts_with(' ') {
                                starts.push(*at);
                            }
                        }
                        let ws: Vec<&str> = t.split_whitespace().collect();
                        if ws.len() == starts.len() {
                            let line_start = lines[*i]["start"].as_f64().unwrap_or(0.0);
                            let mut end = 0usize;
                            let marks: Vec<Value> = ws
                                .iter()
                                .zip(&starts)
                                .enumerate()
                                .map(|(k, (w, at))| {
                                    end += w.encode_utf16().count() + usize::from(k > 0);
                                    json!([((at - line_start).max(0.0) * 100.0).round() / 100.0, end])
                                })
                                .collect();
                            lines[*i]["words"] = json!(marks);
                        } else {
                            lines[*i]["words"] = Value::Null;
                        }
                    }
                    let all: String = words.iter().map(|w| w.2.as_str()).collect();
                    before = all.chars().rev().take(220).collect::<Vec<_>>().into_iter().rev().collect();
                }
            }
        }
    }
    // the voices: grouped over the session (both sources: a microphone may hear a room), each group the voice it is most like (or a new one)
    let voice_path = models.join("voice.onnx");
    let mut used = std::collections::HashSet::new();
    if let Ok(mut m) = EmbeddingExtractor::new(ExtractorConfig { model: voice_path.to_string_lossy().to_string(), num_threads: Some(1), ..Default::default() }) {
        // (each line long enough: what its voice is like, and how long it is)
        let mut groups: Vec<(Vec<f32>, f32, Vec<usize>)> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            let k = if l["source"] == "mic" { 0 } else { 1 };
            let (s, e) = (l["start"].as_f64().unwrap_or(0.0), l["end"].as_f64().unwrap_or(0.0));
            let secs = (e - s) as f32;
            if secs < 1.0 {
                continue;
            }
            let Some(samples) = piece(k, s, e) else { continue };
            if let Ok(v) = m.compute_speaker_embedding(samples, RATE) {
                groups.push((v, secs, vec![i]));
            }
        }
        // (the two most alike, together, while any two are alike enough)
        while groups.len() > 1 {
            let mut best = (0, 0, -1.0f32);
            for a in 0..groups.len() {
                for b in a + 1..groups.len() {
                    let c = cosine(&groups[a].0, &groups[b].0);
                    if c > best.2 {
                        best = (a, b, c);
                    }
                }
            }
            if best.2 < SAME_VOICE {
                break;
            }
            let (a, b) = (best.0, best.1);
            let g = groups.remove(b);
            let (wa, wb) = (groups[a].1, g.1);
            for (x, y) in groups[a].0.iter_mut().zip(&g.0) {
                *x = (*x * wa + y * wb) / (wa + wb);
            }
            groups[a].1 += wb;
            groups[a].2.extend(g.2);
        }
        // (a group of little speech that is like no voice: it goes with the group it is most like)
        groups.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut v = voices.lock().unwrap();
        let mut named: Vec<(Vec<f32>, (String, String))> = Vec::new();
        for (centre, secs, members) in groups {
            let label = match v.known(&centre) {
                Some(l) => l,
                None if secs >= 4.0 || named.is_empty() => v.who(&centre),
                None => named.iter().max_by(|a, b| cosine(&a.0, &centre).total_cmp(&cosine(&b.0, &centre))).map(|n| n.1.clone()).unwrap(),
            };
            for i in members {
                lines[i]["speaker"] = json!(label.0);
                lines[i]["name"] = json!(label.1);
            }
            named.push((centre, label));
        }
        // (a line too short to tell: the speaker of the line before it, from the same source)
        for src in ["mic", "system"] {
            let mut last: Option<(Value, Value)> = None;
            for l in lines.iter_mut().filter(|l| l["source"] == src) {
                let short = l["end"].as_f64().unwrap_or(0.0) - l["start"].as_f64().unwrap_or(0.0) < 1.0;
                match (&last, short) {
                    (Some((s, n)), true) => {
                        l["speaker"] = s.clone();
                        l["name"] = n.clone();
                    }
                    _ => last = Some((l["speaker"].clone(), l["name"].clone())),
                }
            }
        }
        for l in &lines {
            if let Some(s) = l["speaker"].as_str() {
                used.insert(s.to_string());
            }
        }
        v.forget_strays(&used);
        say(json!({ "kind": "voices", "list": v.info() }));
    }
    // written back, and said
    let _ = std::fs::write(&jl, lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
    write_md(name, &lines);
    let revised: Vec<Value> = lines.iter().map(|l| json!({ "uid": l["uid"], "text": l["text"], "speaker": l["speaker"], "name": l["name"], "words": l["words"] })).collect();
    say(json!({ "kind": "revised", "session": name, "lines": revised }));
}

/// The careful pass, at low priority: each session that ends (its sound whole), gone over again.
fn checker(rx: Receiver<String>, model: Arc<Mutex<String>>, voices: Arc<Mutex<Voices>>, listening: Arc<Mutex<bool>>) {
    low_priority();
    for name in rx {
        let m = model.lock().unwrap().clone();
        check(&name, &m, &voices);
        status(if *listening.lock().unwrap() { "listening" } else { "idle" }, json!({ "checked": name }));
    }
}

/// Sessions' sound older than so many days, deleted (their words are kept).
fn forget_sound(days: i64) {
    if days <= 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(sessions_dir()) else { return };
    let cut = std::time::SystemTime::now() - Duration::from_secs(days as u64 * 86_400);
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "wav") && e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < cut) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Play a piece of a session's sound (`start`..`end` seconds of one source) on the speakers.
fn play(session: &str, source: &str, start: f64, end: f64) {
    let path = sessions_dir().join(format!("{session} {source}.wav"));
    let Some(all) = read_wav(&path) else { return };
    let a = ((start - 0.1).max(0.0) * RATE as f64) as usize;
    let b = (((end + 0.2) * RATE as f64) as usize).min(all.len());
    if b <= a {
        return;
    }
    let clip = all[a..b].to_vec();
    std::thread::spawn(move || {
        let host = cpal::default_host();
        let Some(d) = host.default_output_device() else { return };
        let Ok(config) = d.default_output_config() else { return };
        let channels = config.channels() as usize;
        let step = RATE as f64 / config.sample_rate().0 as f64;
        let pos = Arc::new(Mutex::new(0.0f64));
        let p2 = pos.clone();
        let clip2 = clip.clone();
        let cfg: cpal::StreamConfig = config.clone().into();
        let stream = d.build_output_stream(
            &cfg,
            move |out: &mut [f32], _: &_| {
                let mut p = p2.lock().unwrap();
                for frame in out.chunks_mut(channels) {
                    let i = *p as usize;
                    let v = if i + 1 < clip2.len() { clip2[i] } else { 0.0 };
                    for s in frame.iter_mut() {
                        *s = v;
                    }
                    *p += step;
                }
            },
            |e| eprintln!("play: {e}"),
            None,
        );
        let Ok(stream) = stream else { return };
        if stream.play().is_err() {
            return;
        }
        std::thread::sleep(Duration::from_secs_f64(clip.len() as f64 / RATE as f64 + 0.2));
    });
}

/// What it hears: the microphone, what the computer plays, or both.
fn open_streams(host: &cpal::Host, mic: bool, system: bool, tx: &Sender<Chunk>) -> Vec<cpal::Stream> {
    let mut streams = Vec::new();
    if mic {
        if let Some(d) = host.default_input_device() {
            if let Some(s) = d.default_input_config().ok().and_then(|c| hear(&d, c, Source::Mic, tx.clone())) {
                streams.push(s);
            }
        }
    }
    if system {
        if let Some(d) = host.default_output_device() {
            if let Some(s) = d.default_output_config().ok().and_then(|c| hear(&d, c, Source::System, tx.clone())) {
                streams.push(s);
            }
        }
    }
    streams
}

fn main() {
    let (tx, rx) = channel::<Chunk>();
    let (ask_tx, ask_rx) = channel::<String>();
    let (line_tx, line_rx) = channel::<Line>();
    let (check_tx, check_rx) = channel::<String>();
    let model: Arc<Mutex<String>> = Arc::new(Mutex::new("small".into()));
    // (sessions from before, never gone over: their sound kept, nothing checked yet; one command does it)
    let recheck = check_tx.clone();
    let session: Arc<Mutex<Option<Session>>> = Arc::default();
    let listening: Arc<Mutex<bool>> = Arc::default();
    let voices = Arc::new(Mutex::new(Voices::load(data_dir().join("voices.json"))));
    let who: Arc<Mutex<[(String, String); 2]>> = Arc::new(Mutex::new([("s0".into(), "\u{1F464}".into()), ("s0".into(), "\u{1F464}".into())]));
    {
        let (session, who) = (session.clone(), who.clone());
        std::thread::spawn(move || live(rx, line_tx, session, who, check_tx));
    }
    {
        let (session, who, voices) = (session.clone(), who.clone(), voices.clone());
        std::thread::spawn(move || keep(line_rx, ask_rx, session, who, voices));
    }
    let who = who.clone();
    {
    }
    {
        let (voices, listening, model) = (voices.clone(), listening.clone(), model.clone());
        std::thread::spawn(move || checker(check_rx, model, voices, listening));
    }
    status("idle", json!({}));
    say(json!({ "kind": "voices", "list": voices.lock().unwrap().info() }));
    let host = cpal::default_host();
    let mut streams: Vec<cpal::Stream> = Vec::new();
    // (a session ends: the live side closes its sound, then hands it to the careful pass)
    let end_session = || {
        session.lock().unwrap().take();
    };
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
        match m["cmd"].as_str().unwrap_or("") {
            "listen" => {
                streams.clear();
                end_session();
                if let Some(x) = m["model"].as_str().filter(|s| ["tiny", "base", "small", "medium"].contains(s)) {
                    *model.lock().unwrap() = x.to_string();
                }
                let keep_days = m["keep_days"].as_i64().unwrap_or(7);
                forget_sound(keep_days);
                streams = open_streams(&host, m["mic"] != false, m["system"] != false, &tx);
                if streams.is_empty() {
                    status("error", json!({ "text": "Nothing to hear: no microphone, and no sound device." }));
                    continue;
                }
                let now = chrono::Local::now();
                let name = now.format("%Y-%m-%d %H%M%S").to_string();
                let _ = std::fs::create_dir_all(sessions_dir());
                let md = sessions_dir().join(format!("{name}.md"));
                let _ = std::fs::write(&md, format!("# {}\n\n", now.format("%A, %-d %B %Y, %-I:%M %p")));
                *session.lock().unwrap() = Some(Session { name: name.clone(), md, record: keep_days >= 0 });
                *listening.lock().unwrap() = true;
                status("listening", json!({ "session": name }));
            }
            "stop" => {
                streams.clear();
                end_session();
                *listening.lock().unwrap() = false;
                status("idle", json!({}));
            }
            // (while it listens: hear from these now, in the same session)
            "sources" => {
                if *listening.lock().unwrap() {
                    streams.clear();
                    streams = open_streams(&host, m["mic"] != false, m["system"] != false, &tx);
                }
            }
            "name" | "animal" | "me" => {
                let Some(id) = m["speaker"].as_str() else { continue };
                {
                    let mut v = voices.lock().unwrap();
                    match m["cmd"].as_str().unwrap_or("") {
                        "name" => v.name(id, m["name"].as_str().unwrap_or("")),
                        "animal" => v.set_animal(id, m["animal"].as_str().unwrap_or("")),
                        _ => v.set_me(id, m["on"] != false),
                    }
                    say(json!({ "kind": "voices", "list": v.info() }));
                }
                let label = voices.lock().unwrap().label_of(id);
                if let Some(label) = label {
                    // (the live side's label for it, and every session's lines)
                    for w in who.lock().unwrap().iter_mut().filter(|w| w.0 == id) {
                        *w = label.clone();
                    }
                    let id = id.to_string();
                    std::thread::spawn(move || relabel(&id, &label, None));
                }
                let _ = ask_tx.send(id.to_string());
            }
            // (two voices that are one person)
            "merge" => {
                let (Some(into), Some(from)) = (m["into"].as_str(), m["from"].as_str()) else { continue };
                let merged = voices.lock().unwrap().merge(into, from);
                if !merged {
                    continue;
                }
                say(json!({ "kind": "voices", "list": voices.lock().unwrap().info() }));
                if let Some(label) = voices.lock().unwrap().label_of(into) {
                    for w in who.lock().unwrap().iter_mut().filter(|w| w.0 == from || w.0 == into) {
                        *w = label.clone();
                    }
                    let (from, into) = (from.to_string(), into.to_string());
                    std::thread::spawn(move || {
                        relabel(&from, &label, None);
                        relabel(&into, &label, None);
                    });
                }
            }
            "sessions" => say(json!({ "kind": "sessions", "list": sessions_list() })),
            "open" => {
                let Some(name) = m["session"].as_str().filter(|n| !n.contains(['/', '\\', '.'])) else { continue };
                say(json!({ "kind": "session", "name": name, "lines": session_lines(name) }));
            }
            "check" => {
                if let Some(s) = m["session"].as_str() {
                    let _ = recheck.send(s.to_string());
                }
            }
            "play" => {
                if let (Some(s), Some(src), Some(a), Some(b)) = (m["session"].as_str(), m["source"].as_str(), m["start"].as_f64(), m["end"].as_f64()) {
                    play(s, src, a, b);
                }
            }
            "open_sessions" => {
                let dir = sessions_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            }
            "quit" => break,
            _ => {}
        }
    }
    // (stopped: the session closed, so its sound is whole)
    streams.clear();
    *session.lock().unwrap() = None;
    std::thread::sleep(Duration::from_millis(300));
}
