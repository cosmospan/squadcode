//! The desktop app's side of `squad serve`: your own server, reached over SSH.
//!
//! You give it one thing, `you@your-server`, and [`connect`] does the rest
//! with your SSH key: it puts Squad Code on the server and keeps it running
//! as a service (scripts/install-server.sh), asks it for a pairing code, opens
//! a tunnel to it and trades the code for this computer's own token. The
//! server only ever listens on its own 127.0.0.1, so nothing is open to the
//! internet; everything goes through the tunnel.
//!
//! Once connected, a [`Link`] keeps the tunnel up on a thread of its own and
//! reads the server's state every few seconds, so the intelligence layer can
//! place jobs there ([`Link::server`]). A job placed there is sent with
//! [`Link::send`], and its copy in this app is kept up to date from the
//! server's ([`Link::take_jobs`]) until it's done. Nothing here blocks the
//! window: every call to the server happens on the link's thread.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::brain;
use super::server::PORT;
use super::store;

/// What runs on the server to install it and keep it running.
const SETUP: &str = include_str!("../../../../scripts/install-server.sh");
/// What locks the server down once the app's key works: key-only SSH, a
/// firewall that lets in nothing but SSH, fail2ban and automatic security
/// updates. It needs root or sudo without a password, and skips what it can't do.
const HARDEN: &str = include_str!("../../../../scripts/harden-server.sh");
/// How often the server's state is read while connected.
const REFRESH: Duration = Duration::from_secs(4);
/// How long to wait before opening a tunnel again after it dropped.
const RETRY: Duration = Duration::from_secs(15);

/// This computer's pairing with a server, kept in `remote.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// What you typed: `you@host`, or a name from ~/.ssh/config.
    pub host: String,
    /// The path of SquadCode on the server.
    pub bin: String,
    /// This computer's name there, and its token.
    pub name: String,
    pub token: String,
    /// The SSH key the app made for this server when you connected with a
    /// password; empty when your own SSH setup reaches it.
    pub key: String,
    /// Where jobs run: "" lets Squad pick, "server" or "here" is the person's choice.
    pub run: String,
}

impl Config {
    /// The server's name for people: the host without the user.
    pub fn label(&self) -> String {
        label_of(&self.host)
    }
}

fn label_of(host: &str) -> String {
    host.rsplit('@').next().unwrap_or(host).to_string()
}

fn path() -> PathBuf {
    crate::store::data_dir().join("remote.json")
}

pub fn load() -> Option<Config> {
    let text = std::fs::read_to_string(path()).ok()?;
    serde_json::from_str::<Config>(&text)
        .ok()
        .filter(|c| !c.host.is_empty() && !c.token.is_empty())
}

fn save(c: &Config) -> Result<(), String> {
    let text = serde_json::to_string_pretty(c).map_err(|e| e.to_string())?;
    let p = path();
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Keeps where jobs run ("", "server" or "here") for next time.
pub fn set_run(run: &str) {
    if let Some(mut c) = load() {
        c.run = run.to_string();
        let _ = save(&c);
    }
}

/// Forgets the server here. The server keeps running; `squad unpair` there
/// revokes this computer's token.
pub fn forget() {
    let _ = std::fs::remove_file(path());
}

/// Whether `host` reads as something ssh can be given: `user@host`, `host`,
/// or a name from ~/.ssh/config, with no spaces or options.
pub fn valid_host(host: &str) -> bool {
    let h = host.trim();
    !h.is_empty()
        && !h.starts_with('-')
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || "@.-_:[]".contains(c))
}

// ---------- ssh ----------

/// The servers' host keys Squad trusts, kept apart from ~/.ssh/known_hosts:
/// a key the person's own SSH saved long ago for a reused address can't get
/// in the way, and a server's key is trusted the first time Squad meets it
/// and only that key after.
fn known_hosts() -> PathBuf {
    let dir = crate::store::data_dir().join("ssh");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("known_hosts")
}

/// `user@host` without the user, as known_hosts names it.
fn host_name(host: &str) -> String {
    label_of(host)
        .trim_matches(|c| c == '[' || c == ']')
        .to_string()
}

/// ssh with Squad's own known_hosts and trust on first use.
fn ssh_command() -> Command {
    let mut cmd = Command::new("ssh");
    cmd.arg("-o")
        .arg(format!("UserKnownHostsFile={}", known_hosts().display()))
        .args(["-o", "StrictHostKeyChecking=accept-new"]);
    cmd
}

fn ssh_base(host: &str, key: &str) -> Command {
    let mut cmd = ssh_command();
    if !key.is_empty() {
        cmd.args(["-i", key, "-o", "IdentitiesOnly=yes"]);
    }
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]);
    cmd.arg(host);
    cmd
}

/// Whether ssh refused because the server's key isn't the one Squad trusts.
fn key_refused(stderr: &str) -> bool {
    stderr.contains("Host key verification failed") || stderr.contains("IDENTIFICATION HAS CHANGED")
}

/// The server's key Squad trusts, as `SHA256:…`, once it has met it.
pub fn trusted_fingerprint(host: &str) -> Option<String> {
    let out = Command::new("ssh-keygen")
        .args(["-l", "-F", &host_name(host), "-f"])
        .arg(known_hosts())
        .stdin(Stdio::null())
        .output()
        .ok()?;
    fingerprint_in(&String::from_utf8_lossy(&out.stdout))
}

/// The key the server shows now, as `SHA256:…`.
fn offered_fingerprint(host: &str) -> Option<String> {
    let scan = Command::new("ssh-keyscan")
        .args(["-T", "10", "-t", "ed25519,ecdsa,rsa", &host_name(host)])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let mut lines = String::from_utf8_lossy(&scan.stdout).to_string();
    // The strongest key first, the one ssh itself prefers.
    let mut keys: Vec<&str> = lines.lines().filter(|l| !l.starts_with('#')).collect();
    keys.sort_by_key(|l| !l.contains("ed25519"));
    lines = keys.first()?.to_string();
    let mut child = Command::new("ssh-keygen")
        .args(["-l", "-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(lines.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    fingerprint_in(&String::from_utf8_lossy(&out.stdout))
}

fn fingerprint_in(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with("SHA256:"))
        .map(str::to_string)
}

/// Forgets the key Squad trusted for `host`, after you said the server was
/// rebuilt; the next connection trusts the key it shows then.
pub fn trust_new_key(host: &str) {
    let _ = Command::new("ssh-keygen")
        .args(["-R", &host_name(host), "-f"])
        .arg(known_hosts())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Meets the server before logging in: trusts its key the first time, and
/// says so when it isn't the key trusted before.
fn meet(host: &str) -> Result<(), Fail> {
    let out = ssh_command()
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "PreferredAuthentications=none",
            host,
            "true",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                Fail::from("ssh isn't installed on this computer".to_string())
            }
            _ => Fail::from(e.to_string()),
        })?;
    let err = String::from_utf8_lossy(&out.stderr);
    if key_refused(&err) {
        return Err(Fail::KeyChanged(
            offered_fingerprint(host).unwrap_or_default(),
        ));
    }
    if err.contains("Could not resolve")
        || err.contains("timed out")
        || err.contains("refused")
        || err.contains("No route")
    {
        return Err(Fail::from(format!(
            "Couldn't reach {host}: {}",
            err.lines()
                .rev()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("no answer")
        )));
    }
    Ok(())
}

