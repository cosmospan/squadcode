//! When an engine's install or update fails: what went wrong, in plain words,
//! and the fixes worth a click. npm's and the vendor scripts' commonest
//! failures are read from the output (a file already where the program goes,
//! no permission, no npm, no network, a half-finished old update) and each
//! gets its own explanation and ways out. Nothing is overwritten or deleted:
//! "Replace" moves the old copy aside first, and asks before it does.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// What the output says went wrong.
#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// Something is already where the new program goes (npm's EEXIST).
    Exists(String),
    /// An earlier update was cut short and left a folder behind (ENOTEMPTY).
    Leftover(String),
    /// No permission to write there (EACCES, EPERM).
    Denied(String),
    /// A tool the command needs isn't on this computer: npm, node, curl…
    Missing(String),
    /// A certificate along the way isn't trusted: usually a company proxy.
    Cert,
    /// The download couldn't reach the internet.
    Offline,
    /// The registry has no such package or version.
    NoPackage,
    /// The engine needs a newer Node.js than this one; `need` is its range.
    OldNode(Option<String>),
}

/// One way out, as a button.
#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// Run this instead. `confirm` says what it changes, shown before it
    /// runs; None runs it straight away.
    Run {
        label: String,
        cmd: String,
        confirm: Option<String>,
    },
    /// Keep the program that's already there, and use it.
    Keep { label: String, path: String },
    /// Copy a command, for running in your own terminal.
    Copy { label: String, cmd: String },
    /// Open a page or a file.
    Open { label: String, url: String },
}

impl Fix {
    pub fn label(&self) -> &str {
        match self {
            Fix::Run { label, .. }
            | Fix::Keep { label, .. }
            | Fix::Copy { label, .. }
            | Fix::Open { label, .. } => label,
        }
    }
}

/// A failed setup run, explained.
#[derive(Clone, Debug)]
pub struct Trouble {
    pub cause: Cause,
    pub title: String,
    pub why: String,
    /// What Squad found, one fact a line ("It's a link to …").
    pub found: Vec<String>,
    /// The fixes, the one we'd pick first.
    pub fixes: Vec<Fix>,
    /// The program's own version, filled in off the window thread.
    pub version: Arc<Mutex<Option<String>>>,
}

/// Where something in the way came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Homebrew's: a formula or a cask, by name.
    Brew(String),
    /// Another npm's global folder, at this prefix.
    Npm(PathBuf),
    /// A program on its own, from a vendor's installer or a download.
    Standalone,
    /// A link to something that isn't there any more.
    Broken(PathBuf),
    /// A folder, or something else.
    Other,
}

/// What's at a path that's in the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Existing {
    /// Where it links to, when it's a link.
    pub link: Option<PathBuf>,
    pub source: Source,
}

/// The output's lines with npm's "npm error" / "npm ERR!" stripped off.
fn bare(line: &str) -> &str {
    let l = line.trim();
    for p in ["npm error ", "npm ERR! ", "npm error", "npm ERR!"] {
        if let Some(rest) = l.strip_prefix(p) {
            return rest.trim();
        }
    }
    l
}

/// The first 'quoted' bit of `s`.
fn quoted(s: &str) -> Option<String> {
    let a = s.find('\'')? + 1;
    let b = s[a..].find('\'')? + a;
    Some(s[a..b].to_string())
}

/// npm's `path <p>` line.
fn npm_path(out: &str) -> Option<String> {
    out.lines()
        .map(bare)
        .find_map(|l| l.strip_prefix("path "))
        .map(|p| p.trim().to_string())
}

