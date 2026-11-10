//! Apps and commands Squad runs for you: the dev server Peek starts when you
//! say "run the app", a command you click in a bot's message, an install.
//! Each one runs in its own hidden shell and shows up as a card where it was
//! asked for: whether it's starting, up, finished or crashed, the address it
//! serves on, its output, and Stop and Restart. Nothing of the crew's runs in
//! your terminal, which stays yours.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::pty;

use super::ui::{self, c, ca};
use super::Shell;

/// The hidden shell's size: tall, so the card can show a long stretch of
/// output without a scrollback walk.
const ROWS: u16 = 200;
const COLS: u16 = 160;
/// Lines of output on a card, folded and opened.
const TAIL: usize = 4;
const FULL: usize = 200;
/// How long Stop waits for Ctrl-C to work before it ends the run outright.
const GRACE: Duration = Duration::from_secs(3);

/// What to run, and where its card goes.
#[derive(Clone)]
pub struct RunSpec {
    pub cmd: String,
    /// The folder it runs in.
    pub dir: String,
    /// The project folder it belongs to, for the strip over the composer.
    pub folder: String,
    /// The job it was started for, if any: its card shows in the job.
    pub job: Option<u64>,
    /// Peek's message about it, if any: its card shows under the message.
    pub talk: Option<u64>,
    /// The address it's expected to serve on, before it says so itself.
    pub hint: Option<String>,
    /// An app that keeps running (a server), not a command that finishes.
    pub keeps: bool,
    /// Run on your server over SSH, in `dir` there.
    pub remote: Option<super::remote::Config>,
    /// An engine's install or sign-in: its card shows on the Engines page and
    /// the install card, not in a thread.
    pub setup: bool,
}

pub struct Run {
    pub id: u64,
    pub spec: RunSpec,
    sh: pty::Shell,
    started: Instant,
    ended: Option<Instant>,
    /// When you asked it to stop; it's sent Ctrl-C, then ended after GRACE.
    stopping: Option<Instant>,
    /// Start it again once the old one has let go of its port.
    restart: bool,
    /// The address it printed, or the one it was expected on.
    url: Option<String>,
    /// Its port answers.
    up: bool,
    probed: Option<Instant>,
    read_at: Option<Instant>,
    /// Its output shown in full on the card.
    pub open: bool,
}

/// Where a run stands, as its card says it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Starting,
    Up,
    Working,
    Finished,
    Failed,
    Stopped,
}

impl State {
    fn words(self, keeps: bool) -> (&'static str, u32) {
        match self {
            State::Starting => ("Starting", ui::t_amber()),
            State::Up => ("Running", ui::t_green()),
            State::Working => ("Running", ui::t_blue()),
            State::Finished => ("Finished", ui::t_green()),
            State::Failed if keeps => ("Crashed", ui::t_red()),
            State::Failed => ("Failed", ui::t_red()),
            State::Stopped => ("Stopped", ui::muted()),
        }
    }

    fn live(self) -> bool {
        matches!(self, State::Starting | State::Up | State::Working)
    }
}

impl Run {
    /// How long it has been running.
    pub fn age(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// The address it serves on, once it said or was expected to.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub fn live(&self) -> bool {
        !self.sh.exited()
    }

    pub fn state(&self) -> State {
        if self.sh.exited() {
            return if self.stopping.is_some() {
                State::Stopped
            } else if self.sh.succeeded() == Some(true) {
                State::Finished
            } else {
                State::Failed
            };
        }
        if self.up {
            return State::Up;
        }
        // A server with no address yet, for its first half minute; after
        // that it's running all the same, just quiet about where.
        if self.spec.keeps && self.started.elapsed() < Duration::from_secs(30) {
            return State::Starting;
        }
        State::Working
    }

    /// Its output, as the screen shows it, without the blank rows under it.
    pub fn lines(&self) -> Vec<String> {
        let text = self.sh.with_screen(|s| s.contents());
        let mut lines: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let first = lines.iter().position(|l| !l.is_empty()).unwrap_or(0);
        lines.drain(..first);
        lines
    }

    /// What changes the card's height: which run, opened, showing an
    /// address, still running.
    pub fn shape(&self) -> (u64, bool, bool, bool) {
        (self.id, self.open, self.url.is_some(), self.live())
    }

