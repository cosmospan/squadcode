//! `squad serve`: the crew with no window, for a server such as a VPS.
//!
//! One `Runtime` runs here, the same one the desktop app runs inside its
//! window, and every client reaches it through one small HTTP API: the
//! desktop app, a web page, the phone, a chat bot or a script. They all see
//! the same jobs, and `GET /v1/events` streams what changes as it changes, so
//! a job started from one client shows up live in all of them.
//!
//! The server listens on 127.0.0.1 unless told otherwise, so nothing is open
//! to the internet: devices reach it over an SSH tunnel or Tailscale. Every
//! request but pairing needs `Authorization: Bearer <token>`. Each device has
//! its own token, kept in `~/.squadcode/server.json`; `squad pair` prints a
//! one-time code that a device trades for one, and `squad unpair <name>`
//! revokes one.
//!
//! `squad new` and the other queue commands work here too: the runtime picks
//! the queue up, so a terminal on the server is one more client.

use std::collections::HashMap;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::phone::{self, Http};
use crate::settings::AppSettings;

use super::data::{Member, Mode};
use super::runtime::{Opened, Runtime};
use super::store::{self, Job, Status, Store};

/// The port the server listens on unless told otherwise.
pub const PORT: u16 = 7640;
/// The API version in every path (`/v1/...`).
const API: u32 = 1;
/// How long a pairing code works.
const CODE_LIFE: u64 = 10 * 60;
/// Wrong codes before the current one stops working.
const CODE_TRIES: u32 = 5;
/// How often the crew is moved along.
const TICK: Duration = Duration::from_millis(250);

// ---------- devices and pairing ----------

/// One paired device: the desktop app, a browser, a phone, a bot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub token: String,
    /// When it was paired, in seconds since 1970.
    #[serde(default)]
    pub created: u64,
}

/// A code waiting to be traded for a token.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Pairing {
    code: String,
    /// The device's name, when `squad pair <name>` gave one.
    name: String,
    expires: u64,
    misses: u32,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    devices: Vec<Device>,
    pairing: Option<Pairing>,
}

fn config_path(base: &Path) -> PathBuf {
    base.join("server.json")
}

fn load(base: &Path) -> Config {
    let mut c: Config = crate::store::read_json_or_backup(&config_path(base));
    c.devices.retain(|d| d.token.len() >= 32);
    c
}

fn save(base: &Path, c: &Config) {
    // Request threads may save at the same moment; one writes at a time.
    static WRITING: Mutex<()> = Mutex::new(());
    let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(text) = serde_json::to_vec_pretty(c) {
        let _ = crate::store::write_private(&config_path(base), &text);
    }
}

pub fn devices(base: &Path) -> Vec<Device> {
    load(base).devices
}

/// A new one-time pairing code, replacing any waiting one. Short enough to
/// type: eight letters and digits that can't be misread.
pub fn new_code(base: &Path, name: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let raw = phone::new_token();
    let code: String = raw
        .as_bytes()
        .chunks(2)
        .take(8)
        .map(|pair| {
            let n = u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0);
            ALPHABET[n as usize % ALPHABET.len()] as char
        })
        .collect();
    let mut c = load(base);
    c.pairing = Some(Pairing {
        code: code.clone(),
        name: name.trim().to_string(),
        expires: store::now() + CODE_LIFE,
        misses: 0,
    });
    save(base, &c);
    code
}

/// Trades a pairing code for a device token. The code works once, for ten
/// minutes, and stops working after a few wrong tries.
fn pair(base: &Path, code: &str, name: &str) -> Result<Device, String> {
    let mut c = load(base);
    let Some(p) = c.pairing.as_mut() else {
        return Err("No pairing code is waiting. Run `squad pair` on the server.".into());
    };
    if store::now() > p.expires {
        c.pairing = None;
        save(base, &c);
        return Err("That code has expired. Run `squad pair` on the server for a new one.".into());
    }
    let given = code.trim().to_ascii_uppercase().replace(['-', ' '], "");
    if !phone::allowed(&given, std::slice::from_ref(&p.code)) {
        p.misses += 1;
        if p.misses >= CODE_TRIES {
            c.pairing = None;
        }
        save(base, &c);
        return Err("That code isn't right.".into());
    }
    let wanted = if p.name.is_empty() {
        name.trim()
    } else {
        p.name.as_str()
    };
    let wanted = if wanted.is_empty() { "Device" } else { wanted }.to_string();
    let name = (1..)
        .map(|n| match n {
            1 => wanted.clone(),
            n => format!("{wanted} {n}"),
        })
        .find(|n| !c.devices.iter().any(|d| d.name == *n))
        .unwrap_or_default();
    let device = Device {
        name,
        token: phone::new_token(),
        created: store::now(),
    };
    c.devices.push(device.clone());
    c.pairing = None;
    save(base, &c);
    Ok(device)
}

/// Revokes a device by name. False when there was none.
pub fn unpair(base: &Path, name: &str) -> bool {
    let mut c = load(base);
    let before = c.devices.len();
    c.devices.retain(|d| d.name != name);
    let gone = c.devices.len() < before;
    save(base, &c);
    gone
}

/// The device holding `token`, read fresh for every request, so a device
/// removed by `squad unpair` is refused straight away.
fn device_for(base: &Path, auth: &str) -> Option<Device> {
    let given = auth.strip_prefix("Bearer ").unwrap_or("");
    let devices = devices(base);
    let tokens: Vec<String> = devices.iter().map(|d| d.token.clone()).collect();
    if !phone::allowed(given, &tokens) {
        return None;
    }
    devices.into_iter().find(|d| d.token == given)
}