/// Runs `command` on the server, with `input` on its stdin, and gives back
/// what it printed.
fn ssh(host: &str, key: &str, command: &str, input: Option<&str>) -> Result<String, String> {
    let mut cmd = ssh_base(host, key);
    cmd.arg(command)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "ssh isn't installed on this computer".to_string(),
        _ => e.to_string(),
    })?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(text.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The script's own `error:` line says it best; else ssh's last line.
    let why = stderr
        .lines()
        .find_map(|l| l.strip_prefix("error: "))
        .map(str::to_string)
        .or_else(|| {
            stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_else(|| format!("it stopped with {}", out.status));
    Err(why)
}

/// The key the app uses for servers you connect with a password, made once.
fn own_key() -> Result<String, String> {
    let dir = crate::store::data_dir().join("ssh");
    let key = dir.join("id_ed25519");
    if key.is_file() {
        return Ok(key.to_string_lossy().to_string());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let made = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "squad-code", "-f"])
        .arg(&key)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ssh-keygen isn't available: {e}"))?;
    if !made.status.success() {
        return Err(format!(
            "Couldn't make an SSH key: {}",
            String::from_utf8_lossy(&made.stderr).trim()
        ));
    }
    Ok(key.to_string_lossy().to_string())
}

/// ssh that logs in with `password`: ssh asks this same program for it (see
/// `main`), so it's never typed into a terminal or written to disk.
fn password_ssh(password: &str) -> Result<Command, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = ssh_command();
    cmd.args([
        "-o",
        "ConnectTimeout=15",
        "-o",
        "PubkeyAuthentication=no",
        "-o",
        "PreferredAuthentications=password,keyboard-interactive",
        "-o",
        "NumberOfPasswordPrompts=1",
    ])
    .env("SSH_ASKPASS", exe)
    .env("SSH_ASKPASS_REQUIRE", "force")
    .env("SQUAD_ASKPASS", password)
    .env(
        "DISPLAY",
        std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()),
    );
    Ok(cmd)
}

/// The server wants a new password before it lets anyone in (many VPS
/// providers do this on the first login).
fn wants_new_password(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("password change required")
        || s.contains("required to change your password")
        || s.contains("password has expired")
}

/// What a password prompt asks for: the old password, or the new one.
fn prompt_wants_new(prompt: &str) -> Option<bool> {
    let p = prompt.trim().to_lowercase();
    if !p.ends_with(':') {
        return None;
    }
    if p.contains("new") {
        Some(true)
    } else if p.contains("current") || p.contains("old") || p.contains("password") {
        Some(false)
    } else {
        None
    }
}

/// Logs in with the old password in a terminal, which is the only way the
/// server takes a password change, and answers its prompts.
fn change_password(host: &str, old: &str, new: &str) -> Result<(), String> {
    let mut cmd = password_ssh(old)?;
    cmd.args(["-tt", host, "true"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no terminal")?;
    let (out_tx, out_rx) = channel::<String>();
    for mut pipe in [
        Box::new(child.stdout.take().ok_or("no terminal")?) as Box<dyn Read + Send>,
        Box::new(child.stderr.take().ok_or("no terminal")?),
    ] {
        let out_tx = out_tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0
                    || out_tx
                        .send(String::from_utf8_lossy(&buf[..n]).to_string())
                        .is_err()
                {
                    break;
                }
            }
        });
    }
    drop(out_tx);
    let mut seen = String::new();
    let mut answered = 0usize;
    let mut done_at = 0usize;
    let until = Instant::now() + Duration::from_secs(60);
    let result = loop {
        if Instant::now() > until {
            break Err("The server didn't finish changing the password.".to_string());
        }
        let ended = match out_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                seen.push_str(&chunk);
                false
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => true,
        };
        let plain = plain_text(&seen);
        let low = plain.to_lowercase();
        if low.contains("updated successfully") || low.contains("password changed") {
            break Ok(());
        }
        // Each prompt is answered once; a fourth means it turned an answer down.
        let fresh = &plain[done_at.min(plain.len())..];
        if let Some(wants_new) = fresh.lines().last().and_then(prompt_wants_new) {
            if answered >= 3 {
                break Err(last_said(&plain));
            }
            let answer = if wants_new { new } else { old };
            let _ = stdin.write_all(format!("{answer}\n").as_bytes());
            let _ = stdin.flush();
            answered += 1;
            done_at = plain.len();
        }
        if ended {
            break Err(last_said(&plain));
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    result.map_err(|why| format!("The server didn't take the new password: {why}"))
}

/// The last thing a terminal said that isn't a prompt.
fn last_said(plain: &str) -> String {
    plain
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| {
            !l.is_empty()
                && prompt_wants_new(l).is_none()
                && !l.starts_with("Connection to")
                && !l.to_lowercase().contains("required to change")
        })
        .unwrap_or("no reason given")
        .to_string()
}