/// What went wrong, read from a failed run's output. None when it's none of
/// the failures Squad knows.
pub fn parse(out: &str) -> Option<Cause> {
    let has = |s: &str| out.contains(s);
    if has("EEXIST") {
        let path = out
            .lines()
            .map(bare)
            .find_map(|l| l.strip_prefix("File exists:").map(|p| p.trim().to_string()))
            .or_else(|| npm_path(out))?;
        return Some(Cause::Exists(path));
    }
    if has("ENOTEMPTY") {
        let to = out
            .lines()
            .find(|l| l.contains("ENOTEMPTY") && l.contains(" -> "))
            .and_then(|l| l.split(" -> ").nth(1))
            .and_then(quoted)
            .or_else(|| npm_path(out))?;
        return Some(Cause::Leftover(to));
    }
    if has("EACCES") || has("EPERM") || has("Permission denied") || has("permission denied") {
        let path = npm_path(out)
            .or_else(|| {
                out.lines()
                    .find(|l| l.contains("EACCES") || l.contains("EPERM"))
                    .and_then(quoted)
            })
            .or_else(|| {
                // "mkdir: /usr/local/bin: Permission denied", "cp: cannot
                // create regular file '/usr/local/bin/x': Permission denied"
                let l = out
                    .lines()
                    .find(|l| l.to_lowercase().contains("permission denied"))?;
                quoted(l).or_else(|| {
                    l.split(':')
                        .map(str::trim)
                        .find(|p| p.starts_with('/') || p.starts_with('~'))
                        .map(str::to_string)
                })
            })
            .unwrap_or_default();
        return Some(Cause::Denied(path));
    }
    for tool in ["npm", "node", "npx", "curl", "bash", "brew", "wget"] {
        let gone = [
            format!("command not found: {tool}"),
            format!("{tool}: command not found"),
            format!("{tool}: not found"),
            format!("'{tool}' is not recognized"),
            format!("env: {tool}: No such file or directory"),
            format!("env: \u{2018}{tool}\u{2019}: No such file or directory"),
        ];
        if gone.iter().any(|g| has(g)) {
            return Some(Cause::Missing(tool.to_string()));
        }
    }
    if [
        "SELF_SIGNED_CERT_IN_CHAIN",
        "UNABLE_TO_GET_ISSUER_CERT",
        "UNABLE_TO_VERIFY_LEAF_SIGNATURE",
        "CERT_HAS_EXPIRED",
        "SSL certificate problem",
        "curl: (60)",
    ]
    .iter()
    .any(|s| has(s))
    {
        return Some(Cause::Cert);
    }
    if [
        "ENOTFOUND",
        "EAI_AGAIN",
        "ETIMEDOUT",
        "ECONNREFUSED",
        "ECONNRESET",
        "ENETUNREACH",
        "network request to",
        "Could not resolve host",
        "Failed to connect to",
        "Connection timed out",
        "curl: (6)",
        "curl: (7)",
        "curl: (28)",
        "curl: (35)",
        "curl: (56)",
    ]
    .iter()
    .any(|s| has(s))
    {
        return Some(Cause::Offline);
    }
    if has("E404") || has("ETARGET") {
        return Some(Cause::NoPackage);
    }
    let strict = out.lines().map(bare).any(|l| l == "code EBADENGINE");
    if strict || has("requires Node.js") || has("Unsupported Node.js") {
        let need = out.lines().find_map(|l| {
            let at = l.find("\"node\":\"")? + 8;
            let end = l[at..].find('"')? + at;
            Some(l[at..end].to_string())
        });
        return Some(Cause::OldNode(need));
    }
    None
}

/// The log npm wrote, from "A complete log of this run can be found in: …".
pub fn log_path(out: &str) -> Option<String> {
    let at = out.find("can be found in:")? + "can be found in:".len();
    let rest = out[at..].trim_start();
    let end = rest.find(['\n', ' ']).unwrap_or(rest.len());
    let p = rest[..end].trim_end_matches('.');
    (!p.is_empty()).then(|| p.to_string())
}