// ---------- the crew and what changed ----------

/// The crew and the clients listening to it.
pub struct Hub {
    pub rt: Runtime,
    st: AppSettings,
    /// Live event streams, one per open `GET /v1/events`.
    subs: Vec<Sender<String>>,
    /// What each job, chat and the goals looked like at the last tick, to
    /// tell clients only what moved.
    marks: HashMap<String, u64>,
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

/// What a client would redraw a job for.
fn job_mark(j: &Job) -> u64 {
    fnv(&format!(
        "{:?}|{}|{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        j.status,
        j.feed.len(),
        j.plan,
        j.changes.len(),
        j.checks.len(),
        j.checking,
        j.pr_state,
        j.pr_line,
        j.seen,
        j.shipping,
        j.conflict.len(),
        j.worktree
    ))
}

impl Hub {
    pub fn new(rt: Runtime, st: AppSettings) -> Self {
        let mut hub = Hub {
            rt,
            st,
            subs: Vec::new(),
            marks: HashMap::new(),
        };
        // What's there at the start isn't news.
        hub.changes();
        hub
    }

    /// Moves the crew along and tells every listener what changed.
    pub fn tick(&mut self) {
        // Jobs' browsers nobody has used for a while are closed.
        crate::browse::host::reap_now_and_then();
        let finished = self.rt.poll(&self.st, None);
        for id in finished {
            self.send(json!({ "kind": "ended", "id": id }));
        }
        for note in std::mem::take(&mut self.rt.notes) {
            self.send(json!({ "kind": "note", "text": note }));
        }
        for ev in self.changes() {
            self.send(ev);
        }
    }

    /// Events for what moved since the last call.
    fn changes(&mut self) -> Vec<Value> {
        let mut now: HashMap<String, u64> = HashMap::new();
        let mut out = Vec::new();
        for j in &self.rt.store.jobs {
            now.insert(format!("job/{}", j.id), job_mark(j));
        }
        for (who, said) in &self.rt.store.chats {
            now.insert(format!("chat/{}", who.name()), said.len() as u64);
        }
        now.insert(
            "goals".into(),
            fnv(&serde_json::to_string(&self.rt.store.goals).unwrap_or_default()),
        );
        now.insert(
            "folders".into(),
            fnv(&format!(
                "{:?}{}",
                self.rt
                    .store
                    .folders
                    .iter()
                    .map(|f| &f.path)
                    .collect::<Vec<_>>(),
                self.rt.store.current
            )),
        );
        for (key, mark) in &now {
            if self.marks.get(key) == Some(mark) {
                continue;
            }
            out.push(match key.split_once('/') {
                Some(("job", id)) => {
                    let id: u64 = id.parse().unwrap_or(0);
                    let status = self
                        .rt
                        .store
                        .job(id)
                        .map(|j| status_name(j.status))
                        .unwrap_or("");
                    json!({ "kind": "job", "id": id, "status": status })
                }
                Some(("chat", bot)) => json!({ "kind": "chat", "bot": bot }),
                _ => json!({ "kind": key }),
            });
        }
        // Jobs are never removed while the server runs, but say so if one is.
        for key in self.marks.keys() {
            if let Some(id) = key.strip_prefix("job/") {
                if !now.contains_key(key) {
                    out.push(json!({ "kind": "gone", "id": id.parse::<u64>().unwrap_or(0) }));
                }
            }
        }
        self.marks = now;
        out
    }

    fn send(&mut self, ev: Value) {
        let line = ev.to_string();
        self.subs.retain(|tx| tx.send(line.clone()).is_ok());
    }
}

// ---------- reading the store for clients ----------

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Running => "running",
        Status::Done => "done",
        Status::Failed => "failed",
        Status::Stopped => "stopped",
        Status::Interrupted => "interrupted",
        Status::Review => "review",
        Status::Shipping => "shipping",
        Status::Shipped => "shipped",
        Status::Discarded => "discarded",
    }
}

/// One job, short, for lists.
fn summary(j: &Job) -> Value {
    let (added, removed) = j.lines();
    json!({
        "id": j.id,
        "title": j.title,
        "bot": j.bot.name(),
        "mode": format!("{:?}", j.mode).to_lowercase(),
        "status": status_name(j.status),
        "label": j.status.label(),
        "doing": j.doing(),
        "needs_you": j.needs_you(),
        "seen": j.seen,
        "engine": j.engine,
        "folder": j.folder.path,
        "branch": j.branch,
        "started": j.started,
        "ended": j.ended,
        "files": j.changes.len(),
        "added": added,
        "removed": removed,
        "pr": j.pr,
        "race": j.race,
        "goal": j.goal,
        "last_words": j.last_words().map(|w| w.chars().take(240).collect::<String>()),
    })
}

fn state(hub: &Hub) -> Value {
    let s = &hub.rt.store;
    json!({
        "folders": s.folders.iter().map(|f| json!({
            "name": f.name,
            "path": f.path,
            "remote": super::ship::remote_url(&f.path),
        })).collect::<Vec<_>>(),
        // The agents ready here, by id, so a client can tell what can run here.
        "engines": (0..crate::agent::PROVIDERS.len())
            .filter(|i| super::engine::ready(&hub.st, *i))
            .map(|i| crate::agent::PROVIDERS[i].id)
            .collect::<Vec<_>>(),
        "current": s.current,
        "jobs": s.jobs.iter().rev().map(summary).collect::<Vec<_>>(),
        "goals": s.goals,
        "crew": Member::all().into_iter().filter(|m| !s.removed.contains(m)).map(|m| json!({
            "name": m.name(),
            "busy": s.busy(m).map(|j| j.id),
            "thinking": hub.rt.thinking(m),
        })).collect::<Vec<_>>(),
        "running": hub.rt.runners.len(),
        // Each agent this server knows, whether it's installed and whether it
        // has a login or a key, so a client can show what's missing.
        "logins": logins(&hub.st),
    })
}

