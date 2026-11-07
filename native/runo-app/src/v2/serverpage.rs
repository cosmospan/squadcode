//! Your server: a page to look at the machine Squad runs jobs on, and to act
//! on it without opening a terminal app. How busy it is, the jobs running
//! there, its projects, what's eating its processors, and buttons for the few
//! things you'd otherwise SSH in for. A real shell on it opens in the drawer.
//!
//! The reading and acting happen in [`super::machine`].

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::machine::{self, Act};
use super::prefs::Pane;
use super::store::Status;
use super::ui::{self, c, space};
use super::{term, Place, Shell};

/// How long a "click again to …" button waits for the second click.
const ARM: Duration = Duration::from_secs(4);
/// How many answers from the server the page keeps.
const KEEP: usize = 6;

fn card(title: impl Into<SharedString>) -> Div {
    ui::card().overflow_hidden().flex().flex_col().child(
        div()
            .px(px(18.))
            .pt(px(14.))
            .pb(px(10.))
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(c(ui::muted()))
            .child(title.into()),
    )
}

fn row() -> Div {
    div()
        .px(px(18.))
        .py(px(12.))
        .flex()
        .items_center()
        .gap(px(space::M))
        .border_t_1()
        .border_color(c(ui::line2()))
}

fn words(title: impl Into<SharedString>, line: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(3.))
        .child(
            div()
                .truncate()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(ui::ink()))
                .child(title.into()),
        )
        .child(
            div()
                .truncate()
                .text_size(px(12.5))
                .text_color(c(ui::muted()))
                .child(line.into()),
        )
}

/// One of the three gauges: what, how full, and the numbers under it.
fn gauge(name: &'static str, frac: Option<f32>, big: String, small: String) -> Div {
    let f = frac.unwrap_or(0.).clamp(0., 1.);
    let color = if f > 0.9 {
        ui::t_red()
    } else if f > 0.7 {
        ui::t_amber()
    } else {
        ui::t_green()
    };
    div()
        .flex_1()
        .min_w_0()
        .p(px(14.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .rounded(px(10.))
        .bg(c(ui::hover()))
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::muted()))
                .child(name),
        )
        .child(
            div()
                .text_size(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::ink()))
                .child(big),
        )
        .child(ui::bar(f, color))
        .child(
            div()
                .truncate()
                .text_size(px(12.))
                .text_color(c(ui::faint()))
                .child(small),
        )
}

impl Shell {
    /// Whether `key`'s button was clicked once and waits for a second click.
    fn server_armed(&self, key: &str) -> bool {
        self.server_arm
            .as_ref()
            .is_some_and(|(k, at)| k == key && at.elapsed() < ARM)
    }

    /// The first click arms a risky button; the second, soon after, goes ahead.
    fn server_confirm(&mut self, key: &str) -> bool {
        if self.server_armed(key) {
            self.server_arm = None;
            true
        } else {
            self.server_arm = Some((key.to_string(), Instant::now()));
            false
        }
    }

    /// Does `act` on the server, and says how it went on the page.
    pub(super) fn server_do(&mut self, act: Act, cx: &mut Context<Self>) {
        let Some(link) = &self.crew.link else {
            return;
        };
        let rx = machine::act(link.cfg.clone(), act.clone());
        self.server_acts.push((act, rx));
        cx.notify();
    }

    fn server_note(&mut self, head: String, body: String, failed: bool) {
        self.server_out.insert(0, (head, body, failed));
        self.server_out.truncate(KEEP);
    }