/// Looks at what's at `path`: a link and where to, and who put it there.
pub fn inspect(path: &Path) -> Existing {
    let link = std::fs::read_link(path).ok().map(|t| match path.parent() {
        Some(dir) if t.is_relative() => dir.join(t),
        _ => t,
    });
    let real = std::fs::canonicalize(path).ok();
    let source = match (&link, &real) {
        (Some(t), None) => Source::Broken(t.clone()),
        _ => {
            let at = real
                .clone()
                .or(link.clone())
                .unwrap_or_else(|| path.to_path_buf());
            source_of(&at, path)
        }
    };
    Existing { link, source }
}

/// Who put the program at `real` there, from where it really lives.
fn source_of(real: &Path, path: &Path) -> Source {
    let s = real.to_string_lossy();
    for shelf in ["/Cellar/", "/Caskroom/"] {
        if let Some(at) = s.find(shelf) {
            let name = s[at + shelf.len()..].split('/').next().unwrap_or("");
            if !name.is_empty() {
                return Source::Brew(name.to_string());
            }
        }
    }
    if let Some(at) = s.find("/lib/node_modules/") {
        return Source::Npm(PathBuf::from(&s[..at]));
    }
    if let Some(at) = s.find("/node_modules/") {
        return Source::Npm(PathBuf::from(&s[..at]));
    }
    if path.is_dir() {
        return Source::Other;
    }
    // npm's own little launcher scripts, without a link.
    let head = std::fs::read(real)
        .ok()
        .map(|b| String::from_utf8_lossy(&b[..b.len().min(400)]).into_owned())
        .unwrap_or_default();
    if head.starts_with("#!") && head.contains("node_modules") {
        if let Some(at) = head.find("/lib/node_modules/") {
            let start = head[..at]
                .rfind(['"', '\'', ' ', '\n'])
                .map_or(0, |i| i + 1);
            return Source::Npm(PathBuf::from(&head[start..at]));
        }
    }
    if real.is_file() {
        Source::Standalone
    } else {
        Source::Other
    }
}

/// `p` with your home folder written as ~, for reading.
fn tilde(p: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && h != "/" && p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

/// `s`, quoted for a POSIX shell.
fn sh(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "/._-@+=:,".contains(ch))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The npm package a command installs, with any @version taken off.
pub fn package_of(cmd: &str) -> Option<String> {
    let mut words = cmd.split_whitespace();
    words.by_ref().find(|w| *w == "npm")?;
    let pkg = words.rfind(|w| !w.starts_with('-') && *w != "install" && *w != "i")?;
    // "@scope/name@latest" → "@scope/name"; "name@1" → "name".
    let cut = pkg[1..].find('@').map(|i| i + 1);
    Some(match cut {
        Some(i) => pkg[..i].to_string(),
        None => pkg.to_string(),
    })
}

/// "update" for an @latest run, else "install".
fn verb(cmd: &str) -> &'static str {
    if cmd.contains("@latest") {
        "update"
    } else {
        "install"
    }
}

/// The parts of the setting Squad needs to explain a failure: the engine's
/// name, its vendor script, and what's on this computer.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// The engine's name, like "Codex"; "it" when Squad can't tell.
    pub name: String,
    /// The vendor's own install script, when it has one.
    pub script: Option<String>,
    /// Homebrew is here.
    pub brew: bool,
    /// On a Mac (for the words: "your Mac password").
    pub mac: bool,
    /// A suffix for a backup's name, like "20261115-2127".
    pub stamp: String,
}