/// Logs in once with the password and adds the app's key to the server.
fn add_key(host: &str, password: &str, key: &str) -> Result<(), Fail> {
    let public = std::fs::read_to_string(format!("{key}.pub"))
        .map_err(|e| format!("Couldn't read the new key: {e}"))?;
    let mut cmd = password_ssh(password)?;
    cmd.args([
        host,
        "umask 077; mkdir -p ~/.ssh && touch ~/.ssh/authorized_keys && k=$(cat) && (grep -qxF \"$k\" ~/.ssh/authorized_keys || printf '%s\\n' \"$k\" >> ~/.ssh/authorized_keys)",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "ssh isn't installed on this computer".to_string(),
        _ => e.to_string(),
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(public.trim().as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    if wants_new_password(&err) {
        return Err(Fail::NewPassword);
    }
    if err.contains("Permission denied") {
        return Err(format!("{host} didn't take that password. Check it, and the user name before the @ (root, or ubuntu on many Ubuntu servers).").into());
    }
    Err(format!(
        "Couldn't log in to {host}: {}",
        err.lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ssh stopped")
            .trim()
    )
    .into())
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A port nothing on this computer is using.
fn free_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .ok()
}

/// `ssh -N -L`: the server's 127.0.0.1:7640 at a port on this computer.
struct Tunnel {
    child: Child,
    port: u16,
}

impl Tunnel {
    fn open(host: &str, key: &str) -> Result<Tunnel, String> {
        Tunnel::to(host, key, PORT)
    }

    /// A tunnel to `there`, a port on the server's own 127.0.0.1.
    fn to(host: &str, key: &str, there: u16) -> Result<Tunnel, String> {
        let port = free_port().ok_or("no free port on this computer")?;
        let mut cmd = ssh_base(host, key);
        cmd.args([
            "-N",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=20",
            "-o",
            "ServerAliveCountMax=3",
            "-L",
            &format!("127.0.0.1:{port}:127.0.0.1:{there}"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until {
            if let Ok(Some(_)) = child.try_wait() {
                let mut why = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut why);
                }
                let why = why
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("ssh stopped")
                    .trim()
                    .to_string();
                return Err(why);
            }
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                // ssh is listening; stop reading its errors so it never blocks on them.
                drop(child.stderr.take());
                return Ok(Tunnel { child, port });
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        let _ = child.kill();
        Err("the tunnel didn't open in time".into())
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------- http ----------

/// One request to the server through the tunnel.
fn http(
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let auth = if token.is_empty() {
        String::new()
    } else {
        format!("Authorization: Bearer {token}\r\n")
    };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    parse_reply(&raw)
}

fn parse_reply(raw: &[u8]) -> Result<(u16, Value), String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or("the server's answer was cut off")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("the server's answer made no sense")?;
    let value = if body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(body).map_err(|_| "the server's answer wasn't JSON".to_string())?
    };
    Ok((status, value))
}

fn error_of(v: &Value) -> String {
    v["error"]
        .as_str()
        .unwrap_or("the server said no")
        .to_string()
}

// ---------- connecting ----------

/// What [`connect`] reports as it goes.
#[derive(Clone, Debug)]
pub enum Step {
    Say(String),
    Done(Config),
    Failed(String),
    /// The server shows a different key from the one Squad trusted: rebuilt,
    /// or not the same server. Its key now, as `SHA256:…` ("" if unknown).
    KeyChanged(String),
    /// The server wants its password changed before anyone logs in.
    NewPassword,
}

/// Why connecting stopped.
#[derive(Debug)]
enum Fail {
    KeyChanged(String),
    NewPassword,
    Other(String),
}

impl From<String> for Fail {
    fn from(s: String) -> Self {
        Fail::Other(s)
    }
}

impl From<&str> for Fail {
    fn from(s: &str) -> Self {
        Fail::Other(s.to_string())
    }
}

/// Sets the server up and pairs this computer with it, on a thread of its own.
/// `password` is the server's, for a first connection with no SSH key: the
/// app makes a key of its own, puts it on the server and uses it from then on.
/// `new_password` is set when the server asked for its password to be changed
/// first: it's changed to that, and that's the password from then on.
pub fn connect(
    host: String,
    password: String,
    new_password: String,
    device: String,
) -> Receiver<Step> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let step = match setup(&host, &password, &new_password, &device, &tx) {
            Ok(c) => Step::Done(c),
            Err(Fail::Other(why)) => Step::Failed(why),
            Err(Fail::KeyChanged(fp)) => Step::KeyChanged(fp),
            Err(Fail::NewPassword) => Step::NewPassword,
        };
        let _ = tx.send(step);
    });
    rx
}

fn setup(
    host: &str,
    password: &str,
    new_password: &str,
    device: &str,
    tx: &Sender<Step>,
) -> Result<Config, Fail> {
    let say = |t: String| {
        let _ = tx.send(Step::Say(t));
    };
    let host = host.trim();
    if !valid_host(host) {
        return Err("Type it the way you'd type it after ssh, like you@203.0.113.7.".into());
    }
    say(format!("Reaching {host}…"));
    let first = trusted_fingerprint(host).is_none();
    meet(host)?;
    if let Some(fp) = trusted_fingerprint(host) {
        say(if first {
            format!("Trusting {}'s key from now on: {fp}.", host_name(host))
        } else {
            format!("It's the same server as before ({fp}).")
        });
    }
    let password = if new_password.is_empty() || password.is_empty() {
        password
    } else {
        say("Changing the server's password, as it asks…".into());
        change_password(host, password, new_password)?;
        say("Changed it. Use the new password if you ever log in yourself.".into());
        new_password
    };
    let key = if password.is_empty() {
        String::new()
    } else {
        let key = own_key()?;
        say("Putting this computer's key on the server…".into());
        add_key(host, password, &key)?;
        key
    };
    let os = ssh(host, &key, "uname -sm", None).map_err(|why| -> Fail {
        if password.is_empty() {
            format!("Couldn't reach {host}: {why}. If you don't use SSH keys, type the server's password too; your VPS provider emailed it to you.")
        } else {
            format!("Couldn't reach {host} with the new key: {why}")
        }
        .into()
    })?;
    if !os.trim_start().starts_with("Linux") {
        return Err(format!("{host} runs {}; a Squad server needs Linux.", os.trim()).into());
    }
    say("Installing Squad Code there and keeping it running…".into());
    let out = ssh(host, &key, "sh -s", Some(SETUP))
        .map_err(|why| format!("Setting up {host} stopped: {why}"))?;
    let bin = out
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix("bin="))
        .map(|b| b.trim().to_string())
        .ok_or_else(|| format!("Setting up {host} didn't finish."))?;
    say("Locking the server down…".into());
    // A user whose sudo asks for a password (ubuntu on many VPSes) gets it
    // from the first line of stdin, so it never shows on a command line.
    let harden = if password.is_empty() {
        ssh(host, &key, "sh -s", Some(HARDEN))
    } else {
        ssh(
            host,
            &key,
            "IFS= read -r SQUAD_SUDO_PW; export SQUAD_SUDO_PW; sh -s",
            Some(&format!("{password}\n{HARDEN}")),
        )
    };
    match harden {
        Ok(out) => {
            for line in out.lines().filter_map(|l| l.strip_prefix("did: ")) {
                say(format!("Done: {line}."));
            }
        }
        Err(why) => say(format!(
            "Couldn't lock it down ({why}). It still only listens on its own address."
        )),
    }
    say("Pairing this computer…".into());
    let code = ssh(
        host,
        &key,
        &format!("{} v2 pair --bare {}", quote(&bin), quote(device)),
        None,
    )
    .map_err(|why| format!("Couldn't get a pairing code: {why}"))?
    .trim()
    .to_string();
    let tunnel =
        Tunnel::open(host, &key).map_err(|why| format!("Couldn't open a tunnel: {why}"))?;
    // The service may still be starting.
    let mut last = String::new();
    for _ in 0..20 {
        match http(tunnel.port, "GET", "/v1/hello", "", None) {
            Ok((200, _)) => {
                last.clear();
                break;
            }
            Ok((_, v)) => last = error_of(&v),
            Err(e) => last = e,
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if !last.is_empty() {
        return Err(format!("Squad Code isn't answering on {host}: {last}").into());
    }
    let (status, v) = http(
        tunnel.port,
        "POST",
        "/v1/pair",
        "",
        Some(&json!({ "code": code, "name": device })),
    )
    .map_err(|e| format!("Pairing failed: {e}"))?;
    if status != 200 {
        return Err(format!("Pairing failed: {}", error_of(&v)).into());
    }
    let cfg = Config {
        host: host.to_string(),
        bin,
        name: v["name"].as_str().unwrap_or(device).to_string(),
        token: v["token"].as_str().unwrap_or("").to_string(),
        key,
        run: String::new(),
    };
    if cfg.token.is_empty() {
        return Err("Pairing failed: the server sent no token.".into());
    }
    save(&cfg).map_err(|e| format!("Couldn't keep the pairing: {e}"))?;
    Ok(cfg)
}

/// This computer's name, for the server's list of devices.
pub fn device_name() -> String {
    let name = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| {
            Command::new("hostname")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_default();
    let name = name.trim().trim_end_matches(".local").to_string();
    if name.is_empty() {
        "desktop".into()
    } else {
        name
    }
}

// ---------- the live link ----------

/// A project on the server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub name: String,
    pub path: String,
    pub remote: String,
}

/// An agent on the server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentThere {
    pub agent: String,
    pub name: String,
    pub installed: bool,
    pub signed_in: bool,
}

/// What the link last saw.
#[derive(Clone, Debug, Default)]
struct Seen {
    /// One line for Settings: "Connected", or why not.
    line: String,
    up: bool,
    engines: Vec<String>,
    projects: Vec<Project>,
    /// Each agent there: installed, and signed in or given a key.
    logins: Vec<AgentThere>,
    /// Jobs read since the app last took them, by their id on the server.
    jobs: HashMap<u64, store::Job>,
    /// Answers to [`Link::send`]: the local id, and the server's id or why not.
    sent: Vec<(u64, Result<u64, String>)>,
    /// Notes for the person.
    notes: Vec<String>,
    /// Notes for one job's feed, by its local id.
    job_notes: Vec<(u64, String)>,
}

enum Ask {
    /// A job (its local id), and its project here to send the latest
    /// commits from ("" for none).
    Send(u64, Value, String),
    Act(u64, String, Value),
    Watch(Vec<u64>),
    /// Copies a project from this computer (name, path here), then starts
    /// a job on it there when one is given.
    Copy(String, String, Option<(u64, Value)>),
}

/// A connection to the server, kept up on its own thread.
pub struct Link {
    pub cfg: Config,
    seen: Arc<Mutex<Seen>>,
    tx: Sender<Ask>,
}

impl Link {
    pub fn start(cfg: Config) -> Link {
        let seen = Arc::new(Mutex::new(Seen {
            line: format!("Connecting to {}…", cfg.label()),
            ..Default::default()
        }));
        let (tx, rx) = channel();
        {
            let (cfg, seen) = (cfg.clone(), seen.clone());
            std::thread::spawn(move || worker(cfg, seen, rx));
        }
        Link { cfg, seen, tx }
    }

    fn seen(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The server as the intelligence layer sees it.
    pub fn server(&self) -> brain::place::Server {
        let s = self.seen();
        brain::place::Server {
            name: self.cfg.label(),
            reachable: s.up,
            // Agents that can actually run there: installed and signed in. An
            // older server doesn't say who's signed in, so its installed ones count.
            engines: if s.logins.is_empty() {
                s.engines.clone()
            } else {
                s.logins
                    .iter()
                    .filter(|l| l.installed && l.signed_in)
                    .map(|l| l.agent.clone())
                    .collect()
            },
            projects: s
                .projects
                .iter()
                .map(|p| {
                    if p.remote.is_empty() {
                        p.name.clone()
                    } else {
                        p.remote.clone()
                    }
                })
                .collect(),
        }
    }

    pub fn up(&self) -> bool {
        self.seen().up
    }

    pub fn logins(&self) -> Vec<AgentThere> {
        self.seen().logins.clone()
    }

    /// One line on how the connection is.
    pub fn line(&self) -> String {
        self.seen().line.clone()
    }

    pub fn projects(&self) -> Vec<Project> {
        self.seen().projects.clone()
    }

    /// Where `folder` is on the server, matched by its git remote, or by its
    /// name when it has none.
    pub fn path_for(&self, folder: &store::Folder, remote: &str) -> Option<String> {
        self.seen()
            .projects
            .iter()
            .find(|p| {
                if remote.is_empty() || p.remote.is_empty() {
                    p.name.eq_ignore_ascii_case(&folder.name)
                } else {
                    brain::place::same_project(&p.remote, remote)
                }
            })
            .map(|p| p.path.clone())
    }

    /// Starts a job there; [`Link::sent`] gives back its id there.
    /// `here` is the project on this computer, whose latest commits go first.
    pub fn send(&self, local: u64, body: Value, here: &str) {
        let _ = self.tx.send(Ask::Send(local, body, here.to_string()));
    }

    /// `POST /v1/jobs/{id}/{action}` there.
    pub fn act(&self, remote: u64, action: &str, body: Value) {
        let _ = self.tx.send(Ask::Act(remote, action.to_string(), body));
    }

    /// The jobs there to keep reading.
    pub fn watch(&self, remote: Vec<u64>) {
        let _ = self.tx.send(Ask::Watch(remote));
    }

    /// Clones `url` into ~/squad/<name> there and adds it as a project.
    /// Copies the project at `path` here to the server.
    pub fn copy_project(&self, name: &str, path: &str) {
        let _ = self
            .tx
            .send(Ask::Copy(name.to_string(), path.to_string(), None));
    }

    /// Copies the project to the server, then starts the job on it there.
    pub fn copy_then_send(&self, local: u64, folder: &store::Folder, body: Value) {
        let _ = self.tx.send(Ask::Copy(
            folder.name.clone(),
            folder.path.clone(),
            Some((local, body)),
        ));
    }

    pub fn take_job_notes(&self) -> Vec<(u64, String)> {
        std::mem::take(&mut self.seen().job_notes)
    }

    pub fn sent(&self) -> Vec<(u64, Result<u64, String>)> {
        std::mem::take(&mut self.seen().sent)
    }

    pub fn take_jobs(&self) -> HashMap<u64, store::Job> {
        std::mem::take(&mut self.seen().jobs)
    }

    pub fn take_notes(&self) -> Vec<String> {
        std::mem::take(&mut self.seen().notes)
    }
}

fn worker(cfg: Config, seen: Arc<Mutex<Seen>>, rx: Receiver<Ask>) {
    let label = cfg.label();
    let mut tunnel: Option<Tunnel> = None;
    let mut tried: Option<Instant> = None;
    let mut watching: Vec<u64> = Vec::new();
    let mut read_at = Instant::now() - REFRESH;
    let lock = || seen.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let ask = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(a) => Some(a),
            Err(RecvTimeoutError::Timeout) => None,
            // The app let go of the link: close the tunnel and stop.
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if let Some(Ask::Watch(ids)) = &ask {
            watching = ids.clone();
        }
        // Keep the tunnel up, trying again now and then when it's down.
        if tunnel.as_mut().is_some_and(|t| !t.alive()) {
            tunnel = None;
        }
        if tunnel.is_none() && tried.is_none_or(|t| t.elapsed() > RETRY) {
            tried = Some(Instant::now());
            match Tunnel::open(&cfg.host, &cfg.key) {
                Ok(t) => {
                    tunnel = Some(t);
                    read_at = Instant::now() - REFRESH;
                }
                Err(why) => {
                    let mut s = lock();
                    s.up = false;
                    s.line = if key_refused(&why) {
                        format!("{label} isn't showing the key Squad trusted for it. If you rebuilt it, disconnect and connect again.")
                    } else {
                        format!("Can't reach {label}: {why}")
                    };
                }
            }
        }
        let Some(port) = tunnel.as_ref().map(|t| t.port) else {
            if let Some(Ask::Send(local, ..) | Ask::Copy(_, _, Some((local, _)))) = ask {
                lock()
                    .sent
                    .push((local, Err(format!("{label} isn't reachable right now"))));
            }
            continue;
        };
        let call = |method: &str, path: &str, body: Option<&Value>| {
            http(port, method, path, &cfg.token, body)
        };
        match ask {
            Some(Ask::Copy(name, path, then)) => {
                let copied = copy_project(&cfg, port, &name, &path);
                match (copied, then) {
                    (Ok(there), None) => lock().notes.push(format!(
                        "{name} is on {label} now, in {there}. Jobs on it can run there."
                    )),
                    (Err(why), None) => lock().notes.push(why),
                    (Err(why), Some((local, _))) => lock().sent.push((local, Err(why))),
                    (Ok(there), Some((local, mut body))) => {
                        body["folder"] = json!(there);
                        let result = match call("POST", "/v1/jobs", Some(&body)) {
                            Ok((200, v)) => v["job"]
                                .as_u64()
                                .ok_or_else(|| "it didn't start a job".to_string()),
                            Ok((_, v)) => Err(error_of(&v)),
                            Err(e) => Err(e),
                        };
                        if let Ok(id) = result {
                            if !watching.contains(&id) {
                                watching.push(id);
                            }
                        }
                        lock().sent.push((local, result));
                    }
                }
                read_at = Instant::now() - REFRESH;
            }
            Some(Ask::Send(local, body, here)) => {
                if let (false, Some(there)) = (here.is_empty(), body["folder"].as_str()) {
                    let note = push_latest(&cfg, &here, there);
                    lock().job_notes.push((local, note));
                }
                let result = match call("POST", "/v1/jobs", Some(&body)) {
                    Ok((200, v)) => v["job"]
                        .as_u64()
                        .ok_or_else(|| "it didn't start a job".to_string()),
                    Ok((_, v)) => Err(error_of(&v)),
                    Err(e) => Err(e),
                };
                if let Ok(id) = result {
                    if !watching.contains(&id) {
                        watching.push(id);
                    }
                }
                lock().sent.push((local, result));
                read_at = Instant::now() - REFRESH;
            }
            Some(Ask::Act(id, action, body)) => {
                let path = format!("/v1/jobs/{id}/{action}");
                match call("POST", &path, Some(&body)) {
                    Ok((200, _)) => {}
                    Ok((_, v)) => lock().notes.push(format!("{label}: {}", error_of(&v))),
                    Err(e) => lock().notes.push(format!("Couldn't reach {label}: {e}")),
                }
                read_at = Instant::now() - REFRESH;
            }
            Some(Ask::Watch(_)) | None => {}
        }
        if read_at.elapsed() < REFRESH {
            continue;
        }
        read_at = Instant::now();
        match call("GET", "/v1/state", None) {
            Ok((200, v)) => {
                let engines = v["engines"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| e.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let projects = v["folders"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|f| Project {
                                name: f["name"].as_str().unwrap_or("").to_string(),
                                path: f["path"].as_str().unwrap_or("").to_string(),
                                remote: f["remote"].as_str().unwrap_or("").to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut s = lock();
                s.up = true;
                s.line = format!("Connected to {label}.");
                s.engines = engines;
                s.projects = projects;
                s.logins = v["logins"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|l| AgentThere {
                                agent: l["agent"].as_str().unwrap_or("").to_string(),
                                name: l["name"].as_str().unwrap_or("").to_string(),
                                installed: l["installed"].as_bool().unwrap_or(false),
                                signed_in: l["signed_in"].as_bool().unwrap_or(false),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Ok((401, _)) => {
                let mut s = lock();
                s.up = false;
                s.line = format!(
                    "{label} doesn't know this computer any more. Disconnect and connect again."
                );
                continue;
            }
            Ok((_, v)) => {
                let mut s = lock();
                s.up = false;
                s.line = format!("{label}: {}", error_of(&v));
                continue;
            }
            Err(e) => {
                let mut s = lock();
                s.up = false;
                s.line = format!("Can't reach {label}: {e}");
                tunnel = None;
                continue;
            }
        }
        for id in watching.clone() {
            if let Ok((200, v)) = call("GET", &format!("/v1/jobs/{id}"), None) {
                if let Ok(job) = serde_json::from_value::<store::Job>(v) {
                    lock().jobs.insert(id, job);
                }
            }
        }
    }
}

/// Clones a project onto the server and adds it there.
/// Folders a copy to the server leaves out: things the project rebuilds.
const COPY_SKIPS: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".turbo",
];

/// Copies the project at `local` (this computer) into `~/squad` on the server,
/// over SSH, and adds it there. Needs no git remote and no login to one: it's
/// the files as they are here, with their git history. Gives back its path there.
fn copy_project(cfg: &Config, port: u16, name: &str, local: &str) -> Result<String, String> {
    let label = cfg.label();
    let local = std::path::Path::new(local);
    let (Some(parent), Some(base)) = (local.parent(), local.file_name()) else {
        return Err(format!("{name} isn't a folder Squad can copy"));
    };
    let base = base.to_string_lossy().to_string();
    let mut tar = Command::new("tar");
    tar.arg("-C").arg(parent).arg("-czf").arg("-");
    for skip in COPY_SKIPS {
        tar.arg(format!("--exclude={skip}"));
    }
    let mut pack = tar
        .arg(&base)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("tar isn't available here: {e}"))?;
    let packed = pack.stdout.take().ok_or("tar gave no output")?;
    let script = format!(
        "set -e; mkdir -p squad; tar -xzf - -C squad; cd squad/{} && {{ git config receive.denyCurrentBranch updateInstead 2>/dev/null || true; }}; pwd",
        quote(&base)
    );
    let unpack = ssh_base(&cfg.host, &cfg.key)
        .arg(script)
        .stdin(Stdio::from(packed))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    let packed_ok = pack.wait().map(|s| s.success()).unwrap_or(false);
    if !unpack.status.success() {
        let err = String::from_utf8_lossy(&unpack.stderr);
        return Err(format!(
            "Couldn't copy {name} to {label}: {}",
            err.lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("ssh stopped")
                .trim()
        ));
    }
    if !packed_ok {
        return Err(format!("Couldn't pack {name} up to copy it"));
    }
    let path = String::from_utf8_lossy(&unpack.stdout)
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string();
    match http(
        port,
        "POST",
        "/v1/folders",
        &cfg.token,
        Some(&json!({ "path": path })),
    ) {
        Ok((200, _)) => Ok(path),
        Ok((_, v)) => Err(format!("{label} wouldn't add {name}: {}", error_of(&v))),
        Err(e) => Err(format!("Couldn't reach {label}: {e}")),
    }
}

// ---------- keeping this computer and the server in step ----------
//
// Like the cloud coding tools, Squad syncs through git rather than copying
// files back and forth: before a job runs there, the server gets your latest
// commits; when you approve it, its branch comes back here and merges like a
// job that ran here. Git goes over the same SSH as everything else, so it
// needs no GitHub and no login to one.

/// How git reaches the server: Squad's key and Squad's trusted host keys.
fn git_ssh(key: &str) -> String {
    let mut cmd = format!(
        "ssh -o UserKnownHostsFile={} -o StrictHostKeyChecking=accept-new -o BatchMode=yes -o ConnectTimeout=15",
        quote(&known_hosts().to_string_lossy())
    );
    if !key.is_empty() {
        cmd.push_str(&format!(" -i {} -o IdentitiesOnly=yes", quote(key)));
    }
    cmd
}

/// `git` in `dir` here, able to reach the server.
fn git_here(cfg: &Config, dir: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_SSH_COMMAND", git_ssh(&cfg.key))
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git isn't available: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with("hint:"))
            .unwrap_or("git stopped")
            .to_string())
    }
}

/// The server's copy of a project as a git address.
fn git_url(cfg: &Config, there: &str) -> String {
    format!("{}:{there}", cfg.host)
}

/// Lets a push update the branch the server's copy has checked out, the way
/// a pull there would, as long as nothing there is uncommitted.
fn accept_pushes(cfg: &Config, there: &str) -> Result<String, String> {
    ssh(
        &cfg.host,
        &cfg.key,
        &format!(
            "git -C {} config receive.denyCurrentBranch updateInstead",
            quote(there)
        ),
        None,
    )
}

/// Sends the commits of the branch you're on here to the server's copy, so a
/// job there starts from your latest work. Says what it did, for the job.
fn push_latest(cfg: &Config, here: &str, there: &str) -> String {
    let label = cfg.label();
    let branch = crate::git::current_branch(here);
    if branch.is_empty() || branch == "HEAD" {
        return format!(
            "This project isn't on a branch here, so the job on {label} starts from what's there."
        );
    }
    let pushed = accept_pushes(cfg, there).and_then(|_| {
        git_here(
            cfg,
            here,
            &[
                "push",
                "--quiet",
                &git_url(cfg, there),
                &format!("HEAD:refs/heads/{branch}"),
            ],
        )
    });
    let short = crate::git::head(here)
        .map(|h| h.chars().take(7).collect::<String>())
        .unwrap_or_default();
    let uncommitted = if crate::git::has_uncommitted(here) == Some(true) {
        " Changes you haven't committed stay here."
    } else {
        ""
    };
    match pushed {
        Ok(_) => format!("Sent your latest commit on {branch} ({short}) to {label}, so it starts from there.{uncommitted}"),
        Err(why) => format!(
            "Couldn't send your latest commits to {label} ({why}), so it starts from what {label} has."
        ),
    }
}

/// Brings a job's work home from the server: commits it there on its branch,
/// fetches that branch here, and checks it out in a worktree of its own,
/// as if the job had run here. Gives back that worktree.
fn bring_home(cfg: &Config, job: &store::Job, there: &str) -> Result<String, String> {
    let here = job.folder.path.as_str();
    let branch = job.branch.as_str();
    if branch.is_empty() {
        return Err("it didn't work on a branch of its own".into());
    }
    // Your name on the commit, as when a job here commits.
    let who = |key: &str, or: &str| {
        git_here(cfg, here, &["config", "--get", key])
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| or.to_string())
    };
    let (name, email) = (
        who("user.name", "Squad Code"),
        who("user.email", "squad@localhost"),
    );
    if !job.remote_dir.is_empty() {
        let commit = format!(
            "cd {dir} && git add -A && (git diff --cached --quiet || GIT_AUTHOR_NAME={n} GIT_AUTHOR_EMAIL={e} GIT_COMMITTER_NAME={n} GIT_COMMITTER_EMAIL={e} git commit -q -m {m})",
            dir = quote(&job.remote_dir),
            n = quote(&name),
            e = quote(&email),
            m = quote(&super::work::message(job)),
        );
        ssh(&cfg.host, &cfg.key, &commit, None)
            .map_err(|why| format!("couldn't save its work there: {why}"))?;
    }
    git_here(
        cfg,
        here,
        &[
            "fetch",
            "--quiet",
            &git_url(cfg, there),
            &format!("+refs/heads/{branch}:refs/heads/{branch}"),
        ],
    )
    .map_err(|why| format!("couldn't fetch {branch}: {why}"))?;
    crate::git::add_worktree(here, branch, None)
}

/// [`bring_home`] on a thread of its own.
pub fn bring_home_later(
    cfg: Config,
    job: store::Job,
    there: String,
) -> Receiver<Result<String, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(bring_home(&cfg, &job, &there));
    });
    rx
}