/// Whether each agent is installed here and signed in: an API key or token in
/// the environment (server.env included), or its login file in the home folder.
fn logins(st: &AppSettings) -> Vec<Value> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    crate::connections::SIGN_INS
        .iter()
        .filter_map(|s| {
            let i = crate::agent::PROVIDERS
                .iter()
                .position(|p| p.id == s.provider)?;
            let key = s
                .keys
                .iter()
                .any(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()));
            let file = s.file.is_some_and(|f| home.join(f).is_file());
            Some(json!({
                "agent": s.provider,
                "name": crate::agent::PROVIDERS[i].name,
                "installed": super::engine::ready(st, i),
                "signed_in": key || file,
            }))
        })
        .collect()
}

/// The keys and tokens the server's agents use, set from the desktop app:
/// `KEY=value` lines in server.env, read when the server starts.
pub fn env_path() -> PathBuf {
    crate::store::data_dir().join("server.env")
}

fn load_env() {
    let Ok(text) = std::fs::read_to_string(env_path()) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let (k, v) = (k.trim(), v.trim());
            if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                std::env::set_var(k, v);
            }
        }
    }
}

/// `squad serve --set KEY=value`: keep a key for the server's agents (an empty
/// value removes it), then start the server again so it takes effect.
fn set_env(pair: &str) -> i32 {
    // `--set KEY` alone reads the value from stdin, so it never shows in `ps`.
    let mut typed = String::new();
    let (key, value) = match pair.split_once('=') {
        Some(kv) => kv,
        None => {
            let _ = std::io::stdin().read_line(&mut typed);
            (pair, typed.trim())
        }
    };
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        eprintln!("squad: {key} isn't a name for a key.");
        return 2;
    }
    let path = env_path();
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = old
        .lines()
        .filter(|l| l.split_once('=').map(|(k, _)| k.trim()) != Some(key))
        .map(str::to_string)
        .collect();
    if !value.trim().is_empty() {
        lines.push(format!("{key}={}", value.trim()));
    }
    let text = if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    };
    if let Err(e) = std::fs::write(&path, text) {
        eprintln!("squad: couldn't keep the key: {e}");
        return 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    0
}

fn mode_of(word: &str) -> Option<Mode> {
    Mode::ALL
        .into_iter()
        .find(|m| format!("{m:?}").eq_ignore_ascii_case(word.trim()))
}

fn bot_of(name: &str) -> Option<Member> {
    Member::all()
        .into_iter()
        .find(|m| m.name().eq_ignore_ascii_case(name.trim()))
}

// ---------- routing ----------

type Reply = (u16, Value);

fn err(status: u16, text: impl Into<String>) -> Reply {
    (status, json!({ "error": text.into() }))
}