/// The explanation and fixes for `cause`, a failure of `cmd`; `found` is
/// what's in the way, for Exists.
pub fn advise(cmd: &str, cause: Cause, found: Option<&Existing>, cx: &Context) -> Trouble {
    let name = if cx.name.is_empty() {
        "it".to_string()
    } else {
        cx.name.clone()
    };
    let verb = verb(cmd);
    let npm = cmd.contains("npm ");
    let pkg = package_of(cmd);
    let pw = if cx.mac {
        "your Mac password"
    } else {
        "your password"
    };
    let mut found_lines = Vec::new();
    let mut fixes = Vec::new();
    let (title, why) = match &cause {
        Cause::Exists(path) => {
            let backup = format!("{path}.before-squad-{}", cx.stamp);
            let replace = |label: &str, why: &str| Fix::Run {
                label: label.into(),
                cmd: format!("mv {} {} && {cmd}", sh(path), sh(&backup)),
                confirm: Some(format!(
                    "{why}Squad moves {} to {} (nothing is deleted) and runs the {verb} again. To go back, rename it.", tilde(path), tilde(&backup)
                )),
            };
            let keep = Fix::Keep {
                label: "Keep the one you have".into(),
                path: path.clone(),
            };
            let ex = found.cloned().unwrap_or(Existing {
                link: None,
                source: Source::Other,
            });
            if let Some(t) = &ex.link {
                found_lines.push(format!("It's a link to {}", t.display()));
            }
            match &ex.source {
                Source::Brew(formula) => {
                    found_lines.push(format!("Homebrew installed it, as \u{201c}{formula}\u{201d}."));
                    if cx.brew {
                        fixes.push(Fix::Run {
                            label: "Update it with Homebrew".into(),
                            cmd: format!("brew upgrade {}", sh(formula)),
                            confirm: None,
                        });
                    }
                    fixes.push(keep);
                    fixes.push(replace(
                        "Replace it with npm's",
                        "You'd then have two copies, Homebrew's set aside. ",
                    ));
                }
                Source::Npm(prefix) => {
                    found_lines.push(format!(
                        "Another Node.js installed it, the one in {}.",
                        prefix.display()
                    ));
                    let other = prefix.join("bin/npm");
                    if let (Some(p), true) = (&pkg, other.is_file()) {
                        fixes.push(Fix::Run {
                            label: format!("{} it with that npm", cap(verb)),
                            cmd: format!(
                                "{} install -g {p}@latest",
                                sh(&other.to_string_lossy())
                            ),
                            confirm: None,
                        });
                    }
                    fixes.push(replace("Replace it", ""));
                    fixes.push(keep);
                }
                Source::Broken(target) => {
                    found_lines.push(format!(
                        "That's gone: it's a broken link an old install left behind ({}).",
                        target.display()
                    ));
                    fixes.push(replace("Replace the broken link", ""));
                }
                Source::Standalone => {
                    found_lines.push(format!(
                        "It's a program on its own, likely from {name}'s own installer or a download."
                    ));
                    fixes.push(replace("Replace it", ""));
                    fixes.push(keep);
                    if let Some(s) = &cx.script {
                        fixes.push(Fix::Run {
                            label: format!("{} with {name}'s installer", cap(verb)),
                            cmd: s.clone(),
                            confirm: None,
                        });
                    }
                }
                Source::Other => {
                    fixes.push(replace("Replace it", ""));
                    fixes.push(keep);
                }
            }
            (
                format!("There's already a {} at {}", short_name(path), tilde(path)),
                format!(
                    "Another install of {name} put a program where npm wants to put the new one, and npm won't overwrite a file it didn't make. Use the one that's there, or replace it."
                ),
            )
        }
        Cause::Leftover(dir) => {
            let backup = format!("{dir}.before-squad-{}", cx.stamp);
            fixes.push(Fix::Run {
                label: "Clear it and try again".into(),
                cmd: format!("mv {} {} && {cmd}", sh(dir), sh(&backup)),
                confirm: Some(format!(
                    "Squad moves the leftover folder {} to {} (nothing is deleted) and runs the {verb} again.", tilde(dir), tilde(&backup)
                )),
            });
            (
                "An earlier update didn't finish".to_string(),
                "npm left a half-moved folder behind last time, and it's in the way now. Moving it aside fixes it.".to_string(),
            )
        }
        Cause::Denied(path) => {
            let in_cache = path.contains("/.npm/") || path.ends_with("/.npm");
            if in_cache {
                fixes.push(Fix::Run {
                    label: "Give the npm cache back to you".into(),
                    cmd: format!("sudo chown -R \"$(id -u):$(id -g)\" \"$HOME/.npm\" && {cmd}"),
                    confirm: Some(format!(
                        "An earlier sudo npm left files in your npm cache (~/.npm) that you don't own. This makes you their owner again, then runs the {verb}. It asks for {pw} in the card."
                    )),
                });
                (
                    "npm can't write to its own cache".to_string(),
                    "Some files in ~/.npm belong to the admin account, usually from an earlier sudo npm."
                        .to_string(),
                )
            } else if npm {
                fixes.push(Fix::Run {
                    label: "Install it in your home folder".into(),
                    cmd: with_prefix(cmd, "$HOME/.npm-global"),
                    confirm: None,
                });
                fixes.push(Fix::Run {
                    label: "Use admin rights".into(),
                    cmd: format!("sudo {cmd}"),
                    confirm: Some(format!(
                        "npm runs as the admin and writes to {}. It asks for {pw} in the card; type it there and press Return.",
                        if path.is_empty() { "the system folder" } else { path }
                    )),
                });
                (
                    "npm isn't allowed to write there".to_string(),
                    format!(
                        "npm's global folder{} belongs to the admin account. Installing in a folder of your own (~/.npm-global) needs no password, and Squad finds {name} there.",
                        if path.is_empty() { String::new() } else { format!(" ({path})") }
                    ),
                )
            } else {
                fixes.push(Fix::Copy {
                    label: "Copy the command".into(),
                    cmd: cmd.to_string(),
                });
                (
                    "The installer isn't allowed to write there".to_string(),
                    format!(
                        "{}Run it in your terminal, where it can ask for {pw}.",
                        if path.is_empty() { String::new() } else { format!("{path} belongs to the admin account. ") }
                    ),
                )
            }
        }
        Cause::Missing(tool) if tool == "npm" || tool == "node" || tool == "npx" => {
            if let Some(s) = &cx.script {
                fixes.push(Fix::Run {
                    label: format!("Use {name}'s own installer"),
                    cmd: s.clone(),
                    confirm: None,
                });
            }
            if cx.brew {
                fixes.push(Fix::Run {
                    label: format!("Install Node.js, then {name}"),
                    cmd: format!("brew install node && {cmd}"),
                    confirm: None,
                });
            }
            fixes.push(Fix::Open {
                label: "Get Node.js".into(),
                url: "https://nodejs.org/en/download".into(),
            });
            (
                "Node.js isn't on this computer".to_string(),
                format!(
                    "{name} installs with npm, which comes with Node.js. {}",
                    if cx.script.is_some() {
                        format!("{name} also has its own installer that doesn't need it.")
                    } else {
                        "Install Node.js, then try again.".to_string()
                    }
                ),
            )
        }
        Cause::Missing(tool) => {
            if tool == "brew" {
                fixes.push(Fix::Open {
                    label: "Get Homebrew".into(),
                    url: "https://brew.sh".into(),
                });
            } else if cx.brew {
                fixes.push(Fix::Run {
                    label: format!("Install {tool}, then try again"),
                    cmd: format!("brew install {} && {cmd}", sh(tool)),
                    confirm: None,
                });
            }
            (
                format!("{tool} isn't on this computer"),
                format!("The {verb} needs {tool} to run."),
            )
        }
        Cause::Cert => (
            "A certificate along the way isn't trusted".to_string(),
            "Something between you and the download, usually a company proxy or VPN, swaps in its own certificate. Try again off the VPN, or ask your IT team for the certificate and point npm at it (npm config set cafile …).".to_string(),
        ),
        Cause::Offline => (
            "The download couldn't get through".to_string(),
            "This computer couldn't reach the server. Check you're online (and off any VPN or proxy that blocks it), then try again.".to_string(),
        ),
        Cause::NoPackage => (
            format!("npm couldn't find {}", pkg.clone().unwrap_or_else(|| name.clone())),
            "The registry said it has no such package or version. It may be a moment behind; try again in a minute.".to_string(),
        ),
        Cause::OldNode(need) => {
            if cx.brew {
                fixes.push(Fix::Run {
                    label: format!("Update Node.js, then {name}"),
                    cmd: format!("brew upgrade node && {cmd}"),
                    confirm: None,
                });
            }
            fixes.push(Fix::Open {
                label: "Get a newer Node.js".into(),
                url: "https://nodejs.org/en/download".into(),
            });
            (
                "This Node.js is too old for it".to_string(),
                format!(
                    "{name} needs Node.js {}.",
                    need.clone().unwrap_or_else(|| "a newer version".into())
                ),
            )
        }
    };
    if matches!(cause, Cause::Offline | Cause::Cert | Cause::NoPackage) {
        fixes.insert(
            0,
            Fix::Run {
                label: "Try again".into(),
                cmd: cmd.to_string(),
                confirm: None,
            },
        );
    }
    if npm && !matches!(cause, Cause::Exists(_) | Cause::Leftover(_)) {
        // Running it yourself is always a way out.
        if !fixes.iter().any(|f| matches!(f, Fix::Copy { .. })) {
            fixes.push(Fix::Copy {
                label: "Copy the command".into(),
                cmd: cmd.to_string(),
            });
        }
    }
    Trouble {
        cause,
        title,
        why,
        found: found_lines,
        fixes,
        version: Arc::new(Mutex::new(None)),
    }
}