// ---------- the server's brain ----------

/// What giving the server an agent reports as it goes.
#[derive(Clone, Debug)]
pub enum Brain {
    Say(String),
    /// Open this in the browser to sign in; then send the code it shows.
    Url(String),
    Done(String),
    Failed(String),
}

/// The API keys the app can give the server: what people call the company,
/// the agent that uses the key, and the variable it reads.
pub const KEYS: [(&str, &str, &str); 4] = [
    ("Anthropic", "claude", "ANTHROPIC_API_KEY"),
    ("OpenAI", "codex", "OPENAI_API_KEY"),
    ("Google", "gemini", "GEMINI_API_KEY"),
    ("xAI Grok", "grok", "XAI_API_KEY"),
];

fn agent_name(agent: &str) -> &'static str {
    crate::agent::PROVIDERS
        .iter()
        .find(|p| p.id == agent)
        .map(|p| p.name)
        .unwrap_or("the agent")
}

/// Installs `agent` on the server unless it's there already.
fn install_agent(cfg: &Config, agent: &str, say: &dyn Fn(Brain)) -> Result<(), String> {
    let Some(script) = crate::cove::install_script(agent) else {
        return Ok(());
    };
    say(Brain::Say(format!(
        "Making sure {} is installed there…",
        agent_name(agent)
    )));
    ssh(&cfg.host, &cfg.key, "bash -s", Some(&script))
        .map(|_| ())
        .map_err(|why| format!("Couldn't install {} there: {why}", agent_name(agent)))
}