/// Routes one request. Separate from the socket so tests can call it.
pub fn respond(http: &Http, base: &Path, hub: &Mutex<Hub>) -> Reply {
    let path = http.path.split('?').next().unwrap_or("");
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let (get, post, delete) = (
        http.method == "GET",
        http.method == "POST",
        http.method == "DELETE",
    );
    // A trigger's link: its secret is in the path, so it needs no paired device.
    // What it sends may be any text, not only JSON.
    if let Some(token) = super::triggers::token_in(path) {
        if !post {
            return err(405, "Call a trigger's link with POST.");
        }
        let sent = super::triggers::sent_of(&http.body);
        let mut hub = hub.lock().unwrap_or_else(|e| e.into_inner());
        return match hub.rt.fire_link(token, &sent) {
            Ok(name) => (
                202,
                json!({ "trigger": name, "message": "Handed to the crew." }),
            ),
            Err(why) if why.starts_with("No trigger") => err(404, why),
            Err(why) => err(409, why),
        };
    }
    let body: Value = if http.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&http.body) {
            Ok(v) => v,
            Err(_) => return err(400, "The body isn't JSON."),
        }
    };
    // Open to anyone who can reach the server: saying hello and pairing.
    match parts.as_slice() {
        ["v1", "hello"] if get => {
            return (
                200,
                json!({ "app": "Squad Code", "api": API, "version": env!("CARGO_PKG_VERSION") }),
            )
        }
        ["v1", "pair"] if post => {
            let code = body["code"].as_str().unwrap_or("");
            let name = body["name"].as_str().unwrap_or("");
            return match pair(base, code, name) {
                Ok(d) => (200, json!({ "name": d.name, "token": d.token })),
                Err(why) => err(401, why),
            };
        }
        _ => {}
    }
    let Some(device) = device_for(base, &http.auth) else {
        return err(
            401,
            "This device isn't paired. Run `squad pair` on the server and pair it again.",
        );
    };
    let mut hub = hub.lock().unwrap_or_else(|e| e.into_inner());
    let st = hub.st.clone();
    let reply = match parts.as_slice() {
        ["v1", "me"] if get => (200, json!({ "name": device.name })),
        ["v1", "state"] if get => (200, state(&hub)),
        ["v1", "jobs", id] if get => match id.parse().ok().and_then(|id| hub.rt.store.job(id)) {
            Some(j) => (200, json!(j)),
            None => err(404, "No such job."),
        },
        ["v1", "jobs"] if post => new_job(&mut hub, &st, &body),
        ["v1", "jobs", id, action] if post => {
            let Some(id) = id.parse::<u64>().ok().filter(|id| hub.rt.store.job(*id).is_some())
            else {
                return err(404, "No such job.");
            };
            match *action {
                "approve" => hub.rt.approve(id),
                "approve-anyway" => hub.rt.approve_anyway(id),
                "set-aside-and-merge" => hub.rt.set_aside_and_merge(id),
                "bring-back" => hub.rt.bring_back(id),
                "leave-kept" => hub.rt.leave_kept(id),
                "take-out" => hub.rt.take_out(&st, id),
                "discard" => hub.rt.discard(id),
                "stop" => hub.rt.stop(id),
                "recheck" => hub.rt.recheck(id),
                "rerun" => {
                    hub.rt.rerun(&st, id, None);
                }
                "carry-on" => hub.rt.carry_on(&st, id),
                "resolve" => hub.rt.resolve(&st, id),
                "race" => {
                    hub.rt.race_it(&st, id);
                }
                "follow-up" => {
                    let text = body["text"].as_str().unwrap_or("").trim().to_string();
                    if text.is_empty() {
                        return err(400, "Say what to add: {\"text\": \"...\"}.");
                    }
                    hub.rt.follow_up(&st, id, text);
                }
                "merge-pr" | "close-pr" => hub.rt.land_pr(id, *action == "merge-pr"),
                "seen" => {
                    if let Some(j) = hub.rt.store.job_mut(id) {
                        j.seen = true;
                    }
                    hub.rt.store.save();
                }
                _ => return err(404, "No such action."),
            }
            (200, json!({ "ok": true, "job": hub.rt.store.job(id).map(summary) }))
        }
        ["v1", "chats", bot] if get => match bot_of(bot) {
            Some(who) => (200, json!(hub.rt.store.chat(who))),
            None => err(404, "No bot by that name."),
        },
        ["v1", "chats", bot] if post => {
            let Some(who) = bot_of(bot) else {
                return err(404, "No bot by that name.");
            };
            let text = body["text"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return err(400, "Say what to ask: {\"text\": \"...\"}.");
            }
            hub.rt.ask(&st, who, text);
            (202, json!({ "ok": true }))
        }
        ["v1", "goals", id, action] if post => {
            let Ok(id) = id.parse::<u64>() else {
                return err(404, "No such goal.");
            };
            match *action {
                "start" => hub.rt.start_goal(&st, id),
                "skip" => hub.rt.skip_step(&st, id),
                "drop" => hub.rt.drop_goal(id),
                _ => return err(404, "No such action."),
            }
            (200, json!({ "ok": true }))
        }
        ["v1", "folders"] if post => {
            let path = body["path"].as_str().unwrap_or("").trim();
            if path.is_empty() || !Path::new(path).is_dir() {
                return err(400, "That folder isn't on the server.");
            }
            hub.rt.store.add_folder(PathBuf::from(path));
            hub.rt.store.save();
            (200, json!({ "ok": true, "current": hub.rt.store.current }))
        }
        ["v1", "triggers"] if get => (200, json!(hub.rt.store.triggers)),
        // A trigger made from another device: its project is a folder on the server.
        ["v1", "triggers"] if post => {
            let mut t: super::triggers::Trigger = match serde_json::from_value(body.clone()) {
                Ok(t) => t,
                Err(e) => return err(400, format!("That isn't a trigger: {e}")),
            };
            let path = body["dir"].as_str().unwrap_or(&t.folder.path).trim().to_string();
            if path.is_empty() || !Path::new(&path).is_dir() {
                return err(400, "That folder isn't on the server.");
            }
            t.folder = hub.rt.folder_for(&path);
            match hub.rt.add_trigger(t) {
                Ok(id) => {
                    let t = hub.rt.store.triggers.iter().find(|t| t.id == id);
                    (200, json!(t))
                }
                Err(why) => err(400, why),
            }
        }
        ["v1", "triggers", id, action] if post => {
            let Ok(id) = id.parse::<u64>() else {
                return err(404, "No such trigger.");
            };
            if !hub.rt.store.triggers.iter().any(|t| t.id == id) {
                return err(404, "No such trigger.");
            }
            match *action {
                "on" => hub.rt.set_trigger_on(id, true),
                "off" => hub.rt.set_trigger_on(id, false),
                "remove" => hub.rt.remove_trigger(id),
                "run" => {
                    if let Err(why) = hub.rt.run_trigger_now(id) {
                        return err(409, why);
                    }
                }
                _ => return err(404, "No such action."),
            }
            (200, json!({ "ok": true }))
        }
        ["v1", "devices"] if get => (
            200,
            json!(devices(base)
                .into_iter()
                .map(|d| json!({ "name": d.name, "created": d.created, "you": d.name == device.name }))
                .collect::<Vec<_>>()),
        ),
        ["v1", "devices", name] if delete => {
            let name = decode(name);
            if unpair(base, &name) {
                (200, json!({ "ok": true }))
            } else {
                err(404, "No device by that name.")
            }
        }
        ["v1", ..] => err(405, "Not something this server does."),
        _ => err(404, "Not found."),
    };
    // What the request changed reaches every listener now, not at the next tick.
    for ev in hub.changes() {
        hub.send(ev);
    }
    for note in std::mem::take(&mut hub.rt.notes) {
        hub.send(json!({ "kind": "note", "text": note }));
    }
    reply
}