    fn port(&self) -> Option<u16> {
        self.url
            .as_deref()
            .and_then(super::rich::local_port)
            .map(|(p, _)| p)
    }
}

/// The program that runs `spec`: your login shell running the command (so
/// it finds node, cargo and the rest as your terminal would), or ssh to your
/// server running it there.
fn program(spec: &RunSpec) -> pty::Program {
    let mut env = vec![
        ("SQUADCODE".to_string(), "1".to_string()),
        // Dev servers that would open a browser tab of their own: the card
        // opens it, inside Squad.
        ("BROWSER".to_string(), "none".to_string()),
    ];
    if let Some(cfg) = &spec.remote {
        let (bin, mut args) = super::remote::shell_program(cfg, None);
        args.push(format!(
            "cd {} && {}",
            super::remote::quoted(&spec.dir),
            spec.cmd
        ));
        return pty::Program {
            bin,
            args,
            env: Vec::new(),
            unset: Vec::new(),
        };
    }
    if cfg!(windows) {
        return pty::Program {
            bin: "powershell.exe".into(),
            args: vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-Command".into(),
                spec.cmd.clone(),
            ],
            env,
            unset: Vec::new(),
        };
    }
    env.push(("FORCE_COLOR".into(), "0".into()));
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    pty::Program {
        bin: shell,
        args: vec!["-l".into(), "-c".into(), spec.cmd.clone()],
        env,
        unset: Vec::new(),
    }
}

/// The address a program's output says it serves on: the first link to this
/// machine ("Local: http://localhost:5173/"), else a port it says it
/// listens on ("listening on port 3000"). 0.0.0.0 and 127.0.0.1 read as
/// localhost.
pub fn find_url(text: &str) -> Option<String> {
    find_link(text).or_else(|| said_port(text))
}

/// An address written out in the output: a link, or host and port.
fn find_link(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        let Some(at) = word.find("http://").or_else(|| word.find("https://")) else {
            continue;
        };
        let link = word[at..].trim_end_matches(|ch: char| {
            matches!(ch, ')' | ']' | '>' | ',' | '.' | ';' | '\'' | '"' | '`')
        });
        let (scheme, rest) = link.split_once("://")?;
        let rest = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]", "[::]"]
            .iter()
            .find_map(|h| rest.strip_prefix(h).map(|r| (h, r)));
        let Some((_, after)) = rest else { continue };
        let Some(port) = after.strip_prefix(':') else {
            continue;
        };
        let digits: String = port.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        let tail = &port[digits.len()..];
        let tail = if tail == "/" { "" } else { tail };
        return Some(format!("{scheme}://localhost:{digits}{tail}"));
    }
    // "0.0.0.0:8080->80/tcp" from Docker, "listening on 127.0.0.1:4000".
    for word in text.split_whitespace() {
        let rest = ["localhost:", "127.0.0.1:", "0.0.0.0:", "[::]:"]
            .iter()
            .find_map(|h| word.strip_prefix(h));
        if let Some(rest) = rest {
            let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
            if (2..=5).contains(&digits.len()) {
                return Some(format!("http://localhost:{digits}"));
            }
        }
    }
    None
}

/// A port the output only mentions in words: "listening on port 3000".
fn said_port(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    for pat in [
        "listening on port ",
        "listening at port ",
        "running on port ",
        "on port ",
        "port: ",
        "port ",
    ] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(pat) {
            let start = from + at + pat.len();
            let digits: String = lower[start..]
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if (2..=5).contains(&digits.len()) {
                return Some(format!("http://localhost:{digits}"));
            }
            from = start;
        }
    }
    None
}

/// Whether something answers on `port` here.
fn answers(port: u16) -> bool {
    use std::net::{SocketAddr, TcpStream};
    let wait = Duration::from_millis(60);
    ["127.0.0.1", "[::1]"].iter().any(|host| {
        format!("{host}:{port}")
            .parse::<SocketAddr>()
            .is_ok_and(|addr| TcpStream::connect_timeout(&addr, wait).is_ok())
    })
}