/// Keeps `value` as `env` for the server's agents and restarts the server so
/// they use it. The value goes over SSH's stdin, never on a command line.
fn set_on_server(cfg: &Config, env: &str, value: &str) -> Result<(), String> {
    ssh(
        &cfg.host,
        &cfg.key,
        &format!("{} v2 serve --set {env}", quote(&cfg.bin)),
        Some(&format!("{value}\n")),
    )
    .map(|_| ())
}

/// Gives the server an API key for `agent`, installing the agent first.
pub fn give_key(
    cfg: Config,
    agent: &'static str,
    env: &'static str,
    value: String,
) -> Receiver<Brain> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let say = |b: Brain| {
            let _ = tx.send(b);
        };
        let step = install_agent(&cfg, agent, &say)
            .and_then(|_| {
                say(Brain::Say("Saving the key there…".into()));
                set_on_server(&cfg, env, value.trim())
            })
            .map(|_| {
                Brain::Done(format!(
                    "{} on {} can run jobs now.",
                    agent_name(agent),
                    cfg.label()
                ))
            });
        say(step.unwrap_or_else(Brain::Failed));
    });
    rx
}

/// Signing the server's Claude Code in with your Claude plan: the server runs
/// `claude setup-token`, you open its link here and send back the code.
pub struct SignIn {
    pub rx: Receiver<Brain>,
    code: Sender<String>,
}

