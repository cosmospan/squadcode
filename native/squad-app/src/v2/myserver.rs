//! Settings, My server: connect this app to your own Linux server, give it
//! an agent to run jobs on, and put the open project there. Made for people
//! who've never used SSH: the server's address and the password the VPS
//! provider emailed are enough.
//!
//! The work itself happens in [`super::remote`]; this is its page.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::remote::{self, Brain, Step};
use super::ui::{self, c, space};
use super::Shell;

fn line_row(title: impl Into<SharedString>, line: impl Into<SharedString>) -> Div {
    div()
        .mx(px(14.))
        .py(px(10.))
        .flex()
        .flex_col()
        .gap(px(1.))
        .border_t_1()
        .border_color(c(ui::line()))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::ink()))
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(c(ui::body()))
                .child(line.into()),
        )
}

/// A row with words on the left and controls on the right.
fn action_row(
    title: impl Into<SharedString>,
    line: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .mx(px(14.))
        .py(px(10.))
        .flex()
        .items_center()
        .gap(px(space::M))
        .border_t_1()
        .border_color(c(ui::line()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(c(ui::body()))
                        .child(line.into()),
                ),
        )
        .child(control)
}

/// A card of rows, its mono heading at the top.
fn card(title: &'static str) -> Div {
    div()
        .rounded(px(6.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line()))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(14.))
                .pt(px(11.))
                .pb(px(8.))
                .font_family(ui::MONO)
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(ui::faint()))
                .child(title),
        )
}

fn field(input: &Entity<InputState>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line_hi()))
        .font_family(ui::MONO)
        .text_size(px(12.5))
        .child(Input::new(input).appearance(false).bordered(false))
}