/// A command that keeps running rather than finishing: a dev server, a
/// watcher.
pub fn keeps_running(cmd: &str) -> bool {
    let c = cmd.to_lowercase();
    [
        " dev",
        " start",
        " serve",
        "watch",
        "runserver",
        "http.server",
        "cargo run",
        "go run",
        "compose up",
        "docker run",
        "uvicorn",
        "gunicorn",
        "flask run",
        "rails s",
        "rails server",
        "artisan serve",
        "phx.server",
        "next dev",
        "ng serve",
        "vite",
        "nodemon",
        "flutter run",
        "expo start",
        "react-native start",
        "streamlit run",
        "jupyter",
        "hugo server",
        "jekyll serve",
        "dotnet run",
        "bootrun",
        "spring-boot:run",
    ]
    .iter()
    .any(|k| c.contains(k))
        || c == "npm start"
        || c.ends_with("npm start")
}

impl Shell {
    /// Starts `spec` in a hidden shell of its own and shows its card. The
    /// same command already running in the same folder isn't started twice:
    /// that run's card moves to the new place instead. Returns its id.
    pub(super) fn start_run(
        &mut self,
        spec: RunSpec,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        if let Some(r) = self
            .runs
            .iter_mut()
            .find(|r| r.spec.cmd == spec.cmd && r.spec.dir == spec.dir && r.live())
        {
            r.spec.talk = spec.talk.or(r.spec.talk);
            r.spec.job = spec.job.or(r.spec.job);
            return Ok(r.id);
        }
        // A finished run of the same command gives its place to the new one.
        self.runs
            .retain(|r| !(r.spec.cmd == spec.cmd && r.spec.dir == spec.dir));
        self.next_run += 1;
        let id = self.next_run;
        let sh = spawn(id, &spec)?;
        self.runs.push(Run {
            id,
            url: spec.hint.clone(),
            spec,
            sh,
            started: Instant::now(),
            ended: None,
            stopping: None,
            restart: false,
            up: false,
            probed: None,
            read_at: None,
            open: false,
        });
        cx.notify();
        Ok(id)
    }

    /// Asks run `id` to stop, as Ctrl-C does; it's ended outright if it
    /// hasn't after a few seconds.
    pub(super) fn stop_run(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(r) = self.runs.iter_mut().find(|r| r.id == id) {
            if r.live() {
                r.sh.write(b"\x03");
                r.stopping = Some(Instant::now());
            }
        }
        cx.notify();
    }

    /// Stops run `id` and starts it again once it has let go of its port.
    pub(super) fn restart_run(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(r) = self.runs.iter_mut().find(|r| r.id == id) else {
            return;
        };
        r.restart = true;
        if r.live() {
            r.sh.write(b"\x03");
            r.stopping = Some(Instant::now());
        }
        cx.notify();
    }

    /// Takes a finished run's card away.
    pub(super) fn dismiss_run(&mut self, id: u64, cx: &mut Context<Self>) {
        self.runs.retain(|r| r.id != id || r.live());
        cx.notify();
    }

    /// Stops every app that keeps running. Returns how many it stopped.
    pub(super) fn stop_apps(&mut self) -> usize {
        let mut n = 0;
        for r in &mut self.runs {
            if r.live() && r.spec.keeps {
                r.sh.write(b"\x03");
                r.stopping = Some(Instant::now());
                n += 1;
            }
        }
        n
    }

    /// Moves the runs along: reads where they serve, checks they answer, ends
    /// the ones that didn't stop, and starts again the ones asked to.
    pub(super) fn poll_runs(&mut self) {
        let mut again: Vec<usize> = Vec::new();
        for (k, r) in self.runs.iter_mut().enumerate() {
            if !r.live() {
                if r.ended.is_none() {
                    r.ended = Some(Instant::now());
                    r.up = false;
                }
                if r.restart {
                    again.push(k);
                }
                continue;
            }
            if r.stopping.is_some_and(|at| at.elapsed() > GRACE) {
                r.sh.kill();
                continue;
            }
            if r.read_at
                .is_none_or(|at| at.elapsed() > Duration::from_millis(500))
            {
                r.read_at = Some(Instant::now());
                // With an address known ahead (a Compose file's port, say),
                // only a written-out address replaces it: "port 5432" in a
                // database's log isn't where the app is.
                let text = r.lines().join("\n");
                let said = if r.spec.hint.is_some() {
                    find_link(&text)
                } else {
                    find_url(&text)
                };
                if said.is_some() && said != r.url {
                    r.url = said;
                    r.up = false;
                    r.probed = None;
                }
            }
            if r.up || r.url.is_none() {
                continue;
            }
            if r.spec.remote.is_some() {
                // There's no knocking on a port over there: it said where it
                // serves, so it's up.
                r.up = r.read_at.is_some() && find_url(&r.lines().join("\n")).is_some();
                continue;
            }
            if r.probed
                .is_none_or(|at| at.elapsed() > Duration::from_secs(1))
            {
                r.probed = Some(Instant::now());
                if let Some(port) = r.port() {
                    r.up = answers(port);
                }
            }
        }
        for k in again.into_iter().rev() {
            let old = self.runs.remove(k);
            let spec = old.spec.clone();
            let open = old.open;
            drop(old);
            self.next_run += 1;
            let id = self.next_run;
            match spawn(id, &spec) {
                Ok(sh) => self.runs.insert(
                    k,
                    Run {
                        id,
                        url: spec.hint.clone(),
                        spec,
                        sh,
                        started: Instant::now(),
                        ended: None,
                        stopping: None,
                        restart: false,
                        up: false,
                        probed: None,
                        read_at: None,
                        open,
                    },
                ),
                Err(why) => self.say(format!("Couldn't start it again: {why}")),
            }
        }
    }