impl SignIn {
    pub fn send_code(&self, code: &str) {
        let _ = self.code.send(code.trim().to_string());
    }
}

pub fn claude_sign_in(cfg: Config) -> SignIn {
    let (tx, rx) = channel();
    let (code_tx, code_rx) = channel::<String>();
    std::thread::spawn(move || {
        let say = |b: Brain| {
            let _ = tx.send(b);
        };
        let step = install_agent(&cfg, "claude", &say)
            .and_then(|_| setup_token(&cfg, &say, &code_rx))
            .and_then(|token| {
                say(Brain::Say("Saving the sign-in there…".into()));
                set_on_server(&cfg, "CLAUDE_CODE_OAUTH_TOKEN", &token)
            })
            .map(|_| {
                Brain::Done(format!(
                    "Claude Code on {} runs on your Claude plan now.",
                    cfg.label()
                ))
            });
        say(step.unwrap_or_else(Brain::Failed));
    });
    SignIn { rx, code: code_tx }
}

/// Runs `claude setup-token` on the server in a terminal there and gives back
/// the token it prints once the code from the browser is typed in.
fn setup_token(
    cfg: &Config,
    say: &dyn Fn(Brain),
    codes: &Receiver<String>,
) -> Result<String, String> {
    say(Brain::Say("Asking Claude for a sign-in link…".into()));
    let mut cmd = ssh_base(&cfg.host, &cfg.key);
    cmd.arg("-tt")
        .arg(r#"export PATH="$HOME/.local/bin:$PATH"; stty cols 4000 rows 60 2>/dev/null; exec claude setup-token"#)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no terminal")?;
    let mut stdout = child.stdout.take().ok_or("no terminal")?;
    let (out_tx, out_rx) = channel::<String>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0
                || out_tx
                    .send(String::from_utf8_lossy(&buf[..n]).to_string())
                    .is_err()
            {
                break;
            }
        }
    });
    let mut seen = String::new();
    let mut url_sent = false;
    let mut code_sent: Option<Instant> = None;
    let until = Instant::now() + Duration::from_secs(15 * 60);
    let result = loop {
        if Instant::now() > until {
            break Err("The sign-in took too long. Try again.".to_string());
        }
        match out_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => seen.push_str(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                let plain = screen_text(&seen);
                break match find_token(&plain) {
                    Some(t) => Ok(t),
                    None => Err(format!("Claude's sign-in stopped: {}", claude_said(&plain))),
                };
            }
        }
        let plain = screen_text(&seen);
        if let Some(token) = find_token(&plain) {
            break Ok(token);
        }
        if !url_sent {
            if let Some(url) = find_url(&plain) {
                url_sent = true;
                say(Brain::Url(url));
            }
        }
        match codes.try_recv() {
            Ok(code) if url_sent && code_sent.is_none() => {
                code_sent = Some(Instant::now());
                say(Brain::Say("Checking the code…".into()));
                // Enter on its own: Claude reads a long code that arrives
                // with its Enter as a paste and drops the Enter.
                let _ = stdin.write_all(code.as_bytes());
                let _ = stdin.flush();
                std::thread::sleep(Duration::from_millis(500));
                let _ = stdin.write_all(b"\r");
                let _ = stdin.flush();
            }
            // The app let go: the person cancelled.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                break Err("Cancelled.".to_string());
            }
            _ => {}
        }
        // A good code gives a token within seconds; Claude's last words say why not.
        if code_sent.is_some_and(|at| at.elapsed() > Duration::from_secs(60)) {
            let plain = screen_text(&seen);
            let said = claude_said(&plain);
            break Err(format!(
                "Claude didn't take that code ({said}). Try signing in again."
            ));
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    // Keep what Claude showed, so a failed sign-in can be looked into. It
    // never holds a token (that would have been a success) and the code is
    // masked on screen.
    if result.is_err() && code_sent.is_some() {
        let dir = crate::store::data_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("claude-sign-in.txt"), screen_text(&seen));
    }
    result
}