/// What's happening, newest last; the last line in red when it failed.
fn log(lines: &[String], failed: bool) -> Div {
    div()
        .px(px(18.))
        .pb(px(14.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .children(lines.iter().enumerate().map(|(i, l)| {
            let last = i + 1 == lines.len();
            div()
                .text_size(px(12.5))
                .text_color(c(if last && failed {
                    ui::t_red()
                } else {
                    ui::muted()
                }))
                .child(l.clone())
        }))
}

impl Shell {
    /// Reads how connecting and signing in are going.
    pub(super) fn poll_server_setup(&mut self, cx: &mut Context<Self>) {
        if let Some(rx) = &self.server_setup {
            let steps: Vec<Step> = rx.try_iter().collect();
            for step in steps {
                match step {
                    Step::Say(t) => self.server_log.push(t),
                    Step::Done(cfg) => {
                        let locked = self
                            .server_log
                            .iter()
                            .filter(|l| l.starts_with("Done: "))
                            .count();
                        self.server_log.clear();
                        self.server_failed = false;
                        let label = cfg.label();
                        self.crew.link = Some(remote::Link::start(cfg));
                        self.server_setup = None;
                        self.say(if locked > 0 {
                            format!("Connected to {label} and locked it down. Give it an agent to run jobs on next.")
                        } else {
                            format!("Connected to {label}. Give it an agent to run jobs on next.")
                        });
                    }
                    Step::Failed(why) => {
                        self.server_log.push(why);
                        self.server_failed = true;
                        self.server_setup = None;
                    }
                    Step::NewPassword => {
                        self.server_log.push(
                            "This server wants a new password before anyone logs in, which many VPS providers ask for the first time. Pick one below, and Squad changes it and carries on.".into(),
                        );
                        self.server_failed = true;
                        self.server_wants_new_pass = true;
                        self.server_setup = None;
                    }
                    Step::KeyChanged(fp) => {
                        let now = if fp.is_empty() {
                            String::new()
                        } else {
                            format!(" Its key now is {fp}.")
                        };
                        self.server_log.push(format!(
                            "This server isn't showing the key Squad trusted for it before.{now} That's expected if you rebuilt or reinstalled it. If you didn't, don't go on: something may be pretending to be your server."
                        ));
                        self.server_failed = true;
                        self.server_key_changed = true;
                        self.server_setup = None;
                    }
                }
            }
        }
        let mut steps: Vec<Brain> = Vec::new();
        if let Some(rx) = &self.server_brain {
            steps.extend(rx.try_iter());
        }
        if let Some(s) = &self.server_sign_in {
            steps.extend(s.rx.try_iter());
        }
        for step in steps {
            match step {
                Brain::Say(t) => self.server_brain_log.push(t),
                Brain::Url(url) => {
                    self.server_brain_log.push(
                        "Claude's sign-in opened in your browser. Sign in, then paste the code it shows below.".into(),
                    );
                    cx.open_url(&url);
                    self.server_url = Some(url);
                }
                Brain::Done(t) => {
                    self.server_brain_log = vec![t.clone()];
                    self.server_brain_failed = false;
                    self.server_brain = None;
                    self.server_sign_in = None;
                    self.server_url = None;
                    self.say(t);
                }
                Brain::Failed(why) => {
                    self.server_brain_log.push(why);
                    self.server_brain_failed = true;
                    self.server_brain = None;
                    self.server_sign_in = None;
                    self.server_url = None;
                }
            }
        }
    }

    /// The composer's "where it runs" switch, once there's a server: Squad
    /// picks, then your server, then this computer.
    pub(super) fn run_on_pill(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let link = self.crew.link.as_ref()?;
        let (run, label) = (link.cfg.run.clone(), link.cfg.label());
        let (icon, text, tip) = match run.as_str() {
            "server" => (
                IconName::Cloud,
                format!("On {label}"),
                format!("Runs on your server, {label}, and keeps going with this computer closed. Click to run it here instead."),
            ),
            "here" => (
                IconName::Server,
                super::machines::here_name().to_string(),
                "Runs on this computer. Click to let Squad pick.".to_string(),
            ),
            _ => (
                IconName::Sparkles,
                "Squad picks".to_string(),
                format!("Long work goes to {label}, quick work stays here. Click to always use {label}."),
            ),
        };
        Some(
            div()
                .id("run-on")
                .h(px(26.))
                .px(px(9.))
                .flex()
                .items_center()
                .gap(px(5.))
                .rounded(px(3.))
                .text_size(px(12.))
                .text_color(c(ui::muted()))
                .cursor_pointer()
                .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                .child(gpui_kit::component::Icon::new(icon).size(px(13.)))
                .child(text)
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let next = match run.as_str() {
                        "" => "server",
                        "server" => "here",
                        _ => "",
                    };
                    this.set_run(next, cx);
                })),
        )
    }

    /// Where jobs run from now on: "" lets Squad pick, "server" or "here".
    pub(super) fn set_run(&mut self, run: &str, cx: &mut Context<Self>) {
        if let Some(link) = &mut self.crew.link {
            link.cfg.run = run.to_string();
            remote::set_run(run);
            let label = link.cfg.label();
            self.say(match run {
                "server" => format!("Jobs run on {label} from now on."),
                "here" => "Jobs run on this computer from now on.".to_string(),
                _ => format!("Squad picks between here and {label} for each job."),
            });
        }
        cx.notify();
    }

    fn connect_server(&mut self, cx: &mut Context<Self>) {
        if self.server_setup.is_some() {
            return;
        }
        let host = self.server_input.read(cx).value().trim().to_string();
        let password = self.server_pass.read(cx).value().to_string();
        let new_password = if self.server_wants_new_pass {
            self.server_new_pass.read(cx).value().to_string()
        } else {
            String::new()
        };
        if self.server_wants_new_pass && new_password.chars().count() < 8 {
            self.server_log
                .push("Pick a new password of at least 8 characters.".into());
            self.server_failed = true;
            cx.notify();
            return;
        }
        self.server_log.clear();
        self.server_failed = false;
        self.server_key_changed = false;
        self.server_wants_new_pass = false;
        if !remote::valid_host(&host) {
            self.server_log.push(
                "Type the server as user@address, like root@203.0.113.7. Your VPS provider's email has both.".into(),
            );
            self.server_failed = true;
            cx.notify();
            return;
        }
        self.server_setup = Some(remote::connect(
            host,
            password,
            new_password,
            remote::device_name(),
        ));
        cx.notify();
    }

    fn brain_busy(&self) -> bool {
        self.server_brain.is_some() || self.server_sign_in.is_some()
    }

    fn start_claude_sign_in(&mut self, cx: &mut Context<Self>) {
        if self.brain_busy() {
            return;
        }
        let Some(link) = &self.crew.link else {
            return;
        };
        self.server_brain_log.clear();
        self.server_brain_failed = false;
        self.server_sign_in = Some(remote::claude_sign_in(link.cfg.clone()));
        cx.notify();
    }

    fn finish_claude_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.server_code.read(cx).value().trim().to_string();
        if code.is_empty() {
            return;
        }
        if let Some(s) = &self.server_sign_in {
            s.send_code(&code);
        }
        self.server_code
            .update(cx, |i, cx| i.set_value("", window, cx));
        cx.notify();
    }

    fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.brain_busy() {
            return;
        }
        let Some(link) = &self.crew.link else {
            return;
        };
        let value = self.server_key.read(cx).value().trim().to_string();
        if value.is_empty() {
            return;
        }
        let (_, agent, env) = remote::KEYS[self.server_key_pick.min(remote::KEYS.len() - 1)];
        self.server_brain_log.clear();
        self.server_brain_failed = false;
        self.server_brain = Some(remote::give_key(link.cfg.clone(), agent, env, value));
        self.server_key
            .update(cx, |i, cx| i.set_value("", window, cx));
        cx.notify();
    }

    pub(super) fn settings_server(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.crew.link {
            None => self.server_connect_card(cx),
            Some(_) => self.server_connected(cx),
        };
        Self::page(
            "settings-server",
            "Servers",
            "Jobs that run with the lid closed, on your own Linux server. Squad keeps it locked down and reaches it over SSH.",
            body,
        )
    }

    fn server_connect_card(&mut self, cx: &mut Context<Self>) -> Div {
        let busy = self.server_setup.is_some();
        let fields = div()
            .mx(px(14.))
            .pt(px(2.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::M))
                    .child(field(&self.server_input))
                    .child(field(&self.server_pass)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::M))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child("The password is only for the first connection, if you don't use SSH keys. Squad swaps it for a key of its own and never saves it."),
                    )
                    .child(
                        ui::dbtn_i(
                            "server-connect",
                            IconName::Server,
                            if busy { "Setting up…" } else { "Connect" },
                            ui::Btn::Primary,
                        )
                        .when(busy, |d| d.opacity(0.6))
                        .on_click(cx.listener(|this, _, _, cx| this.connect_server(cx))),
                    ),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(
                card("YOUR SERVER")
                    .child(fields)
                    .when(!self.server_log.is_empty(), |d| {
                        d.child(log(&self.server_log, self.server_failed))
                    })
                    .when(self.server_wants_new_pass && !busy, |d| {
                        d.child(
                            div()
                                .px(px(18.))
                                .pb(px(14.))
                                .flex()
                                .items_center()
                                .gap(px(space::M))
                                .child(field(&self.server_new_pass))
                                .child(
                                    ui::solid_i(
                                        "server-new-pass",
                                        IconName::ShieldCheck,
                                        "Change it and connect",
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| this.connect_server(cx))),
                                ),
                        )
                    })
                    .when(self.server_key_changed && !busy, |d| {
                        d.child(
                            div().px(px(18.)).pb(px(14.)).flex().child(
                                ui::ghost_i(
                                    "server-trust",
                                    IconName::ShieldCheck,
                                    "I rebuilt it: trust the new key and connect",
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let host = this.server_input.read(cx).value().trim().to_string();
                                    remote::trust_new_key(&host);
                                    this.connect_server(cx);
                                })),
                            ),
                        )
                    }),
            )
            .child(
                card("WHAT CONNECTING DOES")
                    .child(line_row(
                        "Installs Squad Code there",
                        "Into ~/.local/bin, and keeps it running as a service that comes back after a restart. Claude Code comes with it.",
                    ))
                    .child(line_row(
                        "Locks the server down",
                        "SSH takes keys only, a firewall lets in nothing but SSH, fail2ban blocks password guessing, and security updates install themselves. Squad itself listens only on the server's own address, so there's no web port to attack; this app reaches it through an encrypted SSH tunnel.",
                    ))
                    .child(line_row(
                        "Pairs this computer",
                        "With a one-time code fetched over SSH, so there's nothing to copy.",
                    ))
                    .child(line_row(
                        "Needs",
                        "A Linux VPS (Ubuntu is easiest) and its address and password (root, or ubuntu on many Ubuntu servers) from your provider's email, or SSH access with your own key.",
                    )),
            )
    }

    fn server_connected(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(link) = &self.crew.link else {
            return div();
        };
        let label = link.cfg.label();
        let host = link.cfg.host.clone();
        let up = link.up();
        let logins = link.logins();
        let projects = link.projects();
        let here = self.crew.store.folder().cloned();
        // The open project, when the server doesn't have it yet: put there
        // through git, so it needs no GitHub.
        let copy = here.and_then(|f| {
            let url = super::ship::remote_url(&f.path).unwrap_or_default();
            link.path_for(&f, &url)
                .is_none()
                .then_some((f.name.clone(), f.path.clone()))
        });
        let status = action_row(
            host.clone(),
            link.line(),
            div()
                .flex()
                .gap(px(8.))
                .child(
                    ui::ghost_i("server-open-page", IconName::Activity, "See and control it")
                        .on_click(cx.listener(|this, _, _, cx| this.go(super::Place::Server, cx))),
                )
                .child(
                    ui::ghost("server-disconnect", "Disconnect").on_click(cx.listener(
                        |this, _, _, cx| {
                            remote::forget();
                            this.crew.link = None;
                            this.server_brain_log.clear();
                            this.server_look = None;
                            this.say(
                                "Disconnected. The server keeps running; connect again any time.",
                            );
                            cx.notify();
                        },
                    )),
                ),
        );
        let names = if projects.is_empty() {
            "None yet. Jobs only run there on projects it has.".to_string()
        } else {
            projects
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let copy_button = copy.filter(|_| up).map(|(name, path)| {
            ui::ghost_i(
                "server-copy",
                IconName::CloudUpload,
                format!("Set {name} up there"),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(link) = &this.crew.link {
                    link.copy_project(&name, &path);
                    this.say(format!("Setting {name} up on the server through git…"));
                }
                cx.notify();
            }))
        });
        let projects_row = action_row("Projects there", names, div().children(copy_button));

        // Where jobs run: the person's call, or Squad's.
        let run = link.cfg.run.clone();
        let choice = |id: &'static str, icon: IconName, label: String, value: &'static str| {
            ui::pill_i(id, icon, label, run == value).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.set_run(value, cx);
                },
            ))
        };
        let run_row = div()
            .px(px(18.))
            .py(px(14.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .border_t_1()
            .border_color(c(ui::line2()))
            .child(div().text_size(px(13.5)).child("Where jobs run"))
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child(match run.as_str() {
                        "server" => format!("Every job runs on {label}. When it doesn't have the project yet, Squad sets it up there through git first."),
                        "here" => "Every job runs on this computer. The server waits until you switch.".to_string(),
                        _ => format!("Squad picks: long work goes to {label}, quick work stays here. Each job says where it ran."),
                    }),
            )
            .child(
                div()
                    .flex()
                    .self_start()
                    .gap(px(2.))
                    .p(px(3.))
                    .rounded(px(9.))
                    .bg(c(ui::hover()))
                    .child(choice("run-auto", IconName::Sparkles, "Squad picks".into(), ""))
                    .child(choice("run-server", IconName::Cloud, format!("Always {label}"), "server"))
                    .child(choice("run-here", IconName::Laptop, "Always this computer".into(), "here")),
            );

        // What runs the jobs there.
        let ready: Vec<String> = logins
            .iter()
            .filter(|l| l.installed && l.signed_in)
            .map(|l| l.name.clone())
            .collect();
        let ready_line = if !up {
            "Shown once the server answers.".to_string()
        } else if ready.is_empty() {
            "Nothing yet. Sign in with your Claude plan, or give it an API key.".to_string()
        } else {
            format!("Ready there: {}.", ready.join(", "))
        };
        let claude_ready = logins
            .iter()
            .any(|l| l.agent == "claude" && l.installed && l.signed_in);
        let busy = self.brain_busy();
        let sign_in = action_row(
            "Your Claude plan",
            if claude_ready {
                "Claude Code there is signed in."
            } else {
                "Claude Pro or Max: sign in once in your browser, and the server uses your plan."
            },
            ui::ghost_i(
                "server-claude",
                IconName::LogIn,
                if self.server_sign_in.is_some() {
                    "Cancel sign-in"
                } else if claude_ready {
                    "Sign in again"
                } else {
                    "Sign in with Claude"
                },
            )
            .when(busy && self.server_sign_in.is_none(), |d| d.opacity(0.6))
            .on_click(cx.listener(|this, _, _, cx| {
                // While it's signing in, the button cancels.
                if this.server_sign_in.take().is_some() {
                    this.server_url = None;
                    this.server_brain_log.push("Cancelled.".into());
                    cx.notify();
                } else {
                    this.start_claude_sign_in(cx);
                }
            })),
        );
        let code_row = self.server_url.clone().map(|url| {
            div()
                .px(px(18.))
                .pb(px(14.))
                .flex()
                .items_center()
                .gap(px(space::M))
                .child(field(&self.server_code))
                .child(ui::solid("server-code", "Finish").on_click(
                    cx.listener(|this, _, window, cx| this.finish_claude_sign_in(window, cx)),
                ))
                .child(
                    ui::ghost("server-reopen", "Open the link again")
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
        });
        let pick = self.server_key_pick;
        let pills = div()
            .flex()
            .gap(px(6.))
            .children(remote::KEYS.iter().enumerate().map(|(i, (company, _, _))| {
                ui::pill(("server-key-pick", i), *company, i == pick).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.server_key_pick = i;
                        cx.notify();
                    },
                ))
            }));
        let key_row = div()
            .px(px(18.))
            .py(px(14.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .border_t_1()
            .border_color(c(ui::line2()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(ui::ink()))
                            .child("An API key"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(ui::muted()))
                            .child("Paid by use instead of a plan. Squad installs the agent that uses it and keeps the key on the server, readable only by you."),
                    ),
            )
            .child(pills)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::M))
                    .child(field(&self.server_key))
                    .child(
                        ui::solid("server-key-save", "Save key")
                            .when(busy, |d| d.opacity(0.6))
                            .on_click(cx.listener(|this, _, window, cx| this.save_key(window, cx))),
                    ),
            );
        let mut brain = card("WHAT RUNS THE JOBS THERE")
            .child(line_row("Agents", ready_line))
            .child(sign_in)
            .children(code_row)
            .child(key_row);
        if !self.server_brain_log.is_empty() {
            brain = brain.child(
                div()
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(c(ui::line2()))
                    .child(log(&self.server_brain_log, self.server_brain_failed)),
            );
        }
        let desktop = self.desktop_card(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(card("YOUR SERVER").child(status).child(projects_row).child(run_row))
            .children(desktop)
            .child(brain)
            .child(card("HOW JOBS PICK A SIDE").child(line_row(
                format!("Here or on {label}"),
                "When Squad picks: a job runs where its project is, and when both sides have it, long work (big rewrites, migrations, overnight jobs) goes to the server and the rest stays here. Choose Always above to decide yourself. Each job's feed says where it ran and why, and if the server can't take it, it runs here.",
            ))
            .child(line_row(
                "How work comes back",
                "Like the cloud coding tools, through git, never by copying files: each job gets its own branch, made here at the commit you're on and handed to the server. When you approve, that branch comes back here and merges like any job (or becomes a pull request). Your own branches are never pushed, and it all goes over the same SSH, so the server needs no GitHub login. Changes you haven't committed, and files git ignores like .env, stay here.",
            )))
    }
}
