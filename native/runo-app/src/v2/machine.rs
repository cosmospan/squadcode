//! Looking at your server and acting on it: how busy it is, what's running
//! there, and the few things you'd otherwise SSH in for (stop a process,
//! restart Squad there, restart the machine, run a command).
//!
//! Everything goes over the same SSH and key as [`super::remote`], on threads
//! of their own, so the window never waits on the network. The page that shows
//! it is [`super::serverpage`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

use super::remote::{self, Config};

/// How often the server is read while its page is open.
const EVERY: Duration = Duration::from_secs(5);
/// How many of the busiest processes the page lists.
pub const TOP: usize = 12;

/// One look at the server. It's POSIX sh and reads /proc, so it works on any
/// Linux without installing anything, and as any user.
const LOOK: &str = r#"
. /etc/os-release 2>/dev/null; echo "os=${PRETTY_NAME:-$(uname -s)}"
echo "kernel=$(uname -r)"
echo "name=$(hostname 2>/dev/null)"
echo "user=$(id -un)"
read up _ < /proc/uptime; echo "uptime=${up%.*}"
echo "cores=$(nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo)"
read l1 l5 l15 _ < /proc/loadavg; echo "load=$l1 $l5 $l15"
a=$(head -1 /proc/stat); sleep 0.4; b=$(head -1 /proc/stat)
echo "$a $b" | awk '{i1=$5+$6; t1=0; for(k=2;k<=9;k++) t1+=$k; i2=$16+$17; t2=0; for(k=13;k<=20;k++) t2+=$k; d=t2-t1; if (d>0) printf "cpu=%.1f\n", 100*(d-(i2-i1))/d}'
awk '/^MemTotal:/{print "mem_total="$2} /^MemAvailable:/{print "mem_free="$2} /^SwapTotal:/{print "swap_total="$2} /^SwapFree:/{print "swap_free="$2}' /proc/meminfo
df -Pk / 2>/dev/null | awk 'NR==2{print "disk="$2" "$3}'
if command -v systemctl >/dev/null 2>&1; then echo "squad=$(systemctl --user is-active squad.service 2>/dev/null)"; fi
if [ "$(id -u)" = 0 ] || sudo -n true 2>/dev/null; then echo "root=1"; fi
ps -eo pid=,user=,pcpu=,pmem=,etimes=,args= --sort=-pcpu 2>/dev/null | head -n 16 | sed 's/^/proc=/'
"#;

/// A process on the server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub user: String,
    pub cpu: f32,
    pub mem: f32,
    /// How long it's been running, in seconds.
    pub secs: u64,
    pub command: String,
}

/// What the server looked like, last time it was read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub os: String,
    pub kernel: String,
    pub name: String,
    pub user: String,
    pub uptime: u64,
    pub cores: u32,
    pub load: [f32; 3],
    /// How busy the processors are, 0 to 100.
    pub cpu: Option<f32>,
    /// Memory and swap, in KiB.
    pub mem_total: u64,
    pub mem_free: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    /// The disk Squad lives on, in KiB.
    pub disk_total: u64,
    pub disk_used: u64,
    /// Squad's service there: "active", "failed", … or "" without systemd.
    pub squad: String,
    /// Whether the login can act as root (it is root, or sudo needs no password).
    pub root: bool,
    pub procs: Vec<Proc>,
}

impl Look {
    pub fn mem_used(&self) -> u64 {
        self.mem_total.saturating_sub(self.mem_free)
    }
}