/// Why Claude's sign-in didn't take a code: its error line, or else its last
/// words that aren't the masked code.
fn claude_said(plain: &str) -> String {
    let lines: Vec<&str> = plain.lines().map(str::trim).collect();
    lines
        .iter()
        .rev()
        .find(|l| l.to_lowercase().contains("error"))
        .or_else(|| {
            lines
                .iter()
                .rev()
                .find(|l| !l.is_empty() && !l.contains("Checking") && !l.starts_with("****"))
        })
        .map(|l| l.to_string())
        .unwrap_or_else(|| "no answer".into())
}

/// What a terminal would show for `raw`, one line per row. Claude's sign-in
/// redraws only what changed, skipping over what's already on screen, so its
/// output only reads right once the moves are played back.
fn screen_text(raw: &str) -> String {
    let mut rows: Vec<Vec<char>> = vec![Vec::new()];
    let (mut r, mut col) = (0usize, 0usize);
    let mut it = raw.chars().peekable();
    while let Some(ch) = it.next() {
        match ch {
            '\u{1b}' => match it.next() {
                Some('[') => {
                    let mut params = String::new();
                    let mut fin = ' ';
                    for c in it.by_ref() {
                        if ('@'..='~').contains(&c) {
                            fin = c;
                            break;
                        }
                        params.push(c);
                    }
                    let nums: Vec<usize> = params
                        .trim_start_matches('?')
                        .split(';')
                        .map(|n| n.parse().unwrap_or(0))
                        .collect();
                    let n = nums.first().copied().unwrap_or(0).max(1);
                    match fin {
                        'A' => r = r.saturating_sub(n),
                        'B' | 'E' => r += n,
                        'C' => col += n,
                        'D' => col = col.saturating_sub(n),
                        'G' => col = n - 1,
                        'H' | 'f' => {
                            r = n - 1;
                            col = nums.get(1).copied().unwrap_or(0).max(1) - 1;
                        }
                        'K' => {
                            let row = &mut rows[r];
                            match nums.first().copied().unwrap_or(0) {
                                0 => row.truncate(col),
                                1 => (0..col.min(row.len())).for_each(|i| row[i] = ' '),
                                _ => row.clear(),
                            }
                        }
                        'J' => match nums.first().copied().unwrap_or(0) {
                            0 => {
                                rows[r].truncate(col);
                                rows.truncate(r + 1);
                            }
                            _ => {
                                rows = vec![Vec::new()];
                                (r, col) = (0, 0);
                            }
                        },
                        _ => {}
                    }
                    if fin == 'E' {
                        col = 0;
                    }
                }
                // OSC: up to BEL or ESC \.
                Some(']') => {
                    while let Some(c) = it.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' {
                            it.next();
                            break;
                        }
                    }
                }
                // Character sets, like ESC ( B: one more character.
                Some('(' | ')' | '*' | '+' | '#' | '%') => {
                    it.next();
                }
                _ => {}
            },
            '\r' => col = 0,
            '\n' => r += 1,
            '\u{8}' => col = col.saturating_sub(1),
            '\t' => col = (col / 8 + 1) * 8,
            c if c.is_control() => {}
            c => {
                while rows.len() <= r {
                    rows.push(Vec::new());
                }
                let row = &mut rows[r];
                if row.len() <= col {
                    row.resize(col, ' ');
                    row.push(c);
                } else {
                    row[col] = c;
                }
                col += 1;
            }
        }
        while rows.len() <= r {
            rows.push(Vec::new());
        }
    }
    rows.iter()
        .map(|row| row.iter().collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Terminal output without its colour and cursor codes.
fn plain_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            if c != '\r' {
                out.push(c);
            }
            continue;
        }
        match it.next() {
            // CSI: parameters, then one final letter.
            Some('[') => {
                for c in it.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // Character sets, like ESC ( B: one more character.
            Some('(' | ')' | '*' | '+' | '#' | '%') => {
                it.next();
            }
            // OSC: up to BEL or ESC \.
            Some(']') => {
                while let Some(c) = it.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn find_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with("https://") && w.contains("oauth"))
        .map(str::to_string)
}

fn find_token(text: &str) -> Option<String> {
    let at = text.find("sk-ant-oat")?;
    let token: String = text[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (token.len() > 40).then_some(token)
}

// ---------- looking at the machine ----------

/// Runs `command` on the server with the app's key, and gives back what it
/// printed. It blocks, so it belongs off the window's thread.
pub fn run_there(cfg: &Config, command: &str) -> Result<String, String> {
    ssh(&cfg.host, &cfg.key, command, None)
}

/// What a terminal runs to open a shell on the server: ssh with Squad's own
/// known_hosts and key, and a terminal of its own there. `cd` starts it in
/// that folder.
pub fn shell_program(cfg: &Config, cd: Option<&str>) -> (String, Vec<String>) {
    let mut args = vec![
        "-o".to_string(),
        format!("UserKnownHostsFile={}", known_hosts().display()),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
    ];
    if !cfg.key.is_empty() {
        args.extend([
            "-i".into(),
            cfg.key.clone(),
            "-o".into(),
            "IdentitiesOnly=yes".into(),
        ]);
    }
    args.extend([
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "ServerAliveInterval=20".into(),
        "-t".into(),
        cfg.host.clone(),
    ]);
    if let Some(dir) = cd {
        args.push(format!(
            "cd {} && exec \"${{SHELL:-/bin/sh}}\" -l",
            quote(dir)
        ));
    }
    ("ssh".into(), args)
}

/// Runs a script on the server (fed to `sh -s`) and gives back what it printed.
pub fn run_script(cfg: &Config, script: &str) -> Result<String, String> {
    ssh(&cfg.host, &cfg.key, "sh -s", Some(script))
}

/// A port on the server's own 127.0.0.1, reached from this computer.
pub struct Forward(Tunnel);

impl Forward {
    /// The port on this computer.
    pub fn port(&self) -> u16 {
        self.0.port
    }

    pub fn alive(&mut self) -> bool {
        self.0.alive()
    }
}

/// Opens an SSH tunnel to `there` on the server; it closes when dropped.
pub fn forward(cfg: &Config, there: u16) -> Result<Forward, String> {
    Tunnel::to(&cfg.host, &cfg.key, there).map(Forward)
}

/// Shell-quotes `s` for a command run on the server.
pub fn quoted(s: &str) -> String {
    quote(s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn password_change_prompts() {
        use super::{prompt_wants_new, wants_new_password};
        assert!(wants_new_password(
            "Password change required but no TTY available."
        ));
        assert!(!wants_new_password("Permission denied (password)."));
        assert_eq!(prompt_wants_new("Current password: "), Some(false));
        assert_eq!(prompt_wants_new("(current) UNIX password:"), Some(false));
        assert_eq!(prompt_wants_new("New password: "), Some(true));
        assert_eq!(prompt_wants_new("Retype new password: "), Some(true));
        assert_eq!(prompt_wants_new("Changing password for ubuntu."), None);
    }

    use super::*;

    #[test]
    fn claude_sign_in_error() {
        let out = "Paste code here:\n*****ssss\nOAuth error: That code wasn't accepted.\nPress Enter to retry.";
        assert_eq!(claude_said(out), "OAuth error: That code wasn't accepted.");
        assert_eq!(
            claude_said("Paste code here:\n**********"),
            "Paste code here:"
        );
    }

    #[test]
    fn hosts_read_like_ssh_takes_them() {
        assert!(valid_host("anish@203.0.113.7"));
        assert!(valid_host("my-vps"));
        assert!(valid_host("me@[2001:db8::1]"));
        assert!(!valid_host(""));
        assert!(!valid_host("-oProxyCommand=x"));
        assert!(!valid_host("me@host; rm -rf ~"));
        assert_eq!(label_of("anish@vps.example.com"), "vps.example.com");
    }

    #[test]
    fn replies_are_read_back() {
        let raw = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 15\r\n\r\n{\"error\":\"no\"}";
        let (status, v) = parse_reply(raw).unwrap();
        assert_eq!(status, 401);
        assert_eq!(error_of(&v), "no");
        assert!(parse_reply(b"HTTP/1.1 200").is_err());
    }

    #[test]
    fn claudes_link_and_token_are_read_from_its_terminal() {
        let screen = "\u{1b}[2K\u{1b}[1mBrowser didn't open? Use the url below to sign in\u{1b}[0m\r\nhttps://claude.com/cai/oauth/authorize?code=true&state=abc\r\nPaste code here if prompted >";
        let plain = plain_text(screen);
        assert_eq!(
            find_url(&plain).as_deref(),
            Some("https://claude.com/cai/oauth/authorize?code=true&state=abc")
        );
        assert_eq!(find_token(&plain), None);
        let done = "Your OAuth token (valid for 1 year):\r\n\u{1b}[32msk-ant-oat01-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-zz\u{1b}[0m\r\n";
        assert_eq!(
            find_token(&plain_text(done)).as_deref(),
            Some("sk-ant-oat01-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-zz")
        );
    }

    #[test]
    fn claudes_redraws_are_played_back() {
        // Only the changed letters are written; the rest is skipped over.
        let token = "sk-ant-oat01-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-zz";
        let before = token.replace("Ij", "xx");
        let raw = format!("\u{1b}(B\u{f}{before}\r\n\u{1b}[1A\u{1b}[21CIj\r\n\u{1b}(B\u{1b}[m");
        let screen = screen_text(&raw);
        assert_eq!(find_token(&screen).as_deref(), Some(token));
        assert!(!screen.lines().any(|l| l.trim() == "B"));
        // A code Claude turns down: the error, not the masked code.
        let raw = "\u{1b}(B\u{f}\u{1b}[?2004h\rPaste code here > \r\n\u{1b}[1A\u{1b}[18C*****CSKgBC\r\r\n\u{1b}[2K\u{1b}[1A\u{1b}[2K\u{1b}[G\u{1b}[95mOAuth error: That code wasn't accepted.\r\n\u{1b}[97mPress \u{1b}[1mEnter\u{1b}[22m to retry. \r\n\u{1b}[K";
        assert_eq!(
            claude_said(&screen_text(raw)),
            "OAuth error: That code wasn't accepted."
        );
        assert_eq!(plain_text("\u{1b}(Bok\u{1b}(B"), "ok");
    }

    #[test]
    fn quotes_survive_the_shell() {
        assert_eq!(quote("it's"), "'it'\\''s'");
    }
}