    /// Reads the server while its page is open, and what actions answered.
    pub(super) fn poll_machine(&mut self, cx: &mut Context<Self>) {
        if self.place != Place::Server || self.crew.link.is_none() {
            self.server_watch = None;
        }
        // The picture streams only while you're looking at it.
        if self.place != Place::Screen && self.screen_view.is_some() {
            self.screen_view = None;
            if let Some(old) = self.screen_img.take() {
                self.screen_retired.push(old);
            }
        }
        let mut changed = false;
        if let Some(w) = &self.server_watch {
            let got: Vec<_> = w.rx.try_iter().collect();
            for g in got {
                changed = true;
                match g {
                    Ok(look) => {
                        // A process that's gone needs no force any more.
                        self.server_stopped
                            .retain(|pid| look.procs.iter().any(|p| p.pid == *pid));
                        self.server_look = Some(look);
                        self.server_look_err.clear();
                    }
                    Err(why) => self.server_look_err = why,
                }
            }
        }
        if let Some(rx) = &self.server_screen_rx {
            match rx.try_recv() {
                Ok(Ok(screen)) => {
                    self.server_screen = Some(screen);
                    self.server_screen_rx = None;
                    self.screen_view = None;
                    self.watch_screen(cx);
                    changed = true;
                }
                Ok(Err(why)) => {
                    self.server_screen_rx = None;
                    self.say(format!("Couldn't show the server's screen: {why}"));
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(_) => self.server_screen_rx = None,
            }
        }
        let mut done = Vec::new();
        self.server_acts.retain(|(act, rx)| match rx.try_recv() {
            Ok(got) => {
                done.push((act.clone(), got));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for (act, got) in done {
            changed = true;
            if let Act::Stop { pid, force: false } = act {
                self.server_stopped.insert(pid);
            }
            let head = act.doing();
            match (got, &act) {
                (Ok(out), Act::Run(_)) => {
                    let failed = out
                        .lines()
                        .last()
                        .is_some_and(|l| l.starts_with("(stopped with exit code"));
                    self.server_note(head, out, failed)
                }
                (Ok(said), _) => {
                    self.server_note(said.clone(), String::new(), false);
                    if act == Act::Reboot {
                        self.say(said);
                    }
                }
                (Err(why), _) => self.server_note(head, format!("Didn't work: {why}"), true),
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// "See its screen": starts the server's desktop the first time and
    /// opens it in the screen viewer; after that, opens it straight away.
    pub(super) fn open_server_screen(&mut self, cx: &mut Context<Self>) {
        if self.server_screen_rx.is_some() {
            return;
        }
        if let Some(s) = &mut self.server_screen {
            if s.tunnel.alive() {
                self.watch_screen(cx);
                return;
            }
        }
        self.server_screen = None;
        let Some(link) = &self.crew.link else {
            return;
        };
        self.server_screen_rx = Some(machine::screen(link.cfg.clone()));
        self.say(format!(
            "Getting {}'s screen ready. The first time, Squad installs a light desktop there, which takes a few minutes.",
            link.cfg.label()
        ));
        cx.notify();
    }

    /// The button that shows the server's screen, as it stands.
    pub(super) fn screen_button(
        &self,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let busy = self.server_screen_rx.is_some();
        ui::ghost_i(id, IconName::Monitor, if busy { "Getting its screen…" } else { label })
            .when(busy, |d| d.opacity(0.6))
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(
                    "The server's own desktop, live: whatever a bot opens there (a browser, the app it's testing) shows up. It's shared only over SSH, with a password.",
                )
                .build(window, cx)
            })
            .on_click(cx.listener(|this, _, _, cx| this.open_server_screen(cx)))
    }

    /// Opens or closes the shell on the server, in the drawer.
    pub(super) fn toggle_server_term(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_term(term::SERVER, window, cx);
    }

    pub(super) fn run_server_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.server_cmd.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.server_cmd
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.server_do(Act::Run(text), cx);
    }

    pub(super) fn server_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(link) = &self.crew.link else {
            let body = div().child(
                card("NOT CONNECTED").child(
                    row()
                        .child(words(
                            "No server yet",
                            "Connect your own Linux server in Settings, and this page shows how it's doing and lets you run it from here.",
                        ))
                        .child(
                            ui::solid_i("server-page-connect", IconName::Server, "Connect a server")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.go(Place::Settings(Pane::Server), cx)
                                })),
                        ),
                ),
            );
            return Self::page(
                "server-page",
                "Your server",
                "See how your server is doing and run it from here.",
                body,
            );
        };
        if self.server_watch.is_none() {
            self.server_watch = Some(machine::Watch::start(link.cfg.clone()));
        }
        let cfg = link.cfg.clone();
        let label = cfg.label();
        let up = link.up();
        let projects = link.projects();
        let look = self.server_look.clone();
        let err = self.server_look_err.clone();

        // ---- the machine, and the big buttons ----
        let (dot, state) = match (&look, err.is_empty()) {
            (_, false) => (ui::t_red(), format!("Can't reach it: {err}")),
            (None, true) => (ui::faint(), "Looking…".to_string()),
            (Some(l), true) => (
                ui::t_green(),
                format!("Up {} \u{b7} {}", machine::span(l.uptime), l.os),
            ),
        };
        let squad = look.as_ref().map(|l| l.squad.clone()).unwrap_or_default();
        let squad_line = match (squad.as_str(), up) {
            (_, true) => "Squad is running there and talking to this app.".to_string(),
            ("active", false) => {
                "Squad is running there; the app is still reaching it.".to_string()
            }
            ("", false) => "The app can't reach Squad there right now.".to_string(),
            (s, false) => format!("Squad there is {s}. Restart it below."),
        };
        let can_root = look.as_ref().is_some_and(|l| l.root);
        let reboot_armed = self.server_armed("reboot");
        let restart_armed = self.server_armed("restart");
        let term_open = self.term_open == Some(term::SERVER);
        let head = div()
            .px(px(18.))
            .py(px(14.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .border_t_1()
            .border_color(c(ui::line2()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().size(px(9.)).flex_none().rounded_full().bg(c(dot)))
                    .child(words(
                        format!(
                            "{label}{}",
                            look.as_ref()
                                .filter(|l| !l.name.is_empty() && l.name != label)
                                .map(|l| format!("  ({})", l.name))
                                .unwrap_or_default()
                        ),
                        state,
                    )),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::muted()))
                    .child(squad_line),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        ui::solid_i(
                            "server-term",
                            IconName::SquareTerminal,
                            if term_open {
                                "Hide terminal"
                            } else {
                                "Open terminal"
                            },
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_server_term(window, cx)),
                        ),
                    )
                    .child(self.screen_button("server-screen", "See its screen", cx))
                    .child(
                        ui::ghost_i(
                            "server-restart-squad",
                            IconName::RotateCcw,
                            if restart_armed {
                                "Click again to restart Squad"
                            } else {
                                "Restart Squad there"
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.server_confirm("restart") {
                                this.server_do(Act::RestartSquad, cx);
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        ui::ghost_i(
                            "server-reboot",
                            IconName::Power,
                            if reboot_armed {
                                "Click again: jobs there stop"
                            } else {
                                "Restart the server"
                            },
                        )
                        .when(!can_root, |d| d.opacity(0.5))
                        .when(reboot_armed, |d| {
                            d.border_color(c(ui::t_red())).text_color(c(ui::t_red()))
                        })
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(if can_root {
                                "Restarts the machine. Squad comes back with it; running jobs stop."
                            } else {
                                "Needs root, or sudo without a password, on the server."
                            })
                            .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !can_root {
                                return;
                            }
                            if this.server_confirm("reboot") {
                                this.server_do(Act::Reboot, cx);
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        ui::ghost_i("server-settings", IconName::Settings, "Settings").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.go(Place::Settings(Pane::Server), cx)
                            }),
                        ),
                    ),
            );
        let mut machine_card = card("YOUR SERVER").child(head);
        if self.server_out.iter().any(|(h, _, _)| !h.starts_with("$ ")) {
            machine_card = machine_card.child(self.server_answers(false));
        }