    /// The cards for engine installs and sign-ins.
    pub(super) fn setup_runs(&mut self, cx: &mut Context<Self>) -> Vec<Div> {
        let ids: Vec<u64> = self
            .runs
            .iter()
            .filter(|r| r.spec.setup)
            .map(|r| r.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.run_card(id, cx))
            .collect()
    }

    /// Runs an engine's install, update or sign-in inside Squad, its card on
    /// the page you're on: you watch it, and type to it if it asks.
    pub(super) fn run_setup(&mut self, cmd: &str, cx: &mut Context<Self>) {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        let spec = RunSpec {
            cmd: cmd.to_string(),
            dir: home,
            folder: String::new(),
            job: None,
            talk: None,
            hint: None,
            keeps: false,
            remote: None,
            setup: true,
        };
        match self.start_run(spec, cx) {
            Ok(_) => self.say("Running it here. Its card shows how it goes; once it's done, the engine shows as ready."),
            Err(why) => self.say(format!("Couldn't run it: {why}")),
        }
        cx.notify();
    }

    /// The cards for runs started from Peek's message `talk`.
    pub(super) fn talk_runs(&mut self, talk: u64, cx: &mut Context<Self>) -> Vec<Div> {
        let ids: Vec<u64> = self
            .runs
            .iter()
            .filter(|r| r.spec.talk == Some(talk))
            .map(|r| r.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.run_card(id, cx))
            .collect()
    }

    /// The cards for runs started for job `job` (not shown under a message).
    pub(super) fn job_runs(&mut self, job: u64, cx: &mut Context<Self>) -> Vec<Div> {
        let ids: Vec<u64> = self
            .runs
            .iter()
            .filter(|r| r.spec.job == Some(job) && r.spec.talk.is_none())
            .map(|r| r.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.run_card(id, cx))
            .collect()
    }

    /// Opens the address run `id` serves on: inside Squad, in its job's
    /// browser, when it has a job; else in your browser. Over a tunnel when
    /// it runs on your server.
    fn open_run(&mut self, id: u64, outside: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.runs.iter().find(|r| r.id == id) else {
            return;
        };
        let Some(url) = r.url.clone() else { return };
        let (job, remote, dir) = (r.spec.job, r.spec.remote.is_some(), r.spec.dir.clone());
        if remote {
            self.act_where(&super::rich::Act::Link(url), &dir, true, window, cx);
            return;
        }
        match job.filter(|_| !outside) {
            Some(job) if self.crew.store.job(job).is_some() => {
                self.open_job_browser(job, Some(url), cx)
            }
            _ => cx.open_url(&url),
        }
    }