/// `POST /v1/jobs`: `{text, mode?, bot?, folder?, race?, split?}`.
fn new_job(hub: &mut Hub, st: &AppSettings, body: &Value) -> Reply {
    let text = body["text"].as_str().unwrap_or("").trim().to_string();
    let mode = match body["mode"].as_str() {
        Some(m) => match mode_of(m) {
            Some(m) => m,
            None => return err(400, format!("No kind of job called {m}.")),
        },
        None => Mode::Build,
    };
    let bot = match body["bot"].as_str().filter(|b| !b.trim().is_empty()) {
        Some(b) => match bot_of(b) {
            Some(who) => who,
            None => return err(400, format!("No bot called {b} on the crew.")),
        },
        None => Member::doer_for(mode).unwrap_or(Member::for_mode(mode)),
    };
    let text = if text.is_empty() {
        match mode.default_job() {
            Some(d) => d.to_string(),
            None => return err(400, "Say what the job is: {\"text\": \"...\"}."),
        }
    } else {
        text
    };
    let folder = match body["folder"]
        .as_str()
        .map(str::trim)
        .filter(|f| !f.is_empty())
    {
        Some(path) if Path::new(path).is_dir() => hub.rt.folder_for(path),
        Some(_) => return err(400, "That folder isn't on the server."),
        None => match hub.rt.store.folder().cloned() {
            Some(f) => f,
            None => {
                return err(
                    400,
                    "Add a folder first: POST /v1/folders {\"path\": \"...\"}.",
                )
            }
        },
    };
    if mode == Mode::Ask {
        hub.rt.ask(st, bot, text);
        return (202, json!({ "chat": bot.name() }));
    }
    let race = body["race"].as_u64().unwrap_or(0) as usize;
    // Sent from another computer through the hub: fetch the branch it
    // handed over, so the job starts from that exact commit.
    let handoff = match handoff_of(body) {
        Some(h) => match fetch_handoff(&folder.path, &h) {
            Ok(()) => Some(h),
            Err(why) => return err(409, why),
        },
        None => None,
    };
    let plain = !(body["split"].as_bool() == Some(true) && mode.writes()) && race <= 1;
    hub.rt.handoff = handoff.filter(|_| plain);
    let opened = if body["split"].as_bool() == Some(true) && mode.writes() {
        let id = hub.rt.begin_goal(st, text, folder);
        return (200, json!({ "goal": id }));
    } else if race > 1 && mode.writes() {
        hub.rt.start_race(st, bot, mode, text, folder, race)
    } else {
        hub.rt.start_job(st, bot, mode, text, folder)
    };
    hub.rt.handoff = None;
    match opened {
        Opened::Job(id) => (200, json!({ "job": id })),
        Opened::Race(id) => (200, json!({ "race": id })),
        // Next in line: it starts once the bot is free.
        Opened::Busy(id) => (202, json!({ "queued": true, "busy": id })),
        Opened::Waiting(id) => (202, json!({ "queued": true, "waiting": id })),
    }
}

/// The branch and commit a job sent from another computer starts from:
/// `{branch, base_sha, base}` in the body, when it has them.
fn handoff_of(body: &Value) -> Option<super::work::Handoff> {
    let field = |k: &str| body[k].as_str().unwrap_or("").trim().to_string();
    let h = super::work::Handoff {
        branch: field("branch"),
        base_sha: field("base_sha"),
        base: field("base"),
    };
    (!h.base_sha.is_empty()).then_some(h)
}