        // ---- how busy it is ----
        let gauges = look.as_ref().map(|l| {
            let cpu = l.cpu.map(|p| p / 100.);
            let cores = l.cores.max(1);
            let mem = (l.mem_total > 0).then(|| l.mem_used() as f32 / l.mem_total as f32);
            let disk = (l.disk_total > 0).then(|| l.disk_used as f32 / l.disk_total as f32);
            let swap = if l.swap_total > 0 {
                format!(
                    " \u{b7} swap {}",
                    machine::size(l.swap_total.saturating_sub(l.swap_free))
                )
            } else {
                String::new()
            };
            div()
                .px(px(18.))
                .pb(px(16.))
                .flex()
                .gap(px(10.))
                .child(gauge(
                    "PROCESSORS",
                    cpu,
                    l.cpu.map(|p| format!("{p:.0}%")).unwrap_or("–".into()),
                    format!(
                        "{cores} cores \u{b7} load {:.2} {:.2} {:.2}",
                        l.load[0], l.load[1], l.load[2]
                    ),
                ))
                .child(gauge(
                    "MEMORY",
                    mem,
                    machine::size(l.mem_used()),
                    format!("of {}{swap}", machine::size(l.mem_total)),
                ))
                .child(gauge(
                    "DISK",
                    disk,
                    machine::size(l.disk_used),
                    format!(
                        "of {} \u{b7} {} free",
                        machine::size(l.disk_total),
                        machine::size(l.disk_total.saturating_sub(l.disk_used))
                    ),
                ))
        });