    /// One run's card: where it stands, its address, its output, and what
    /// you can do with it.
    pub(super) fn run_card(&mut self, id: u64, cx: &mut Context<Self>) -> Option<Div> {
        let r = self.runs.iter().find(|r| r.id == id)?;
        let state = r.state();
        let (word, tone) = state.words(r.spec.keeps);
        let lines = r.lines();
        let open = r.open;
        let url = r.url.clone().filter(|_| state.live());
        let up = r.up;
        let since = match r.ended {
            Some(at) => format!("ended {} ago", super::store::span(at.elapsed().as_secs())),
            None => format!("for {}", super::store::span(r.started.elapsed().as_secs())),
        };
        let cmd = r.spec.cmd.clone();
        let remote = r.spec.remote.is_some();
        let has_job = r.spec.job.is_some();
        let keeps = r.spec.keeps;
        let typing = self.run_typing == Some(id) && state.live();
        let shown: Vec<String> = {
            let n = if open { FULL } else { TAIL };
            lines[lines.len().saturating_sub(n)..].to_vec()
        };
        let all = lines.join("\n");
        let more = lines.len() > TAIL;
        let term = ui::term();

        let dot = div()
            .size(px(8.))
            .rounded_full()
            .flex_none()
            .bg(c(tone))
            .when(state.live(), |d| d.border_2().border_color(ca(tone, 0.3)));
        let head = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(dot)
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(tone))
                    .child(if state == State::Starting {
                        format!("{word}\u{2026}")
                    } else {
                        word.to_string()
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .text_color(c(ui::ink()))
                    .child(cmd),
            )
            .when(remote, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(11.5))
                        .text_color(c(ui::muted()))
                        .child(Icon::new(IconName::Server).size(px(12.)))
                        .child("on your server"),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child(since),
            );