/// Brings the handed-over branch into the server's copy from its hub (the
/// `squad` remote), and checks the commit the job starts from is there.
fn fetch_handoff(folder: &str, h: &super::work::Handoff) -> Result<(), String> {
    let sha_ok = h.base_sha.len() >= 7 && h.base_sha.chars().all(|c| c.is_ascii_hexdigit());
    if !sha_ok {
        return Err("That isn't a commit.".into());
    }
    if super::work::handoff_branch_ok(&h.branch) {
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(folder)
            .args([
                "fetch",
                "--quiet",
                "squad",
                &format!("+refs/heads/{0}:refs/remotes/squad/{0}", h.branch),
            ])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .output();
    }
    match crate::git::rev(folder, &h.base_sha) {
        Some(_) => Ok(()),
        None => Err(format!(
            "The server doesn't have commit {} yet.",
            h.base_sha.chars().take(7).collect::<String>()
        )),
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(n) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------- the socket ----------

fn write_json(stream: &mut impl Write, (status, body): Reply) -> std::io::Result<()> {
    let text = body.to_string();
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

/// `GET /v1/events`: a stream of what changes, as server-sent events, with a
/// comment every 15 seconds so proxies and tunnels keep it open.
fn stream_events(mut stream: TcpStream, hub: &Mutex<Hub>) {
    let (tx, rx) = channel();
    let hello = {
        let mut h = hub.lock().unwrap_or_else(|e| e.into_inner());
        h.subs.push(tx);
        json!({ "kind": "hello", "running": h.rt.runners.len() }).to_string()
    };
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n";
    if write!(stream, "{head}data: {hello}\n\n").is_err() {
        return;
    }
    let _ = stream.flush();
    loop {
        let sent = match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(line) => write!(stream, "data: {line}\n\n"),
            Err(RecvTimeoutError::Timeout) => write!(stream, ": still here\n\n"),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if sent.and_then(|_| stream.flush()).is_err() {
            return;
        }
    }
}

fn handle(mut stream: TcpStream, base: &Path, hub: &Mutex<Hub>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let Some(http) = phone::read_request(&mut stream) else {
        let _ = write_json(&mut stream, err(400, "bad request"));
        return;
    };
    let path = http.path.split('?').next().unwrap_or("");
    if http.method == "GET" && path.trim_matches('/') == "v1/events" {
        if device_for(base, &http.auth).is_none() {
            let _ = write_json(&mut stream, err(401, "This device isn't paired."));
            return;
        }
        let _ = stream.set_read_timeout(None);
        stream_events(stream, hub);
        return;
    }
    let reply = respond(&http, base, hub);
    let _ = write_json(&mut stream, reply);
}

/// Where to listen: the address given, `tailscale` for this machine's
/// Tailscale address, or 127.0.0.1.
fn listen_address(given: Option<&str>) -> String {
    match given.map(str::trim).filter(|g| !g.is_empty()) {
        Some("tailscale") => {
            let ip = phone::addresses()
                .into_iter()
                .find(phone::is_tailscale)
                .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
            SocketAddr::new(ip, PORT).to_string()
        }
        Some(g) => match g.parse::<std::net::IpAddr>() {
            Ok(ip) => SocketAddr::new(ip, PORT).to_string(),
            Err(_) => g.to_string(),
        },
        None => format!("127.0.0.1:{PORT}"),
    }
}

/// `squad serve [--listen <address>]`: run the crew here until stopped.
pub fn serve(args: &[String]) -> i32 {
    let mut listen = None;
    let mut install_it = false;
    let mut set = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--listen" => listen = it.next().cloned(),
            "--install" => install_it = true,
            "--set" => set.extend(it.next().cloned()),
            other => {
                eprintln!("squad serve doesn't know {other}. Usage: squad serve [--listen <address>|tailscale] [--install] [--set KEY=value]");
                return 2;
            }
        }
    }
    for pair in &set {
        let code = set_env(pair);
        if code != 0 {
            return code;
        }
    }
    if install_it || !set.is_empty() {
        return install(listen.as_deref());
    }
    load_env();
    let base = crate::store::data_dir();
    let address = listen_address(listen.as_deref());
    let listener = match TcpListener::bind(&address) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("squad: could not listen on {address}: {e}");
            return 1;
        }
    };
    let bound = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or(address);
    let st = AppSettings::load();
    st.apply();
    crate::caps::refresh();
    let mut store = Store::load();
    // Jobs that were running when the server last stopped didn't finish.
    for j in store
        .jobs
        .iter_mut()
        .filter(|j| j.status == Status::Running)
    {
        j.status = Status::Interrupted;
        j.ended.get_or_insert_with(store::now);
    }
    let hub = Arc::new(Mutex::new(Hub::new(Runtime::new(store), st)));
    {
        let hub = hub.clone();
        std::thread::spawn(move || loop {
            hub.lock().unwrap_or_else(|e| e.into_inner()).tick();
            std::thread::sleep(TICK);
        });
    }
    println!("Squad Code is serving the crew on {bound}.");
    if devices(&base).is_empty() {
        let code = new_code(&base, "");
        println!();
        println!("Pair your first device with this code (it works once, for 10 minutes):");
        println!();
        println!("    {code}");
    } else {
        println!("Run `squad pair` here to pair another device.");
    }
    if bound.starts_with("127.") {
        println!();
        println!("Only this machine can reach it. From your computer, open a tunnel:");
        println!("    ssh -N -L {PORT}:127.0.0.1:{PORT} <you>@<this server>");
        println!("or run `squad serve --listen tailscale` to reach it over Tailscale.");
    }
    for stream in listener.incoming().flatten() {
        let (hub, base) = (hub.clone(), base.clone());
        std::thread::spawn(move || handle(stream, &base, &hub));
    }
    0
}

/// `squad serve --install`: keep the server running after you log out and
/// after the machine restarts, as a systemd user service. Where there's no
/// systemd it starts in the background instead, which lasts until a restart.
fn install(listen: Option<&str>) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("squad: can't tell where this program is: {e}");
            return 1;
        }
    };
    let mut cmd = format!("{} v2 serve", shell_quote(&exe.to_string_lossy()));
    if let Some(l) = listen {
        cmd.push_str(&format!(" --listen {}", shell_quote(l)));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let systemd = std::process::Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
        .is_ok_and(|o| o.status.success());
    if systemd {
        let dir = home.join(".config/systemd/user");
        let unit = format!(
            "[Unit]\nDescription=Squad Code crew server\nAfter=network-online.target\n\n[Service]\nExecStart={cmd}\nRestart=on-failure\nRestartSec=5\nEnvironment=PATH={}\n\n[Install]\nWantedBy=default.target\n",
            path_with_local_bin(&home)
        );
        if let Err(e) = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join("squad.service"), unit))
        {
            eprintln!("squad: couldn't write the service: {e}");
            return 1;
        }
        let ctl = |args: &[&str]| {
            std::process::Command::new("systemctl")
                .arg("--user")
                .args(args)
                .status()
                .is_ok_and(|s| s.success())
        };
        if !(ctl(&["daemon-reload"])
            && ctl(&["enable", "squad.service"])
            && ctl(&["restart", "squad.service"]))
        {
            eprintln!(
                "squad: systemd wouldn't start the service. `journalctl --user -u squad` says why."
            );
            return 1;
        }
        // Without lingering, user services stop when you log out.
        let user = std::env::var("USER").unwrap_or_default();
        let linger = std::process::Command::new("loginctl")
            .args(["enable-linger", &user])
            .status()
            .is_ok_and(|s| s.success())
            || std::process::Command::new("sudo")
                .args(["-n", "loginctl", "enable-linger", &user])
                .status()
                .is_ok_and(|s| s.success());
        println!("Squad Code is running as a service and starts again after a restart.");
        if !linger {
            println!(
                "It stops when you log out until someone runs: sudo loginctl enable-linger {user}"
            );
        }
        return 0;
    }
    // No systemd (a container, say): start it in the background, stopping
    // the one it started before.
    let pid_file = crate::store::data_dir().join("serve.pid");
    if let Ok(pid) = std::fs::read_to_string(&pid_file) {
        let pid = pid.trim();
        if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) {
            let _ = std::process::Command::new("kill").arg(pid).status();
            for _ in 0..20 {
                if TcpStream::connect(("127.0.0.1", PORT)).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    if TcpStream::connect(("127.0.0.1", PORT)).is_ok() {
        println!("Squad Code is already serving here, started some other way. Stop it and run this again to pick up new keys.");
        return 0;
    }
    let log = crate::store::data_dir().join("serve.log");
    let started = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "PATH={} nohup {cmd} >> {} 2>&1 & echo $! > {}",
            shell_quote(&path_with_local_bin(&home)),
            shell_quote(&log.to_string_lossy()),
            shell_quote(&pid_file.to_string_lossy())
        ))
        .status()
        .is_ok_and(|s| s.success());
    if started {
        println!("Squad Code is running in the background. There's no systemd here, so start it again after a restart: squad serve --install");
        0
    } else {
        eprintln!("squad: couldn't start the server in the background.");
        1
    }
}