/// Reads what [`LOOK`] printed.
pub fn parse(text: &str) -> Look {
    let mut l = Look::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        let num = |v: &str| {
            v.split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
        };
        match k {
            "os" => l.os = v.to_string(),
            "kernel" => l.kernel = v.to_string(),
            "name" => l.name = v.to_string(),
            "user" => l.user = v.to_string(),
            "uptime" => l.uptime = num(v).unwrap_or(0),
            "cores" => l.cores = num(v).unwrap_or(0) as u32,
            "load" => {
                for (i, n) in v.split_whitespace().take(3).enumerate() {
                    l.load[i] = n.parse().unwrap_or(0.);
                }
            }
            "cpu" => l.cpu = v.parse().ok(),
            "mem_total" => l.mem_total = num(v).unwrap_or(0),
            "mem_free" => l.mem_free = num(v).unwrap_or(0),
            "swap_total" => l.swap_total = num(v).unwrap_or(0),
            "swap_free" => l.swap_free = num(v).unwrap_or(0),
            "disk" => {
                let mut it = v.split_whitespace().filter_map(|n| n.parse::<u64>().ok());
                l.disk_total = it.next().unwrap_or(0);
                l.disk_used = it.next().unwrap_or(0);
            }
            "squad" => l.squad = v.to_string(),
            "root" => l.root = v == "1",
            "proc" => {
                if let Some(p) = parse_proc(v) {
                    // The look itself isn't worth listing, nor the kernel's own threads.
                    let skip = p.command.starts_with("ps -eo") || p.command.starts_with('[');
                    if !skip && l.procs.len() < TOP {
                        l.procs.push(p);
                    }
                }
            }
            _ => {}
        }
    }
    l
}

fn parse_proc(line: &str) -> Option<Proc> {
    let mut rest = line.trim_start();
    let mut field = || {
        let end = rest.find(char::is_whitespace)?;
        let (f, r) = rest.split_at(end);
        rest = r.trim_start();
        Some(f)
    };
    let pid = field()?.parse().ok()?;
    let user = field()?.to_string();
    let cpu = field()?.parse().unwrap_or(0.);
    let mem = field()?.parse().unwrap_or(0.);
    let secs = field()?.parse().unwrap_or(0);
    let command = rest.trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some(Proc {
        pid,
        user,
        cpu,
        mem,
        secs,
        command,
    })
}

/// Reads the server every few seconds until dropped.
pub struct Watch {
    pub rx: Receiver<Result<Look, String>>,
    on: Arc<AtomicBool>,
}

impl Watch {
    pub fn start(cfg: Config) -> Watch {
        let (tx, rx) = channel();
        let on = Arc::new(AtomicBool::new(true));
        let still = on.clone();
        std::thread::spawn(move || {
            while still.load(Ordering::Relaxed) {
                let got = remote::run_there(&cfg, LOOK).map(|t| parse(&t));
                if tx.send(got).is_err() {
                    return;
                }
                // Sleep in short steps, so leaving the page stops it soon.
                for _ in 0..(EVERY.as_millis() / 250) {
                    if !still.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        });
        Watch { rx, on }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.on.store(false, Ordering::Relaxed);
    }
}

/// Something to do on the server.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Ask a process to stop, or make it (`force`).
    Stop { pid: u32, force: bool },
    /// Start Squad there again.
    RestartSquad,
    /// Restart the whole machine.
    Reboot,
    /// Run a command and show what it printed.
    Run(String),
}

impl Act {
    fn command(&self, cfg: &Config) -> String {
        match self {
            Act::Stop { pid, force } => {
                let sig = if *force { "KILL" } else { "TERM" };
                // As the login first; as root when it can't and sudo allows it.
                format!(
                    "kill -{sig} {pid} 2>/dev/null || sudo -n kill -{sig} {pid}"
                )
            }
            Act::RestartSquad => {
                let bin = if cfg.bin.is_empty() {
                    "$HOME/.local/bin/SquadCode".to_string()
                } else {
                    remote::quoted(&cfg.bin)
                };
                format!(
                    "if command -v systemctl >/dev/null 2>&1 && systemctl --user cat squad.service >/dev/null 2>&1; then systemctl --user restart squad.service; else {bin} v2 serve --install; fi"
                )
            }
            // Detached and a moment later, so ssh gets its answer first.
            Act::Reboot => "if [ \"$(id -u)\" = 0 ]; then r=''; elif sudo -n true 2>/dev/null; then r='sudo -n'; else echo 'error: restarting the server needs root, or sudo without a password' >&2; exit 1; fi; nohup sh -c \"sleep 1; $r reboot\" >/dev/null 2>&1 &".to_string(),
            // Its errors read with its output, and a failure says so instead
            // of losing what it printed.
            Act::Run(c) => format!("{{ {c}\n}} 2>&1 </dev/null; s=$?; [ $s = 0 ] || echo \"(stopped with exit code $s)\""),
        }
    }