        // Where it serves: the address, and opening it.
        let address = url.clone().map(|u| {
            let short = u
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .trim_end_matches('/')
                .to_string();
            let port = super::rich::local_port(&u).map(|(p, _)| p);
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .id(("run-url", id as usize))
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(if up {
                            ca(ui::t_green(), 0.35)
                        } else {
                            c(ui::line())
                        })
                        .bg(c(ui::panel()))
                        .text_size(px(12.5))
                        .font_family(ui::MONO)
                        .text_color(c(if up { ui::t_blue() } else { ui::muted() }))
                        .cursor_pointer()
                        .hover(|s| s.bg(c(ui::hover())))
                        .child(Icon::new(IconName::Globe).size(px(13.)))
                        .child(short)
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(if has_job {
                                "Open it in this job's browser, inside Squad"
                            } else {
                                "Open it in your browser"
                            })
                            .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_run(id, false, window, cx)
                        })),
                )
                .when(has_job, |d| {
                    d.child(
                        ui::icon_btn(("run-url-out", id as usize), IconName::ExternalLink)
                            .size(px(28.))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Open in your browser")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_run(id, true, window, cx)
                            })),
                    )
                })
                .child(div().text_size(px(11.5)).text_color(c(ui::muted())).child(
                    match (up, port) {
                        (true, Some(p)) => format!("port {p} is answering"),
                        (true, None) => "up".to_string(),
                        (false, Some(p)) => format!("waiting for port {p}"),
                        (false, None) => "waiting for it".to_string(),
                    },
                ))
        });

        // Its output, in a dark well like the terminal tails on job cards.
        let mut well = div()
            .id(("run-log", id as usize))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(c(term.bg))
            .border_1()
            .border_color(c(if typing { ui::t_blue() } else { term.line }))
            .font_family(ui::MONO)
            .text_size(px(11.5))
            .line_height(px(17.))
            .text_color(c(term.fg))
            .flex()
            .flex_col()
            .when(open, |d| d.max_h(px(320.)).overflow_y_scroll())
            .when(state.live(), |d| {
                d.cursor_text()
                    .track_focus(&self.run_focus)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_typing = Some(id);
                        window.focus(&this.run_focus, cx);
                        cx.notify();
                    }))
                    .on_key_down(
                        cx.listener(move |this, ev: &KeyDownEvent, _, cx| this.run_key(id, ev, cx)),
                    )
            });
        if shown.is_empty() {
            well = well.child(div().text_color(c(term.dim)).child(if state.live() {
                "Nothing printed yet."
            } else {
                "It printed nothing."
            }));
        }
        for (n, l) in shown.into_iter().enumerate() {
            well = well.child(
                div()
                    .id(("run-line", id as usize * 1000 + n))
                    .whitespace_nowrap()
                    .when(!open, |d| d.truncate())
                    .child(if l.is_empty() { " ".to_string() } else { l }),
            );
        }

        let mut buttons = div().flex().items_center().gap(px(6.));
        if more {
            buttons = buttons.child(
                ui::ghost_i(
                    ("run-more", id as usize),
                    if open {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    },
                    if open { "Less output" } else { "All output" },
                )
                .h(px(28.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(r) = this.runs.iter_mut().find(|r| r.id == id) {
                        r.open = !r.open;
                    }
                    cx.notify();
                })),
            );
        }
        buttons = buttons.child(
            ui::ghost_i(("run-copy", id as usize), IconName::Copy, "Copy output")
                .h(px(28.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(all.clone()));
                    this.say("Copied its output.");
                    cx.notify();
                })),
        );
        buttons = buttons.child(div().flex_1());
        if state.live() {
            if keeps {
                buttons = buttons.child(
                    ui::ghost_i(("run-restart", id as usize), IconName::RotateCw, "Restart")
                        .h(px(28.))
                        .on_click(cx.listener(move |this, _, _, cx| this.restart_run(id, cx))),
                );
            }
            buttons = buttons.child(
                ui::ghost_i(("run-stop", id as usize), IconName::CircleStop, "Stop")
                    .h(px(28.))
                    .text_color(c(ui::RED))
                    .on_click(cx.listener(move |this, _, _, cx| this.stop_run(id, cx))),
            );
        } else {
            buttons = buttons
                .child(
                    ui::ghost_i(
                        ("run-again", id as usize),
                        IconName::RotateCcw,
                        if keeps { "Start again" } else { "Run again" },
                    )
                    .h(px(28.))
                    .on_click(cx.listener(move |this, _, _, cx| this.restart_run(id, cx))),
                )
                .child(
                    ui::icon_btn(("run-dismiss", id as usize), IconName::X)
                        .size(px(28.))
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new("Take this card away")
                                .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.dismiss_run(id, cx))),
                );
        }

        Some(
            ui::card()
                .mt(px(6.))
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(head)
                .children(address)
                .child(well)
                .when(typing, |d| {
                    d.child(
                        div().text_size(px(11.)).text_color(c(ui::faint())).child(
                            "Typing goes to it, for anything it asks. Click elsewhere to stop.",
                        ),
                    )
                })
                .child(buttons),
        )
    }

    /// The apps running in this folder, one line each over the composer, so
    /// they're never lost up the thread: what's running, its address, and
    /// Stop. `except` are runs whose card is in view at the bottom already.
    pub(super) fn runs_strip(
        &mut self,
        folder: &str,
        except: &[u64],
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        // A command run from somewhere with no place for its card (a direct
        // message, a file) shows its whole card here.
        let loose: Vec<u64> = self
            .runs
            .iter()
            .filter(|r| {
                r.spec.folder == folder
                    && !r.spec.setup
                    && r.spec.talk.is_none()
                    && r.spec.job.is_none()
            })
            .map(|r| r.id)
            .collect();
        let rows: Vec<(u64, String, Option<String>, State, bool)> = self
            .runs
            .iter()
            .filter(|r| r.spec.folder == folder && r.live() && r.spec.keeps)
            .filter(|r| !except.contains(&r.id) && !loose.contains(&r.id))
            .map(|r| (r.id, r.spec.cmd.clone(), r.url.clone(), r.state(), r.up))
            .collect();
        if rows.is_empty() && loose.is_empty() {
            return None;
        }
        let mut strip = div().flex().flex_col().gap(px(4.));
        for id in loose {
            strip = strip.children(self.run_card(id, cx));
        }
        for (id, cmd, url, state, up) in rows {
            let (word, tone) = state.words(true);
            strip = strip.child(
                div()
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(10.))
                    .bg(c(ui::panel()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_size(px(12.5))
                    .child(div().size(px(7.)).rounded_full().bg(c(tone)))
                    .child(
                        div()
                            .text_color(c(tone))
                            .font_weight(FontWeight::MEDIUM)
                            .child(word),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(ui::ink()))
                            .child(cmd),
                    )
                    .children(url.filter(|_| up).map(|u| {
                        let short = u
                            .trim_start_matches("http://")
                            .trim_end_matches('/')
                            .to_string();
                        div()
                            .id(("strip-url", id as usize))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(ui::t_blue()))
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .child(Icon::new(IconName::Globe).size(px(12.)))
                            .child(short)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_run(id, false, window, cx)
                            }))
                    }))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id(("strip-stop", id as usize))
                            .h(px(24.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded(px(6.))
                            .text_size(px(12.))
                            .text_color(c(ui::RED))
                            .cursor_pointer()
                            .hover(|s| s.bg(c(ui::hover())))
                            .child(Icon::new(IconName::CircleStop).size(px(13.)))
                            .child("Stop")
                            .on_click(cx.listener(move |this, _, _, cx| this.stop_run(id, cx))),
                    ),
            );
        }
        Some(strip)
    }

    /// A key typed into a run's output: sent to it, for a prompt it shows.
    fn run_key(&mut self, id: u64, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(r) = self.runs.iter().find(|r| r.id == id) else {
            return;
        };
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if m.platform {
            return;
        }
        let app_cursor = r.sh.with_screen(|s| s.application_cursor());
        if let Some(bytes) = pty::key_bytes(
            &ks.key,
            ks.key_char.as_deref(),
            m.control,
            m.alt,
            m.shift,
            app_cursor,
        ) {
            r.sh.write(&bytes);
            cx.stop_propagation();
            cx.notify();
        }
    }
}