/// PATH with ~/.local/bin first, where the installer puts Squad Code and the
/// agents, since a service doesn't get your login shell's PATH.
fn path_with_local_bin(home: &Path) -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    let mut dirs: Vec<String> = [
        ".local/bin",
        ".local/squadcode-node/bin",
        // Where builds from before the rename installed Node.
        ".local/runo-node/bin",
        ".opencode/bin",
        ".factory/bin",
        ".grok/bin",
    ]
    .iter()
    .map(|d| home.join(d).to_string_lossy().to_string())
    .filter(|d| !path.split(':').any(|p| p == d))
    .collect();
    if !path.is_empty() {
        dirs.push(path);
    }
    dirs.join(":")
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-:@".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// `squad pair [--bare] [name]`: a one-time code for a device to pair with.
/// `--bare` prints only the code, for the desktop app pairing over SSH.
pub fn pair_command(args: &[String]) -> i32 {
    let base = crate::store::data_dir();
    let bare = args.first().map(String::as_str) == Some("--bare");
    let name = args[usize::from(bare)..].join(" ");
    let code = new_code(&base, &name);
    if bare {
        println!("{code}");
        return 0;
    }
    println!("Pair a device with this code. It works once, for 10 minutes:");
    println!();
    println!("    {code}");
    0
}

/// `squad devices`: who is paired.
pub fn devices_command() -> i32 {
    let list = devices(&crate::store::data_dir());
    if list.is_empty() {
        println!("No devices are paired. Run `squad pair` to pair one.");
    }
    for d in list {
        println!("{}", d.name);
    }
    0
}