        // ---- jobs running there ----
        let jobs: Vec<(u64, String, String)> = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.status == Status::Running && j.remote_on == label)
            .map(|j| (j.id, j.bot.name().to_string(), j.label().to_string()))
            .collect();
        let mut jobs_card = card(format!("JOBS RUNNING THERE ({})", jobs.len()));
        if jobs.is_empty() {
            jobs_card = jobs_card.child(row().child(words(
                "Nothing right now",
                "Jobs you send to the server show up here while they run.",
            )));
        }
        for (id, bot, what) in jobs {
            jobs_card = jobs_card.child(
                row()
                    .child(words(what, format!("{bot} \u{b7} #{id}")))
                    .child(
                        ui::ghost(("server-job-open", id as usize), "Open").on_click(
                            cx.listener(move |this, _, _, cx| this.go(Place::Live(id), cx)),
                        ),
                    )
                    .child(
                        ui::ghost_i(
                            ("server-job-stop", id as usize),
                            IconName::CircleStop,
                            "Stop",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.stop_job(id, cx))),
                    ),
            );
        }

        // ---- its projects ----
        let mut projects_card = card(format!("PROJECTS THERE ({})", projects.len()));
        if projects.is_empty() {
            projects_card = projects_card.child(row().child(words(
                "None yet",
                "Copy a project there from Settings, or send it a job and Squad copies it first.",
            )));
        }
        for (i, p) in projects.into_iter().enumerate() {
            let path = p.path.clone();
            projects_card = projects_card.child(
                row()
                    .child(words(
                        p.name.clone(),
                        if p.remote.is_empty() {
                            p.path.clone()
                        } else {
                            format!("{} \u{b7} {}", p.path, p.remote)
                        },
                    ))
                    .child(
                        ui::ghost_i(
                            ("server-project-term", i),
                            IconName::SquareTerminal,
                            "Terminal here",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                let cd = format!("cd {}", super::remote::quoted(&path));
                                this.term_run(term::SERVER, &cd, window, cx);
                            },
                        )),
                    ),
            );
        }

        // ---- what's running ----
        let procs = look.as_ref().map(|l| l.procs.clone()).unwrap_or_default();
        let me = look.as_ref().map(|l| l.user.clone()).unwrap_or_default();
        let mut procs_card = card("BUSIEST PROCESSES");
        if procs.is_empty() {
            procs_card = procs_card.child(row().child(words(
                if look.is_some() {
                    "Nothing to show"
                } else {
                    "Looking…"
                },
                "The processes using the most of the server, refreshed every few seconds.",
            )));
        } else {
            procs_card = procs_card.child(
                row()
                    .py(px(8.))
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(ui::faint()))
                    .child(div().w(px(64.)).child("PID"))
                    .child(div().w(px(56.)).child("CPU"))
                    .child(div().w(px(56.)).child("MEM"))
                    .child(div().w(px(64.)).child("UP"))
                    .child(div().flex_1().child("COMMAND"))
                    .child(div().w(px(96.))),
            );
        }
        for p in procs {
            let pid = p.pid;
            let key = format!("stop-{pid}");
            let armed = self.server_armed(&key);
            let force = self.server_stopped.contains(&pid);
            let other = !me.is_empty() && p.user != me && me != "root";
            let label = match (force, armed) {
                (_, true) => "Sure?",
                (true, false) => "Force stop",
                (false, false) => "Stop",
            };
            procs_card = procs_card.child(
                row()
                    .py(px(7.))
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .text_color(c(ui::muted()))
                    .child(div().w(px(64.)).child(pid.to_string()))
                    .child(
                        div()
                            .w(px(56.))
                            .text_color(c(if p.cpu > 50. { ui::t_amber() } else { ui::muted() }))
                            .child(format!("{:.0}%", p.cpu)),
                    )
                    .child(div().w(px(56.)).child(format!("{:.1}%", p.mem)))
                    .child(div().w(px(64.)).child(machine::span(p.secs)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(c(ui::ink()))
                            .child(p.command.clone()),
                    )
                    .child(
                        div().w(px(96.)).flex().justify_end().child(
                            div()
                                .id(("server-proc-stop", pid as usize))
                                .h(px(24.))
                                .px(px(9.))
                                .flex()
                                .items_center()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(c(if armed || force {
                                    ui::t_red()
                                } else {
                                    ui::line_hi()
                                }))
                                .font_family("Geist")
                                .text_size(px(11.5))
                                .text_color(c(if armed || force { ui::t_red() } else { ui::ink() }))
                                .cursor_pointer()
                                .hover(|s| s.bg(c(ui::hover())))
                                .when(other, |d| d.opacity(0.6))
                                .tooltip(move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(if other {
                                        "Another user's process: stopping it needs sudo without a password."
                                    } else if force {
                                        "It didn't stop when asked; this makes it."
                                    } else {
                                        "Asks it to stop, the way Ctrl+C would."
                                    })
                                    .build(window, cx)
                                })
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.server_confirm(&key) {
                                        this.server_do(Act::Stop { pid, force }, cx);
                                    }
                                    cx.notify();
                                })),
                        ),
                    ),
            );
        }

        // ---- a command ----
        let busy = self
            .server_acts
            .iter()
            .any(|(a, _)| matches!(a, Act::Run(_)));
        let run_card = card("RUN A COMMAND THERE").child(
            row()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(36.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(8.))
                        .bg(c(ui::bg()))
                        .border_1()
                        .border_color(c(ui::line()))
                        .font_family(ui::MONO)
                        .text_size(px(12.5))
                        .child(div().text_color(c(ui::faint())).child("$"))
                        .child(Input::new(&self.server_cmd).appearance(false).bordered(false)),
                )
                .child(
                    ui::solid_i(
                        "server-run",
                        IconName::Play,
                        if busy { "Running…" } else { "Run" },
                    )
                    .when(busy, |d| d.opacity(0.6))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.run_server_command(window, cx)
                    })),
                ),
        )
        .child(
            div()
                .px(px(18.))
                .pb(px(14.))
                .text_size(px(12.))
                .text_color(c(ui::faint()))
                .child("Runs once, as your login there, and shows what it printed. For anything that asks questions back, open the terminal."),
        )
        .when(self.server_out.iter().any(|(h, _, _)| h.starts_with("$ ")), |d| {
            d.child(self.server_answers(true))
        });

        let body = div()
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(machine_card.children(gauges))
            .child(jobs_card)
            .child(procs_card)
            .child(run_card)
            .child(projects_card);
        Self::page(
            "server-page",
            "Your server",
            "How it's doing, what's running there, and the controls you'd otherwise SSH in for.",
            body,
        )
    }

    /// What the server said back to the last few things you did, newest
    /// first: the commands you ran (`runs`), or the buttons you pressed.
    fn server_answers(&self, runs: bool) -> Div {
        div()
            .px(px(18.))
            .pb(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(
                self.server_out
                    .iter()
                    .filter(|(head, _, _)| head.starts_with("$ ") == runs)
                    .map(|(head, body, failed)| {
                        let mono = runs;
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(12.5))
                                    .text_color(c(if *failed { ui::t_red() } else { ui::muted() }))
                                    .when(mono, |d| d.font_family(ui::MONO))
                                    .child(
                                        Icon::new(if *failed {
                                            IconName::CircleX
                                        } else {
                                            IconName::Check
                                        })
                                        .size(px(13.)),
                                    )
                                    .child(head.clone()),
                            )
                            .when(!body.is_empty(), |d| {
                                // The last lines of what it printed: a page, not a log.
                                let lines: Vec<&str> = body.lines().collect();
                                let from = lines.len().saturating_sub(24);
                                d.child(
                                    div()
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .bg(c(ui::term().bg))
                                        .font_family(ui::MONO)
                                        .text_size(px(11.5))
                                        .text_color(c(if *failed {
                                            ui::t_red()
                                        } else {
                                            ui::term().fg
                                        }))
                                        .flex()
                                        .flex_col()
                                        .when(from > 0, |d| {
                                            d.child(
                                                div()
                                                    .text_color(c(ui::term().dim))
                                                    .child(format!("… {from} lines above")),
                                            )
                                        })
                                        .children(lines[from..].iter().map(|l| {
                                            div().whitespace_nowrap().child(l.to_string())
                                        })),
                                )
                            })
                    }),
            )
    }
}