fn spawn(id: u64, spec: &RunSpec) -> Result<pty::Shell, String> {
    // Ids for these shells sit well away from the terminals' own.
    pty::Shell::spawn_program(
        u64::MAX / 2 + id,
        &spec.dir,
        ROWS,
        COLS,
        Some(program(spec)),
        |_| {},
    )
}

#[cfg(test)]
mod tests {
    use super::{find_url, keeps_running};

    #[cfg(unix)]
    #[test]
    fn a_server_runs_hidden_and_says_where_it_is() {
        use super::{answers, spawn, Run, RunSpec};
        let dir = std::env::temp_dir();
        let port = 20000 + (std::process::id() % 20000) as u16;
        let spec = RunSpec {
            cmd: format!("python3 -u -m http.server {port} --bind 127.0.0.1"),
            dir: dir.to_string_lossy().into_owned(),
            folder: String::new(),
            job: None,
            talk: None,
            hint: None,
            keeps: true,
            remote: None,
            setup: false,
        };
        let sh = spawn(1, &spec).unwrap();
        let run = Run {
            id: 1,
            spec,
            sh,
            started: std::time::Instant::now(),
            ended: None,
            stopping: None,
            restart: false,
            url: None,
            up: false,
            probed: None,
            read_at: None,
            open: false,
        };
        let mut url = None;
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            url = find_url(&run.lines().join("\n"));
            if url.is_some() && answers(port) {
                break;
            }
        }
        assert_eq!(
            url,
            Some(format!("http://localhost:{port}")),
            "{:?}",
            run.lines()
        );
        assert!(answers(port));
        assert!(run.live());
        // Ctrl-C ends it, and the card says it was stopped.
        run.sh.write(b"\x03");
        for _ in 0..50 {
            if !run.live() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!run.live());
        assert!(!answers(port));
    }

    #[test]
    fn a_dev_server_says_where_it_serves() {
        let vite = "  VITE v5.0.0  ready in 300 ms\n\n  \u{279c}  Local:   http://localhost:5173/\n  \u{279c}  Network: http://192.168.1.4:5173/";
        assert_eq!(find_url(vite).as_deref(), Some("http://localhost:5173"));
        assert_eq!(
            find_url("Server running at http://0.0.0.0:8000/docs.").as_deref(),
            Some("http://localhost:8000/docs")
        );
        assert_eq!(
            find_url("Example app listening on port 3000").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            find_url("ready - started server on 0.0.0.0:3000, url: http://127.0.0.1:3000")
                .as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            find_url("web-1  | 0.0.0.0:8080->80/tcp").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            find_url("lib/main.dart is being served at http://localhost:8080").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(find_url("see https://github.com/x/y for help"), None);
        assert_eq!(find_url("compiled 12 files"), None);
    }

    #[test]
    fn servers_keep_running_and_tests_finish() {
        assert!(keeps_running("npm run dev"));
        assert!(keeps_running("npm start"));
        assert!(keeps_running("python3 -m http.server 8000"));
        assert!(keeps_running("docker compose up"));
        assert!(keeps_running("flutter run -d chrome"));
        assert!(keeps_running("go run ."));
        assert!(!keeps_running("npm test"));
        assert!(!keeps_running("cargo build"));
    }
}