    /// What it's called while it runs.
    pub fn doing(&self) -> String {
        match self {
            Act::Stop { pid, force: false } => format!("Stopping process {pid}…"),
            Act::Stop { pid, force: true } => format!("Force-stopping process {pid}…"),
            Act::RestartSquad => "Restarting Squad there…".into(),
            Act::Reboot => "Restarting the server…".into(),
            Act::Run(c) => format!("$ {c}"),
        }
    }

    /// What to say when it worked.
    fn done(&self, out: &str) -> String {
        match self {
            Act::Stop { pid, force: false } => format!("Asked process {pid} to stop."),
            Act::Stop { pid, force: true } => format!("Process {pid} was made to stop."),
            // Started by hand rather than by Squad: it says so, and that's the news.
            Act::RestartSquad if out.contains("already serving") => {
                "Squad there was started by hand, so it wasn't restarted. Stop it there, then press Restart again.".into()
            }
            Act::RestartSquad => {
                "Squad is starting again there; jobs pick up in a few seconds.".into()
            }
            Act::Reboot => {
                "The server is restarting. It's back in a minute or two, and Squad starts with it."
                    .into()
            }
            Act::Run(_) => out.trim_end().to_string(),
        }
    }
}

/// Does `act` on the server; the answer comes back on the receiver once.
pub fn act(cfg: Config, act: Act) -> Receiver<Result<String, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let got = remote::run_there(&cfg, &act.command(&cfg)).map(|out| act.done(&out));
        let _ = tx.send(got);
    });
    rx
}

/// "3 days", "5 h", "12 min": how long, for people.
pub fn span(secs: u64) -> String {
    match secs {
        s if s < 90 => format!("{s} s"),
        s if s < 90 * 60 => format!("{} min", s / 60),
        s if s < 36 * 3600 => format!("{} h", s / 3600),
        s => format!("{} days", s / 86400),
    }
}

/// KiB as GB or MB.
pub fn size(kib: u64) -> String {
    let gb = kib as f64 / 1024. / 1024.;
    if gb >= 1. {
        format!("{gb:.1} GB")
    } else {
        format!("{} MB", kib / 1024)
    }
}

// ---------- its screen ----------

/// What gives the server a desktop and shares it on its own 127.0.0.1.
const DESKTOP: &str = include_str!("../../../../scripts/server-desktop.sh");
/// The virtual screen's number there; the script's default.
pub const DISPLAY: u32 = 77;

/// The server's screen, shared and reached through a tunnel.
pub struct Screen {
    pub tunnel: remote::Forward,
    pub pass: String,
}

impl Screen {
    /// What a VNC viewer opens: `vnc://:pass@127.0.0.1:port`.
    pub fn url(&self) -> String {
        format!("vnc://:{}@127.0.0.1:{}", self.pass, self.tunnel.port())
    }
}

/// Reads the port and password the desktop script printed last.
pub fn parse_desktop(out: &str) -> Option<(u16, String)> {
    let mut port = None;
    let mut pass = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("port=") {
            port = p.trim().parse().ok();
        } else if let Some(p) = line.strip_prefix("pass=") {
            pass = Some(p.trim().to_string());
        }
    }
    Some((port?, pass.filter(|p| !p.is_empty())?))
}

/// Starts the server's desktop (installing it the first time) and opens a
/// tunnel to it, on a thread of its own.
pub fn screen(cfg: Config) -> Receiver<Result<Screen, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let got = remote::run_script(&cfg, DESKTOP).and_then(|out| {
            let (port, pass) = parse_desktop(&out)
                .ok_or_else(|| "the server didn't say where its screen is".to_string())?;
            let tunnel = remote::forward(&cfg, port)
                .map_err(|why| format!("couldn't reach its screen: {why}"))?;
            Ok(Screen { tunnel, pass })
        });
        let _ = tx.send(got);
    });
    rx
}

