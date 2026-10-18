//! Claude usage: how much of the Claude 5-hour and 7-day limits is used. Two thin bars in the head (the numbers and when the
//! window opens again are in their tooltip), a ring for each in the tray, and a peek on the island when a number moves.
//!
//! It signs in to Claude on its own, the way the Claude Code CLI does (OAuth with PKCE: the browser opens, the page shows a code,
//! the code is pasted back). Its tokens are its own: kept in its `storage` (sealed for this Windows user) and renewed by the
//! module itself, so the numbers come whether or not Claude Code runs. Claude Code's own login (`~/.claude/.credentials.json`) is
//! never read, written or renewed: the CLI renews it only while it runs, and a refresh token is good for one use, so sharing it
//! would break the CLI's login.
//!
//! It reaches two hosts (`permissions.net`) through the island, and for that it sees nothing of this PC: a module that can reach
//! the network has nothing to carry out.

use nadi_plugin::*;
use sha2::{Digest, Sha256};
use std::cell::RefCell;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const REDIRECT_URL: &str = "https://console.anthropic.com/oauth/code/callback";
const SCOPE: &str = "org:create_api_key user:profile user:inference";
/// The public OAuth client of the Claude Code CLI.
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const BETA: &str = "oauth-2025-04-20";
/// Cloudflare in front of both hosts blocks default HTTP-library names; a small honestly-named tool gets through.
const USER_AGENT: &str = "claude-code/2.1.0";
/// The token endpoint rate-limits that name for everyone (a plain 429 on the first try), so the sign-in and its renewal say who
/// they are.
const OAUTH_AGENT: &str = "nadi/1.1";
/// asking every 5 s got the endpoint rate-limited: back off to 2 minutes
const POLL_MS: u64 = 120_000;
/// ... and sooner while there is nothing to show (a sign-in brings it)
const POLL_EMPTY_MS: u64 = 20_000;
const MANUAL_COOLDOWN_MS: u64 = 60_000;
/// a token is renewed this long before it runs out
const RENEW_BEFORE_MS: u64 = 60_000;
/// the peek: the bars sit at the old numbers for a moment, slide to the new ones, and rest
const PEEK_BEFORE_MS: u64 = 500;
const PEEK_AFTER_MS: u64 = 1_400 + 2_500;

const FIVE: &str = "#D97757";
const SEVEN: &str = "#E8D5B5";
const WARM: &str = "#E6A15A";
const FULL: &str = "#E5675C";
const RING: &str = "#E8963C";

#[derive(Clone, Debug, PartialEq)]
struct Tokens {
    access: String,
    refresh: String,
    expires_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Window {
    pct: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Snapshot {
    /// there is a sign-in (and so something to show, or a reason there is not)
    available: bool,
    error: Option<String>,
    needs_login: bool,
    five: Option<Window>,
    seven: Option<Window>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Peek {
    /// the bars sit at the old numbers; the next wake moves them
    Before,
    /// they have moved; the next wake takes the pill back
    After,
}

#[derive(Default)]
struct State {
    tokens: Option<Tokens>,
    snap: Snapshot,
    /// a sign-in that was begun: (the code verifier, the state)
    pending: Option<(String, String)>,
    last_poll: Option<u64>,
    last_manual: Option<u64>,
    /// a request is out (its id): `usage`, `renew` or `exchange`
    inflight: Option<&'static str>,
    /// the usage request was refused once and the token renewed: it is not renewed again for it
    renewed: bool,
    /// the numbers last seen, to tell when one moved
    last_pct: (Option<f64>, Option<f64>),
    peek: Option<Peek>,
    peek_new: (Option<f64>, Option<f64>),
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// What a call asks of the island besides drawing: collected while the state moves, put on the reply at the end.
#[derive(Default)]
struct Act {
    fetch: Option<Req>,
    store: Option<Value>,
    pill: Option<Option<Pill>>,
    wake: Option<u64>,
    open: Option<String>,
    note: Option<String>,
}

struct Req {
    id: &'static str,
    method: &'static str,
    url: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Option<String>,
}

// ------------------------------------------------------------------------------------------------------------ small tools

fn b64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        if c.len() > 1 {
            out.push(A[(n >> 6) as usize & 63] as char);
        }
        if c.len() > 2 {
            out.push(A[n as usize & 63] as char);
        }
    }
    out
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()).collect()
}

fn challenge_of(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn authorize_url(challenge: &str, state: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        encode(REDIRECT_URL),
        encode(SCOPE),
    )
}

/// What the page shows is `code#state`: only the code goes to Anthropic (with the state, which is its own).
fn code_of(pasted: &str) -> &str {
    let t = pasted.trim();
    t.split('#').next().unwrap_or(t).trim()
}

/// `2026-10-16T14:05:00.123456+00:00` (or `Z`) as ms since 1970
fn parse_iso(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let split = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(split);
    let mut c = clock.split(':');
    let (h, mi): (i64, i64) = (c.next()?.parse().ok()?, c.next()?.parse().ok()?);
    let sec: f64 = c.next().unwrap_or("0").parse().ok()?;
    let offset_min = match zone.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let z = &zone[1..];
            let (zh, zm) = z.split_once(':').unwrap_or((z, "0"));
            let v = zh.parse::<i64>().ok()? * 60 + zm.parse::<i64>().ok()?;
            if sign == '+' {
                v
            } else {
                -v
            }
        }
        _ => 0,
    };
    // days since 1970 (the proleptic Gregorian calendar)
    let (yy, mm) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + h * 3600 + mi * 60 - offset_min * 60) as f64 * 1000.0 + sec * 1000.0) as i64)
}