/// "codex" for "/Users/me/.local/bin/codex", with "program" or "folder".
fn short_name(path: &str) -> String {
    let p = Path::new(path);
    let file = p
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    if p.is_dir() {
        format!("{file} folder")
    } else {
        format!("{file} program")
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// `cmd` with npm told to install under `prefix`, for this run only.
fn with_prefix(cmd: &str, prefix: &str) -> String {
    match cmd.find("npm install") {
        Some(at) => {
            let end = at + "npm install".len();
            format!("{} --prefix \"{prefix}\"{}", &cmd[..end], &cmd[end..])
        }
        None => format!("{cmd} --prefix \"{prefix}\""),
    }
}

/// Asks the program at `path` its version, off the window thread, and puts
/// the answer in `into` (its first line, if it answers within a few seconds).
pub fn ask_version(path: PathBuf, into: Arc<Mutex<Option<String>>>) {
    std::thread::spawn(move || {
        let Ok(mut child) = std::process::Command::new(&path)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            return;
        };
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if start.elapsed() < std::time::Duration::from_secs(4) => {
                    std::thread::sleep(std::time::Duration::from_millis(50))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
        let mut out = String::new();
        if let Some(mut so) = child.stdout.take() {
            use std::io::Read;
            let _ = so.read_to_string(&mut out);
        }
        let line = out.lines().next().unwrap_or("").trim().to_string();
        if !line.is_empty() && line.len() < 120 {
            if let Ok(mut v) = into.lock() {
                *v = Some(line);
            }
        }
    });
}

/// Explains a failed setup run of `cmd` that printed `out`: reads the
/// output, looks at what's in the way, and asks it its version.
pub fn explain(cmd: &str, out: &str, cx: &Context) -> Option<Trouble> {
    let cause = parse(out)?;
    let found = match &cause {
        Cause::Exists(p) => Some(inspect(Path::new(p))),
        _ => None,
    };
    let mut t = advise(cmd, cause, found.as_ref(), cx);
    if let Cause::Exists(p) = &t.cause {
        if found.is_some_and(|f| !matches!(f.source, Source::Broken(_) | Source::Other)) {
            ask_version(PathBuf::from(p), t.version.clone());
        }
    }
    if let Some(log) = log_path(out) {
        t.fixes.push(Fix::Open {
            label: "Open npm's log".into(),
            url: format!("file://{log}"),
        });
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EEXIST: &str = "npm http fetch GET 200 https://registry.npmjs.org/@openai/codex/-/codex-0.169.0-darwin-arm64.tgz 14461ms (cache miss)
npm verbose stack Error: EEXIST: file already exists
npm verbose stack     at failEEXIST (/Users/a/.hermes/node/lib/node_modules/npm/node_modules/bin-links/lib/check-bin.js:49:32)
npm error code EEXIST
npm error path /Users/a/.local/bin/codex
npm error EEXIST: file already exists
npm error File exists: /Users/a/.local/bin/codex
npm error Remove the existing file and try again, or run npm
npm error with --force to overwrite files recklessly.
npm error A complete log of this run can be found in: /Users/a/.npm/_logs/2026-11-15T21_27_50_638Z-debug-0.log";

    fn cx() -> Context {
        Context {
            name: "Codex".into(),
            script: None,
            brew: true,
            mac: true,
            stamp: "20261115-2127".into(),
        }
    }

    #[test]
    fn reads_a_file_in_the_way() {
        assert_eq!(
            parse(EEXIST),
            Some(Cause::Exists("/Users/a/.local/bin/codex".into()))
        );
        assert_eq!(
            log_path(EEXIST).as_deref(),
            Some("/Users/a/.npm/_logs/2026-11-15T21_27_50_638Z-debug-0.log")
        );
        // Older npm says it with ERR!.
        let old = "npm ERR! code EEXIST\nnpm ERR! path /usr/local/bin/gemini\nnpm ERR! EEXIST: file already exists, symlink '../lib/node_modules/@google/gemini-cli/dist/index.js' -> '/usr/local/bin/gemini'";
        assert_eq!(
            parse(old),
            Some(Cause::Exists("/usr/local/bin/gemini".into()))
        );
    }

    #[test]
    fn replacing_moves_the_old_one_aside_and_asks_first() {
        let cmd = "npm install -g @openai/codex@latest";
        let ex = Existing {
            link: None,
            source: Source::Standalone,
        };
        let t = advise(
            cmd,
            Cause::Exists("/Users/a/.local/bin/codex".into()),
            Some(&ex),
            &cx(),
        );
        let Fix::Run {
            cmd: run, confirm, ..
        } = &t.fixes[0]
        else {
            panic!("{:?}", t.fixes)
        };
        assert_eq!(
            run,
            "mv /Users/a/.local/bin/codex /Users/a/.local/bin/codex.before-squad-20261115-2127 && npm install -g @openai/codex@latest"
        );
        assert!(confirm.as_deref().unwrap().contains("nothing is deleted"));
        assert!(t.fixes.iter().any(|f| matches!(f, Fix::Keep { .. })));
        assert!(!run.contains("--force"));
    }

    #[test]
    fn a_homebrew_copy_is_updated_with_homebrew() {
        let cmd = "npm install -g @openai/codex@latest";
        let ex = Existing {
            link: Some("/opt/homebrew/Caskroom/codex/0.150.0/codex".into()),
            source: source_of(
                Path::new("/opt/homebrew/Caskroom/codex/0.150.0/codex"),
                Path::new("/x"),
            ),
        };
        assert_eq!(ex.source, Source::Brew("codex".into()));
        let t = advise(
            cmd,
            Cause::Exists("/opt/homebrew/bin/codex".into()),
            Some(&ex),
            &cx(),
        );
        assert_eq!(
            t.fixes[0],
            Fix::Run {
                label: "Update it with Homebrew".into(),
                cmd: "brew upgrade codex".into(),
                confirm: None
            }
        );
    }

    #[test]
    fn a_copy_from_another_node_is_named() {
        assert_eq!(
            source_of(
                Path::new("/Users/a/.nvm/versions/node/v20.1.0/lib/node_modules/@openai/codex/bin/codex.js"),
                Path::new("/x")
            ),
            Source::Npm("/Users/a/.nvm/versions/node/v20.1.0".into())
        );
    }

    #[test]
    fn a_link_to_nothing_is_broken() {
        let dir = std::env::temp_dir().join(format!("squad-fix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("codex");
        let _ = std::fs::remove_file(&p);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("gone/codex.js"), &p).unwrap();
            assert!(matches!(inspect(&p).source, Source::Broken(_)));
            std::fs::remove_file(&p).unwrap();
        }
        std::fs::write(&p, b"\x7fELF....").unwrap();
        assert_eq!(inspect(&p).source, Source::Standalone);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_permission_offers_a_folder_of_your_own() {
        let out = "npm error code EACCES\nnpm error syscall mkdir\nnpm error path /usr/local/lib/node_modules/@google\nnpm error errno -13\nnpm error Error: EACCES: permission denied, mkdir '/usr/local/lib/node_modules/@google'";
        let cause = parse(out).unwrap();
        assert_eq!(
            cause,
            Cause::Denied("/usr/local/lib/node_modules/@google".into())
        );
        let t = advise("npm install -g @google/gemini-cli", cause, None, &cx());
        assert_eq!(
            t.fixes[0],
            Fix::Run {
                label: "Install it in your home folder".into(),
                cmd: "npm install --prefix \"$HOME/.npm-global\" -g @google/gemini-cli".into(),
                confirm: None
            }
        );
        // sudo is offered, never first, and asks.
        assert!(
            matches!(&t.fixes[1], Fix::Run { cmd, confirm: Some(_), .. } if cmd.starts_with("sudo "))
        );
    }

    #[test]
    fn a_root_owned_cache_is_given_back() {
        let out = "npm error code EACCES\nnpm error path /Users/a/.npm/_cacache/tmp/1\nnpm error Your cache folder contains root-owned files";
        let t = advise("npm install -g x", parse(out).unwrap(), None, &cx());
        assert!(matches!(&t.fixes[0], Fix::Run { cmd, .. } if cmd.contains("chown")));
    }

    #[test]
    fn no_npm_offers_the_vendors_installer() {
        for out in [
            "zsh: command not found: npm",
            "bash: npm: command not found",
            "env: node: No such file or directory",
        ] {
            let c = parse(out).unwrap();
            assert!(matches!(c, Cause::Missing(_)), "{out}");
            let mut x = cx();
            x.script = Some("curl -fsSL https://opencode.ai/install | bash".into());
            let t = advise("npm install -g opencode-ai", c, None, &x);
            assert!(matches!(&t.fixes[0], Fix::Run { cmd, .. } if cmd.starts_with("curl")));
        }
    }

    #[test]
    fn network_certs_leftovers_and_old_node() {
        assert_eq!(parse("npm error code ENOTFOUND\nnpm error network request to https://registry.npmjs.org failed"), Some(Cause::Offline));
        assert_eq!(
            parse("curl: (6) Could not resolve host: claude.ai"),
            Some(Cause::Offline)
        );
        assert_eq!(
            parse("npm error code SELF_SIGNED_CERT_IN_CHAIN"),
            Some(Cause::Cert)
        );
        assert_eq!(
            parse("npm error code ENOTEMPTY\nnpm error ENOTEMPTY: directory not empty, rename '/u/lib/node_modules/@openai/codex' -> '/u/lib/node_modules/@openai/.codex-AbC12'"),
            Some(Cause::Leftover("/u/lib/node_modules/@openai/.codex-AbC12".into()))
        );
        assert_eq!(
            parse("npm error code EBADENGINE\nnpm error notsup Required: {\"node\":\">=20\"}"),
            Some(Cause::OldNode(Some(">=20".into())))
        );
        // A warning about engines isn't the failure.
        assert_eq!(parse("npm warn EBADENGINE Unsupported engine"), None);
        assert_eq!(parse("something else broke"), None);
    }

    #[test]
    fn package_names() {
        assert_eq!(
            package_of("npm install -g @openai/codex@latest").as_deref(),
            Some("@openai/codex")
        );
        assert_eq!(
            package_of("npm install -g opencode-ai").as_deref(),
            Some("opencode-ai")
        );
        assert_eq!(package_of("curl x | bash"), None);
    }
}