/// `squad unpair <name>`: revoke a device.
pub fn unpair_command(args: &[String]) -> i32 {
    let name = args.join(" ");
    if unpair(&crate::store::data_dir(), &name) {
        println!("{name} can no longer reach the crew.");
        0
    } else {
        eprintln!("No device called {name}. `squad devices` lists them.");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("squad-server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn hub() -> Mutex<Hub> {
        let store = Store {
            demo: true,
            ..Default::default()
        };
        // No agent can start, so a test never runs a real one.
        let mut st = AppSettings::default();
        for p in crate::agent::PROVIDERS.iter() {
            let mut a = st.agent(p.id);
            a.binary = "/nonexistent/squad-test-agent".into();
            st.agents.insert(p.id.to_string(), a);
        }
        Mutex::new(Hub::new(Runtime::new(store), st))
    }

    fn call(
        base: &Path,
        hub: &Mutex<Hub>,
        method: &str,
        path: &str,
        token: &str,
        body: Value,
    ) -> Reply {
        let http = Http {
            method: method.into(),
            path: path.into(),
            auth: if token.is_empty() {
                String::new()
            } else {
                format!("Bearer {token}")
            },
            if_none_match: String::new(),
            body: if body.is_null() {
                Vec::new()
            } else {
                body.to_string().into_bytes()
            },
        };
        respond(&http, base, hub)
    }

    #[test]
    fn a_code_pairs_once_and_the_token_opens_the_api() {
        let dir = base("pair");
        let hub = hub();
        assert_eq!(call(&dir, &hub, "GET", "/v1/hello", "", Value::Null).0, 200);
        assert_eq!(call(&dir, &hub, "GET", "/v1/state", "", Value::Null).0, 401);
        let code = new_code(&dir, "Laptop");
        assert_eq!(code.len(), 8);
        let (status, body) = call(
            &dir,
            &hub,
            "POST",
            "/v1/pair",
            "",
            json!({ "code": code.to_lowercase() }),
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["name"], "Laptop");
        let token = body["token"].as_str().unwrap().to_string();
        // The same code doesn't work twice.
        assert_eq!(
            call(&dir, &hub, "POST", "/v1/pair", "", json!({ "code": code })).0,
            401
        );
        let (status, state) = call(&dir, &hub, "GET", "/v1/state", &token, Value::Null);
        assert_eq!(status, 200);
        assert!(state["jobs"].as_array().unwrap().is_empty());
        // Revoked, it's refused.
        assert!(unpair(&dir, "Laptop"));
        assert_eq!(
            call(&dir, &hub, "GET", "/v1/state", &token, Value::Null).0,
            401
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_triggers_link_needs_its_secret_not_a_paired_device() {
        let dir = base("hooks");
        let hub = hub();
        let code = new_code(&dir, "Laptop");
        let token = call(&dir, &hub, "POST", "/v1/pair", "", json!({ "code": code })).1["token"]
            .as_str()
            .unwrap()
            .to_string();
        // A paired device makes the trigger, in a folder on the server.
        let (status, t) = call(
            &dir,
            &hub,
            "POST",
            "/v1/triggers",
            &token,
            json!({
                "name": "Deploy broke",
                "when": { "kind": "link", "token": "" },
                "text": "Find out why the deploy broke.",
                "bot": "Fixer",
                "mode": "Fix",
                "dir": dir.to_string_lossy(),
            }),
        );
        assert_eq!(status, 200, "{t}");
        let secret = t["when"]["token"].as_str().unwrap().to_string();
        assert_eq!(secret.len(), 32);
        let hook = format!("/v1/hooks/{secret}");
        assert_eq!(call(&dir, &hub, "GET", &hook, "", Value::Null).0, 405);
        assert_eq!(
            call(
                &dir,
                &hub,
                "POST",
                "/v1/hooks/wrong",
                "",
                json!({ "text": "x" })
            )
            .0,
            404
        );
        // Plain text works as well as JSON, with no device token.
        let http = Http {
            method: "POST".into(),
            path: hook.clone(),
            body: b"502 on /cart".to_vec(),
            ..Default::default()
        };
        let (status, reply) = respond(&http, &dir, &hub);
        assert_eq!(status, 202, "{reply}");
        assert_eq!(reply["trigger"], "Deploy broke");
        {
            let h = hub.lock().unwrap();
            let l = h.rt.store.later.last().expect("its job waits to start");
            assert_eq!(l.bot, Member::Fixer);
            assert!(l.text.contains("> 502 on /cart"));
        }
        let id = t["id"].as_u64().unwrap();
        let off = format!("/v1/triggers/{id}/off");
        assert_eq!(call(&dir, &hub, "POST", &off, &token, Value::Null).0, 200);
        assert_eq!(
            call(&dir, &hub, "POST", &hook, "", json!({ "text": "x" })).0,
            409
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_codes_wear_the_code_out() {
        let dir = base("misses");
        let hub = hub();
        let code = new_code(&dir, "");
        for _ in 0..CODE_TRIES {
            assert_eq!(
                call(
                    &dir,
                    &hub,
                    "POST",
                    "/v1/pair",
                    "",
                    json!({ "code": "WRONG123" })
                )
                .0,
                401
            );
        }
        assert_eq!(
            call(&dir, &hub, "POST", "/v1/pair", "", json!({ "code": code })).0,
            401
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_job_over_the_api_lands_in_the_store_and_is_announced() {
        let dir = base("job");
        let hub = hub();
        let code = new_code(&dir, "Phone");
        let token = call(&dir, &hub, "POST", "/v1/pair", "", json!({ "code": code })).1["token"]
            .as_str()
            .unwrap()
            .to_string();
        let (rx_tx, rx) = channel();
        hub.lock().unwrap().subs.push(rx_tx);
        // No folder yet: it says how to add one.
        let (status, _) = call(
            &dir,
            &hub,
            "POST",
            "/v1/jobs",
            &token,
            json!({ "text": "Read it" }),
        );
        assert_eq!(status, 400);
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().into_owned();
        assert_eq!(
            call(
                &dir,
                &hub,
                "POST",
                "/v1/folders",
                &token,
                json!({ "path": path })
            )
            .0,
            200
        );
        let (status, body) = call(
            &dir,
            &hub,
            "POST",
            "/v1/jobs",
            &token,
            json!({ "text": "Look around", "mode": "review" }),
        );
        assert_eq!(status, 200, "{body}");
        let id = body["job"].as_u64().unwrap();
        let (status, job) = call(
            &dir,
            &hub,
            "GET",
            &format!("/v1/jobs/{id}"),
            &token,
            Value::Null,
        );
        assert_eq!(status, 200);
        assert_eq!(job["title"], "Look around");
        let events: Vec<Value> = rx
            .try_iter()
            .map(|l: String| serde_json::from_str(&l).unwrap())
            .collect();
        assert!(
            events.iter().any(|e| e["kind"] == "job" && e["id"] == id),
            "{events:?}"
        );
        assert!(events.iter().any(|e| e["kind"] == "folders"), "{events:?}");
        assert_eq!(
            call(
                &dir,
                &hub,
                "POST",
                "/v1/jobs/999/approve",
                &token,
                Value::Null
            )
            .0,
            404
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn listen_addresses_read_plainly() {
        assert_eq!(listen_address(None), "127.0.0.1:7640");
        assert_eq!(listen_address(Some("0.0.0.0")), "0.0.0.0:7640");
        assert_eq!(listen_address(Some("10.0.0.2:9000")), "10.0.0.2:9000");
    }

    #[test]
    fn names_in_paths_are_decoded() {
        assert_eq!(decode("My%20Laptop"), "My Laptop");
        assert_eq!(decode("Phone+2"), "Phone 2");
        assert_eq!(decode("plain"), "plain");
    }
}