/// On the server: a job started while the desktop is up runs on its screen,
/// so a browser or app the bot opens shows up where you can watch it.
pub fn use_server_screen() {
    if !cfg!(target_os = "linux") || std::env::var_os("DISPLAY").is_some() {
        return;
    }
    let socket = format!("/tmp/.X11-unix/X{DISPLAY}");
    if std::path::Path::new(&socket).exists() {
        // The serve process starts jobs one at a time from its own thread,
        // and nothing else reads DISPLAY concurrently.
        std::env::set_var("DISPLAY", format!(":{DISPLAY}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_look() {
        let l = parse(
            "os=Ubuntu 24.04.1 LTS\nkernel=6.8.0\nname=box\nuser=root\nuptime=93784\ncores=4\nload=0.52 0.40 0.31\ncpu=12.5\nmem_total=8000000\nmem_free=6000000\nswap_total=0\nswap_free=0\ndisk=80000000 20000000\nsquad=active\nroot=1\nproc=  812 root      23.0  4.1  3600 /root/.local/bin/SquadCode v2 serve\nproc=  999 root       0.0  0.0     0 ps -eo pid=,user=\nproc=   21 root 0.1 0.0 206 [migration/1]\nproc=1201 anish 1.5 0.2 75 node  server.js --port 3000\n",
        );
        assert_eq!(l.os, "Ubuntu 24.04.1 LTS");
        assert_eq!(l.uptime, 93784);
        assert_eq!(l.cores, 4);
        assert_eq!(l.load, [0.52, 0.40, 0.31]);
        assert_eq!(l.cpu, Some(12.5));
        assert_eq!(l.mem_used(), 2000000);
        assert_eq!((l.disk_total, l.disk_used), (80000000, 20000000));
        assert_eq!(l.squad, "active");
        assert!(l.root);
        assert_eq!(l.procs.len(), 2);
        assert_eq!(l.procs[0].pid, 812);
        assert_eq!(l.procs[0].command, "/root/.local/bin/SquadCode v2 serve");
        assert_eq!(l.procs[1].user, "anish");
        assert_eq!(l.procs[1].secs, 75);
        assert_eq!(l.procs[1].command, "node  server.js --port 3000");
    }

    #[test]
    fn odd_lines_dont_break_it() {
        let l = parse("garbage\nload=x\nproc=12\ncpu=\n");
        assert_eq!(l.load, [0., 0., 0.]);
        assert!(l.procs.is_empty());
        assert_eq!(l.cpu, None);
    }

    #[test]
    fn spans_and_sizes_read_well() {
        assert_eq!(span(30), "30 s");
        assert_eq!(span(600), "10 min");
        assert_eq!(span(7200), "2 h");
        assert_eq!(span(3 * 86400), "3 days");
        assert_eq!(size(512 * 1024), "512 MB");
        assert_eq!(size(3 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn reads_where_the_screen_is() {
        let out = "say: Starting its screen…\nport=5977\npass=Ab12Cd34\n";
        assert_eq!(parse_desktop(out), Some((5977, "Ab12Cd34".to_string())));
        assert_eq!(parse_desktop("say: hi\nport=5977\n"), None);
        assert_eq!(parse_desktop("pass=x\nport=nope\n"), None);
    }

    #[test]
    fn the_desktop_stays_private() {
        assert!(DESKTOP.contains("-localhost"));
        assert!(DESKTOP.contains("-rfbauth"));
        assert!(DESKTOP.contains("-nolisten tcp"));
        assert!(DESKTOP.contains(&format!("SQUAD_DISPLAY:-{DISPLAY}")));
    }

    #[test]
    fn the_look_is_plain_sh() {
        // It has to run under dash, Ubuntu's sh.
        assert!(!LOOK.contains("[["));
        assert!(!LOOK.contains("$(("));
    }
}