/// "12m", "3h 2m", "2d 4h": how long until the window opens again
fn reset_in(resets_at: &Option<String>, now: u64) -> String {
    let Some(at) = resets_at.as_deref().and_then(parse_iso) else { return String::new() };
    let mins = ((at - now as i64) / 60_000).max(0);
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h {}m", mins % 60);
    }
    format!("{}d {}h", hours / 24, hours % 24)
}

fn bar_color(pct: f64, own: &'static str) -> &'static str {
    if pct >= 100.0 {
        FULL
    } else if pct >= 80.0 {
        WARM
    } else {
        own
    }
}

// ----------------------------------------------------------------------------------------------------------- what it asks

fn token_request(act: &mut Act, id: &'static str, body: Value) {
    act.fetch = Some(Req {
        id,
        method: "POST",
        url: TOKEN_URL,
        headers: vec![("Content-Type", "application/json".into()), ("Accept", "application/json".into()), ("User-Agent", OAUTH_AGENT.into())],
        body: Some(body.to_string()),
    });
}

fn usage_request(act: &mut Act, access: &str) {
    act.fetch = Some(Req {
        id: "usage",
        method: "GET",
        url: USAGE_URL,
        headers: vec![("Authorization", format!("Bearer {access}")), ("anthropic-beta", BETA.into()), ("Accept", "application/json".into()), ("User-Agent", USER_AGENT.into())],
        body: None,
    });
}

fn tokens_of(data: &Value, keep_refresh: &str, now: u64) -> Option<Tokens> {
    let access = data["access_token"].as_str()?.to_string();
    let expires_in = data["expires_in"].as_u64().unwrap_or(0);
    Some(Tokens { access, refresh: data["refresh_token"].as_str().unwrap_or(keep_refresh).to_string(), expires_ms: if expires_in > 0 { now + expires_in * 1000 } else { 0 } })
}

fn keep(t: &Option<Tokens>) -> Value {
    match t {
        Some(t) => json!({ "login": { "access": t.access, "refresh": t.refresh, "expires_ms": t.expires_ms } }),
        None => json!({}),
    }
}

fn window_of(usage: &Value, key: &str) -> Option<Window> {
    let w = usage.get(key)?;
    w.is_object().then(|| Window { pct: w["utilization"].as_f64(), resets_at: w["resets_at"].as_str().map(str::to_string) })
}

/// the numbers from what the endpoint answered (flat, or under `usage` or `data`)
fn snapshot_of(data: &Value) -> Snapshot {
    let usage = if data.get("five_hour").is_none() && data.get("seven_day").is_none() { data.get("usage").or_else(|| data.get("data")).unwrap_or(data) } else { data };
    Snapshot { available: true, five: window_of(usage, "five_hour"), seven: window_of(usage, "seven_day"), ..Snapshot::default() }
}

/// the last numbers stay, with why they did not move
fn failed(st: &mut State, msg: impl Into<String>, needs_login: bool) {
    st.snap = Snapshot { available: true, error: Some(msg.into()), needs_login, five: st.snap.five.clone(), seven: st.snap.seven.clone() };
}

// ------------------------------------------------------------------------------------------------------------- what it shows

fn render(st: &State, now: u64, wanted: bool, act: Act) -> Reply {
    let s = &st.snap;
    let mut bars: Vec<(&str, f32, &str, String, String)> = Vec::new();
    let mut rings: Vec<Ring> = Vec::new();
    if wanted && s.available {
        for (label, own, id, w) in [("5H", FIVE, "five_hour", &s.five), ("7D", SEVEN, "seven_day", &s.seven)] {
            let Some(pct) = w.as_ref().and_then(|w| w.pct) else { continue };
            let back = reset_in(&w.as_ref().and_then(|w| w.resets_at.clone()), now);
            let under = if back.is_empty() { String::new() } else { format!("Back in {back}") };
            bars.push((label, (pct / 100.0).clamp(0.0, 1.0) as f32, bar_color(pct, own), format!("{}%", pct.round() as i64), under));
            let status = s.error.as_deref().map(|e| format!("Not up to date: {e}")).unwrap_or_else(|| "Click to refresh".into());
            let resets = if back.is_empty() { String::new() } else { format!("Resets in {back} · ") };
            rings.push(Ring::new(id, pct as f32, format!("Claude {} · {}%\n{resets}{status}", label.to_lowercase(), pct.round() as i64), RING).in_tray());
        }
    }
    let plain: Vec<(&str, f32, &str, &str)> = bars.iter().map(|(l, p, c, _, _)| (*l, *p, *c, "")).collect();
    let in_tip: Vec<(&str, f32, &str, &str, &str)> = bars.iter().map(|(l, p, c, w, u)| (*l, *p, *c, w.as_str(), u.as_str())).collect();
    let foot = s.error.as_deref().map(|e| format!("Not up to date: {e}")).unwrap_or_else(|| "Click to refresh".into());
    // (the bars take no room until there is something to show: no sign-in, no bars)
    let mut reply = if plain.is_empty() {
        Reply::widget("bars", Blocks::new(), Blocks::new()).with_widget("rings", Blocks::new(), Blocks::new())
    } else {
        // (the same two numbers twice: as thin bars, and as circles)
        Reply::widget("bars", Blocks::new().bars(&plain), Blocks::new().bars_with_sub(&in_tip).text("muted", foot.clone()))
            .with_widget("rings", Blocks::new().rings(&plain), Blocks::new().bars_with_sub(&in_tip).text("muted", foot))
    };
    reply = reply.rings(rings);
    // the line under the plugin's name
    let note = act.note.or_else(|| {
        if !wanted {
            None
        } else if let Some(e) = &s.error {
            Some(format!("Usage rings: {e}"))
        } else if s.needs_login {
            Some("Usage rings: sign in below to see how much of your limits is used.".to_string())
        } else {
            None
        }
    });
    reply = match note {
        Some(n) => reply.note(n),
        None => reply.no_note(),
    };
    if let Some(r) = act.fetch {
        let headers: Vec<(&str, &str)> = r.headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        reply = reply.fetch(r.id, r.method, r.url, &headers, r.body.as_deref());
    }
    if let Some(v) = act.store {
        reply = reply.store(v);
    }
    match act.pill {
        Some(Some(p)) => reply = reply.pill(p),
        Some(None) => reply = reply.no_pill(),
        None => {}
    }
    if let Some(ms) = act.wake {
        reply = reply.wake_in(ms);
    }
    if let Some(u) = act.open {
        reply = reply.open(u);
    }
    reply
}

fn peek_pill(v: (Option<f64>, Option<f64>)) -> Pill {
    let row = |label: &str, pct: Option<f64>| PillRow { label: label.into(), value: pct.map_or(0.0, |p| (p / 100.0).clamp(0.0, 1.0)) as f32, text: pct.map(|p| format!("{}%", p.round() as i64)).unwrap_or_default() };
    Pill::new().icon("sparkle").rank(100).size(240.0, 78.0).briefly().rows(vec![row("5h", v.0), row("7d", v.1)])
}

/// When a number moved (rounded: a jitter under a percent is not news), the island drops down a peek: the bars sit at the old
/// numbers, slide to the new ones, and rest. Not over the hub, which shows them already; the new baseline is kept either way.
fn on_numbers(st: &mut State, hub: bool, act: &mut Act) {
    if st.snap.error.is_some() || !st.snap.available {
        return;
    }
    let new = (st.snap.five.as_ref().and_then(|w| w.pct), st.snap.seven.as_ref().and_then(|w| w.pct));
    let old = std::mem::replace(&mut st.last_pct, new);
    let moved = |o: Option<f64>, n: Option<f64>| matches!((o, n), (Some(o), Some(n)) if o.round() != n.round());
    if !(moved(old.0, new.0) || moved(old.1, new.1)) || hub {
        return;
    }
    st.peek = Some(Peek::Before);
    st.peek_new = new;
    act.pill = Some(Some(peek_pill(old)));
    act.wake = Some(PEEK_BEFORE_MS);
}

// ------------------------------------------------------------------------------------------------------------ what it does

/// Ask for the numbers: renew the token first when it is about to run out.
fn refresh(st: &mut State, now: u64, act: &mut Act) {
    st.last_poll = Some(now);
    let Some(t) = st.tokens.clone() else {
        st.snap = Snapshot { needs_login: true, ..Snapshot::default() };
        return;
    };
    st.renewed = false;
    if t.expires_ms != 0 && t.expires_ms < now + RENEW_BEFORE_MS {
        renew(st, &t, act);
    } else {
        st.inflight = Some("usage");
        usage_request(act, &t.access);
    }
}

fn renew(st: &mut State, t: &Tokens, act: &mut Act) {
    if t.refresh.is_empty() {
        failed(st, "The sign-in has no way to renew itself. Sign in again.", true);
        return;
    }
    st.inflight = Some("renew");
    token_request(act, "renew", json!({ "grant_type": "refresh_token", "refresh_token": t.refresh, "client_id": CLIENT_ID }));
}

/// What a request came back with.
fn answered(st: &mut State, ev: &Event, act: &mut Act) {
    let data: Value = serde_json::from_str(&ev.body).unwrap_or(Value::Null);
    let net = || if ev.error.is_empty() { format!("HTTP {}", ev.status) } else { ev.error.clone() };
    let waited = st.inflight.take();
    match (waited, ev.id.as_str()) {
        (Some("usage"), "usage") => match ev.status {
            200 => {
                st.snap = snapshot_of(&data);
                on_numbers(st, ev.hub, act);
            }
            401 if !st.renewed => {
                // (revoked, or run out before its time: renewed once, then asked again)
                st.renewed = true;
                if let Some(t) = st.tokens.clone() {
                    renew(st, &t, act);
                }
            }
            401 => failed(st, "The sign-in was refused. Sign in again.", true),
            429 => failed(st, "Rate limited by the usage endpoint -- try again in a bit.", false),
            _ => failed(st, net(), false),
        },
        (Some("renew"), "renew") => match ev.status {
            200 => {
                let old = st.tokens.clone().map(|t| t.refresh).unwrap_or_default();
                match tokens_of(&data, &old, ev.now) {
                    Some(fresh) => {
                        st.tokens = Some(fresh.clone());
                        st.inflight = Some("usage");
                        usage_request(act, &fresh.access);
                        act.store = Some(keep(&st.tokens));
                    }
                    None => failed(st, "the sign-in returned no access token", false),
                }
            }
            400 | 401 => failed(st, "The sign-in ran out. Sign in again.", true),
            429 => failed(st, "Claude is rate limiting the sign-in renewal.", false),
            _ => failed(st, net(), false),
        },
        (Some("exchange"), "exchange") => match ev.status {
            200 => match tokens_of(&data, "", ev.now) {
                Some(t) => {
                    st.tokens = Some(t);
                    st.pending = None;
                    st.snap = Snapshot::default();
                    act.store = Some(keep(&st.tokens));
                    refresh(st, ev.now, act);
                }
                None => failed(st, "the sign-in returned no access token", false),
            },
            400 | 401 => failed(st, "Claude did not accept that code. Start the sign-in again and paste the new one.", true),
            429 => failed(st, "Claude is rate limiting the sign-in. Try again in a minute.", false),
            _ => failed(st, net(), false),
        },
        // (an answer that is not the one it waited for)
        (other, _) => st.inflight = other,
    }
}

fn handle(ev: &Event) -> Reply {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let wanted = ev.setting_bool("usage", true);
        let now = ev.now;
        let mut act = Act::default();
        match ev.event.as_str() {
            "start" => {
                *st = State::default();
                let login = &ev.store["login"];
                if let (Some(access), Some(refresh)) = (login["access"].as_str(), login["refresh"].as_str()) {
                    st.tokens = Some(Tokens { access: access.into(), refresh: refresh.into(), expires_ms: login["expires_ms"].as_u64().unwrap_or(0) });
                }
            }
            "fetched" => answered(&mut st, ev, &mut act),
            "action" => match ev.action.as_str() {
                // Sign in: the browser opens on Claude's page, which shows a code to paste back
                "usage_login" => {
                    let verifier = b64url(&unhex(&ev.rand));
                    if verifier.len() < 43 {
                        act.note = Some("Usage rings: this PC gave no randomness to sign in with.".into());
                    } else {
                        let state = b64url(&Sha256::digest(format!("{}state", ev.rand).as_bytes()));
                        act.open = Some(authorize_url(&challenge_of(&verifier), &state));
                        st.pending = Some((verifier, state));
                    }
                }
                "usage_finish" => {
                    let code = code_of(ev.args["text"].as_str().unwrap_or("")).to_string();
                    if code.is_empty() {
                        // (nothing in the field: the sign-in is forgotten)
                        st.tokens = None;
                        st.pending = None;
                        st.snap = Snapshot { needs_login: true, ..Snapshot::default() };
                        st.last_pct = (None, None);
                        act.store = Some(keep(&None));
                    } else if let Some((verifier, state)) = st.pending.clone() {
                        st.inflight = Some("exchange");
                        token_request(
                            &mut act,
                            "exchange",
                            json!({ "grant_type": "authorization_code", "client_id": CLIENT_ID, "code": code, "redirect_uri": REDIRECT_URL, "code_verifier": verifier, "state": state }),
                        );
                    } else {
                        act.note = Some("Usage rings: start the sign-in first.".into());
                    }
                }
                // A click on the bars or a ring: ask again, at most once a minute (the endpoint rate-limits)
                "widget" | "ring" => {
                    let cooling = st.last_manual.map_or(false, |t| now.saturating_sub(t) < MANUAL_COOLDOWN_MS);
                    if !cooling && st.inflight.is_none() {
                        st.last_manual = Some(now);
                        refresh(&mut st, now, &mut act);
                    }
                }
                _ => return Reply::nothing(),
            },
            // the peek moves on (its own wake), and the poll comes due (the manifest's tick)
            "tick" => {
                if ev.wake && st.peek.is_some() {
                    match st.peek {
                        Some(Peek::Before) => {
                            st.peek = Some(Peek::After);
                            act.pill = Some(Some(peek_pill(st.peek_new)));
                            act.wake = Some(PEEK_AFTER_MS);
                        }
                        _ => {
                            st.peek = None;
                            act.pill = Some(None);
                        }
                    }
                } else if !wanted {
                    st.snap = Snapshot::default();
                    st.last_pct = (None, None);
                    st.inflight = None;
                } else {
                    let every = if st.snap.available { POLL_MS } else { POLL_EMPTY_MS };
                    if st.inflight.is_none() && st.last_poll.map_or(true, |t| now.saturating_sub(t) >= every) {
                        refresh(&mut st, now, &mut act);
                    }
                }
            }
            _ => return Reply::nothing(),
        }
        render(&st, now, wanted, act)
    })
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000_000;

    fn ev(kind: &str, now: u64) -> Event {
        Event { event: kind.into(), now, settings: json!({}), ..Event::default() }
    }

    fn fresh() {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }

    fn signed_in(expires: u64) -> Event {
        Event { store: json!({ "login": { "access": "ACC", "refresh": "REF", "expires_ms": expires } }), ..ev("start", T0) }
    }

    fn answer(id: &str, status: u16, body: Value, now: u64) -> Event {
        Event { id: id.into(), status, body: body.to_string(), ..ev("fetched", now) }
    }

    fn usage_body(five: f64, seven: f64) -> Value {
        json!({ "five_hour": { "utilization": five, "resets_at": "2026-10-16T18:00:00+00:00" }, "seven_day": { "utilization": seven, "resets_at": "2026-10-20T00:00:00Z" } })
    }

    #[test]
    fn the_challenge_is_the_sha256_of_the_verifier() {
        // the example of RFC 7636, appendix B
        assert_eq!(challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(b64url(&[0xfb, 0xff, 0xfe]), "-__-");
        assert_eq!(b64url(b"M"), "TQ");
        assert_eq!(b64url(&unhex(&"ab".repeat(32))).len(), 43);
    }

    #[test]
    fn the_sign_in_page_is_asked_for_a_code_and_the_pasted_one_loses_its_state() {
        let url = authorize_url("CHAL", "STATE");
        assert!(url.starts_with("https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fconsole.anthropic.com%2Foauth%2Fcode%2Fcallback"));
        assert!(url.contains("scope=org%3Acreate_api_key%20user%3Aprofile%20user%3Ainference"));
        assert!(url.ends_with("code_challenge=CHAL&code_challenge_method=S256&state=STATE"));
        assert_eq!(code_of("abc123#xyz"), "abc123");
        assert_eq!(code_of("  abc123#xyz \r\n"), "abc123");
        assert_eq!(code_of("   "), "");
    }

    #[test]
    fn a_time_is_read_and_a_window_says_when_it_opens_again() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2026-10-16T14:05:00.5+09:00"), Some(parse_iso("2026-10-16T05:05:00Z").unwrap() + 500));
        let now = parse_iso("2026-10-16T14:00:00Z").unwrap() as u64;
        assert_eq!(reset_in(&Some("2026-10-16T14:40:00Z".into()), now), "40m");
        assert_eq!(reset_in(&Some("2026-10-16T17:02:00Z".into()), now), "3h 2m");
        assert_eq!(reset_in(&Some("2026-10-18T18:00:00Z".into()), now), "2d 4h");
        assert_eq!(reset_in(&Some("2026-10-15T00:00:00Z".into()), now), "0m");
        assert_eq!(reset_in(&None, now), "");
    }

    #[test]
    fn signed_out_it_has_nothing_to_show_and_says_how_to_get_it() {
        fresh();
        handle(&Event { store: Value::Null, ..ev("start", T0) });
        let r = handle(&ev("tick", T0 + 5000)).to_json();
        assert_eq!(r["note"], "Usage rings: sign in below to see how much of your limits is used.");
        assert_eq!(r["widgets"]["bars"]["blocks"], json!([]));
        assert_eq!(r["rings"], json!([]));
        assert!(r.get("fetch").is_none());
    }

    #[test]
    fn signed_in_it_asks_and_draws_what_comes_back() {
        fresh();
        let now = parse_iso("2026-10-16T14:00:00Z").unwrap() as u64;
        handle(&Event { store: json!({ "login": { "access": "ACC", "refresh": "REF", "expires_ms": now + 3_600_000 } }), ..ev("start", now) });
        let r = handle(&ev("tick", now + 5000)).to_json();
        let f = &r["fetch"][0];
        assert_eq!((f["id"].as_str(), f["method"].as_str(), f["url"].as_str()), (Some("usage"), Some("GET"), Some(USAGE_URL)));
        assert_eq!(f["headers"]["Authorization"], "Bearer ACC");
        assert_eq!(f["headers"]["anthropic-beta"], BETA);
        // not asked again while the answer is out
        assert!(handle(&ev("tick", now + 6000)).to_json().get("fetch").is_none());
        let r = handle(&answer("usage", 200, usage_body(42.0, 18.4), now + 6500)).to_json();
        let bars = r["widgets"]["bars"]["blocks"][0]["bars"].as_array().unwrap();
        assert_eq!((bars[0]["label"].as_str(), bars[0]["color"].as_str()), (Some("5H"), Some(FIVE)));
        assert_eq!(bars[1]["part"].as_f64().map(|p| (p * 100.0).round()), Some(18.0));
        assert_eq!(r["widgets"]["bars"]["tip"][0]["bars"][0]["text"], "42%");
        assert_eq!(r["widgets"]["rings"]["blocks"][0]["rings"].as_array().unwrap().len(), 2);
        assert_eq!(r["widgets"]["rings"]["blocks"][0]["rings"][0]["label"], "5H");
        assert_eq!(r["widgets"]["bars"]["tip"][0]["bars"][0]["sub"], "Back in 3h 59m");
        let rings = r["rings"].as_array().unwrap();
        assert_eq!((rings[0]["id"].as_str(), rings[0]["start"].as_bool()), (Some("five_hour"), Some(false)));
        assert_eq!(rings[0]["tip"], "Claude 5h · 42%\nResets in 3h 59m · Click to refresh");
        assert_eq!(r["note"], Value::Null);
        // the first numbers are not news; asked again two minutes on, nothing moved, no peek
        let r = handle(&ev("tick", now + 5000 + POLL_MS)).to_json();
        assert_eq!(r["fetch"][0]["id"], "usage");
        let r = handle(&answer("usage", 200, usage_body(42.2, 18.4), now + 5500 + POLL_MS)).to_json();
        assert!(r.get("pill").is_none());
    }

    #[test]
    fn a_number_that_moved_peeks_on_the_island_and_the_peek_comes_back() {
        fresh();
        handle(&signed_in(0));
        handle(&ev("tick", T0 + 5000));
        handle(&answer("usage", 200, usage_body(42.0, 18.0), T0 + 5500));
        handle(&ev("tick", T0 + 5000 + POLL_MS));
        let r = handle(&answer("usage", 200, usage_body(47.0, 18.0), T0 + 6000 + POLL_MS)).to_json();
        let pill = &r["pill"];
        assert_eq!((pill["brief"].as_bool(), pill["rank"].as_i64()), (Some(true), Some(100)));
        assert_eq!((pill["rows"][0]["label"].as_str(), pill["rows"][0]["text"].as_str()), (Some("5h"), Some("42%")));
        assert_eq!(pill["rows"][0]["value"].as_f64().map(|v| (v * 100.0).round()), Some(42.0));
        assert_eq!(r["wake_ms"], PEEK_BEFORE_MS);
        // it slides to the new numbers ...
        let wake = Event { wake: true, ..ev("tick", T0 + 6600 + POLL_MS) };
        let r = handle(&wake).to_json();
        assert_eq!(r["pill"]["rows"][0]["text"], "47%");
        assert_eq!(r["wake_ms"], PEEK_AFTER_MS);
        // ... and is taken back
        let r = handle(&wake).to_json();
        assert_eq!(r["pill"], Value::Null);
        // over the hub there is no peek
        handle(&ev("tick", T0 + 7000 + 2 * POLL_MS));
        let hubbed = Event { hub: true, ..answer("usage", 200, usage_body(60.0, 18.0), T0 + 7500 + 2 * POLL_MS) };
        assert!(handle(&hubbed).to_json().get("pill").is_none());
    }

    #[test]
    fn a_token_about_to_run_out_is_renewed_and_what_comes_back_is_kept() {
        fresh();
        handle(&signed_in(T0 + 30_000));
        let r = handle(&ev("tick", T0 + 5000)).to_json();
        let f = &r["fetch"][0];
        assert_eq!((f["id"].as_str(), f["method"].as_str(), f["url"].as_str()), (Some("renew"), Some("POST"), Some(TOKEN_URL)));
        assert_eq!(f["headers"]["User-Agent"], OAUTH_AGENT);
        let body: Value = serde_json::from_str(f["body"].as_str().unwrap()).unwrap();
        assert_eq!((body["grant_type"].as_str(), body["refresh_token"].as_str()), (Some("refresh_token"), Some("REF")));
        let r = handle(&answer("renew", 200, json!({ "access_token": "NEW", "expires_in": 3600 }), T0 + 5500)).to_json();
        assert_eq!(r["store"]["login"]["access"], "NEW");
        assert_eq!(r["store"]["login"]["refresh"], "REF", "(none came back: the old one stays)");
        assert_eq!(r["fetch"][0]["headers"]["Authorization"], "Bearer NEW");
    }

    #[test]
    fn a_refused_token_is_renewed_once_and_then_asks_for_a_sign_in() {
        fresh();
        handle(&signed_in(0));
        handle(&ev("tick", T0 + 5000));
        let r = handle(&answer("usage", 401, Value::Null, T0 + 5500)).to_json();
        assert_eq!(r["fetch"][0]["id"], "renew");
        handle(&answer("renew", 200, json!({ "access_token": "NEW", "refresh_token": "REF2", "expires_in": 3600 }), T0 + 6000));
        let r = handle(&answer("usage", 401, Value::Null, T0 + 6500)).to_json();
        assert_eq!(r["note"], "Usage rings: The sign-in was refused. Sign in again.");
        assert!(r.get("fetch").is_none());
        // a busy endpoint is not a sign-in matter, and the numbers that were there stay
        fresh();
        handle(&signed_in(0));
        handle(&ev("tick", T0 + 5000));
        handle(&answer("usage", 200, usage_body(10.0, 20.0), T0 + 5500));
        handle(&ev("tick", T0 + 5000 + POLL_MS));
        let r = handle(&answer("usage", 429, Value::Null, T0 + 5500 + POLL_MS)).to_json();
        assert_eq!(r["note"], "Usage rings: Rate limited by the usage endpoint -- try again in a bit.");
        assert_eq!(r["widgets"]["bars"]["blocks"][0]["bars"].as_array().unwrap().len(), 2);
        assert!(r["rings"][0]["tip"].as_str().unwrap().contains("Not up to date"));
    }

    #[test]
    fn signing_in_opens_the_page_asks_for_the_code_and_keeps_the_login() {
        fresh();
        handle(&Event { store: Value::Null, ..ev("start", T0) });
        let rand = "ab".repeat(32);
        let r = handle(&Event { action: "usage_login".into(), rand: rand.clone(), ..ev("action", T0 + 1000) }).to_json();
        let url = r["open"].as_str().unwrap();
        assert!(url.starts_with("https://claude.ai/oauth/authorize?code=true"));
        let verifier = b64url(&unhex(&rand));
        assert!(url.contains(&format!("code_challenge={}", challenge_of(&verifier))));
        // the code is pasted: it is exchanged, with the verifier
        let r = handle(&Event { action: "usage_finish".into(), args: json!({ "text": " the-code#state " }), ..ev("action", T0 + 9000) }).to_json();
        let body: Value = serde_json::from_str(r["fetch"][0]["body"].as_str().unwrap()).unwrap();
        assert_eq!((body["grant_type"].as_str(), body["code"].as_str(), body["code_verifier"].as_str()), (Some("authorization_code"), Some("the-code"), Some(verifier.as_str())));
        let r = handle(&answer("exchange", 200, json!({ "access_token": "A", "refresh_token": "R", "expires_in": 3600 }), T0 + 9500)).to_json();
        assert_eq!(r["store"]["login"]["refresh"], "R");
        assert_eq!(r["fetch"][0]["id"], "usage");
        // a code that is not taken
        fresh();
        handle(&Event { store: Value::Null, ..ev("start", T0) });
        handle(&Event { action: "usage_login".into(), rand, ..ev("action", T0 + 1000) });
        handle(&Event { action: "usage_finish".into(), args: json!({ "text": "bad" }), ..ev("action", T0 + 2000) });
        let r = handle(&answer("exchange", 400, Value::Null, T0 + 2500)).to_json();
        assert_eq!(r["note"], "Usage rings: Claude did not accept that code. Start the sign-in again and paste the new one.");
        // the code asked for without the page opened first
        fresh();
        let r = handle(&Event { action: "usage_finish".into(), args: json!({ "text": "x" }), ..ev("action", T0) }).to_json();
        assert_eq!(r["note"], "Usage rings: start the sign-in first.");
    }

    #[test]
    fn an_empty_code_signs_out() {
        fresh();
        handle(&signed_in(0));
        let r = handle(&Event { action: "usage_finish".into(), args: json!({ "text": "" }), ..ev("action", T0 + 1000) }).to_json();
        assert_eq!(r["store"], json!({}));
        assert_eq!(r["note"], "Usage rings: sign in below to see how much of your limits is used.");
        assert!(handle(&ev("tick", T0 + 5000)).to_json().get("fetch").is_none());
    }

    #[test]
    fn a_click_asks_again_but_not_more_than_once_a_minute_and_the_setting_off_shows_nothing() {
        fresh();
        handle(&signed_in(0));
        let click = |now: u64| handle(&Event { action: "widget".into(), ..ev("action", now) }).to_json();
        assert_eq!(click(T0 + 1000)["fetch"][0]["id"], "usage");
        handle(&answer("usage", 200, usage_body(1.0, 2.0), T0 + 1500));
        assert!(click(T0 + 20_000).get("fetch").is_none());
        assert_eq!(click(T0 + 62_000)["fetch"][0]["id"], "usage");
        // the setting off: no numbers, no note
        handle(&answer("usage", 200, usage_body(1.0, 2.0), T0 + 62_500));
        let off = Event { settings: json!({ "usage": false }), ..ev("tick", T0 + 70_000) };
        let r = handle(&off).to_json();
        assert_eq!(r["rings"], json!([]));
        assert_eq!(r["widgets"]["bars"]["blocks"], json!([]));
        assert_eq!(r["note"], Value::Null);
    }
}
