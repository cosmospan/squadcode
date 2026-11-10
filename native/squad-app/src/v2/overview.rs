//! A project's Overview: the things that belong to the project rather than
//! to one job. Running the app, how every job sets up (`squad.toml`, as a
//! form), how work runs here, what happened lately, and Git.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::project::{self, Project};
use super::store::{self, Job, Status};
use super::ui::{self, c};
use super::{Place, Shell};

/// The Overview's left column stops growing here.
const MAX_W: f32 = 1180.;
/// The Git column on the right.
const GIT_W: f32 = 380.;

/// The form for `squad.toml`, one line per key. Lists are comma-separated.
pub(super) struct ProjectForm {
    setup: Entity<InputState>,
    copy: Entity<InputState>,
    run: Entity<InputState>,
    url: Entity<InputState>,
    port: Entity<InputState>,
    checks: Entity<InputState>,
    /// The form is open.
    pub(super) editing: bool,
    /// Why the last save didn't happen.
    error: Option<String>,
}

impl ProjectForm {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let mut make =
            |hint: &'static str| cx.new(|cx| InputState::new(window, cx).placeholder(hint));
        Self {
            setup: make("pnpm install"),
            copy: make(".env.local"),
            run: make("pnpm dev --port $PORT"),
            url: make("http://localhost:$PORT"),
            port: make("3100"),
            checks: make("pnpm lint, pnpm test"),
            editing: false,
            error: None,
        }
    }

    /// Fills the form from `p`.
    fn fill(&self, p: &Project, window: &mut Window, cx: &mut App) {
        let set = |input: &Entity<InputState>, text: String, window: &mut Window, cx: &mut App| {
            input.update(cx, |i, cx| i.set_value(text, window, cx));
        };
        set(&self.setup, p.setup.join(", "), window, cx);
        set(&self.copy, p.copy.join(", "), window, cx);
        set(&self.run, p.run.clone(), window, cx);
        set(&self.url, p.url.clone(), window, cx);
        set(
            &self.port,
            if p.port == 0 {
                String::new()
            } else {
                p.port.to_string()
            },
            window,
            cx,
        );
        set(&self.checks, p.checks.join(", "), window, cx);
    }

    /// What the form says, or why it can't be saved.
    fn read(&self, cx: &App) -> Result<Project, String> {
        let text = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();
        let port = text(&self.port);
        let port = if port.is_empty() {
            0
        } else {
            port.parse::<u16>().map_err(|_| {
                format!("The port should be a number like 3100, not \u{201c}{port}\u{201d}.")
            })?
        };
        Ok(Project {
            setup: project::split_list(&text(&self.setup)),
            copy: project::split_list(&text(&self.copy)),
            run: text(&self.run),
            url: text(&self.url),
            port,
            checks: project::split_list(&text(&self.checks)),
        })
    }
}

/// What a job did, in a few words, for the activity list.
pub(super) fn did(j: &Job) -> String {
    match j.status {
        Status::Running => "is working on".into(),
        Status::Review => "finished".into(),
        Status::Done if j.needs_you() => "finished".into(),
        Status::Done => "finished".into(),
        Status::Shipping if j.pr_number > 0 => format!("opened PR #{} for", j.pr_number),
        Status::Shipping => "is shipping".into(),
        Status::Shipped => "shipped".into(),
        Status::Failed => "failed on".into(),
        Status::Stopped => "was stopped on".into(),
        Status::Interrupted => "was interrupted on".into(),
        s => format!("{}:", s.label().to_lowercase()),
    }
}

/// A card's title, in small mono capitals like the design, and its quiet
/// line under it.
fn card_head(title: &str, sub: Option<String>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .font_family(ui::MONO)
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::faint()))
                .child(title.to_uppercase()),
        )
        .when_some(sub, |d, s| {
            d.child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child(s),
            )
        })
}

/// One of the Overview's cards.
fn panel() -> Div {
    ui::card().p(px(18.)).flex().flex_col().gap(px(14.))
}

/// A key and its value in the setup card.
fn field(key: &str, value: Div) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .py(px(6.))
        .border_b_1()
        .border_color(c(ui::line2()))
        .child(
            div()
                .w(px(70.))
                .flex_none()
                .font_family(ui::MONO)
                .text_size(px(12.))
                .text_color(c(ui::faint()))
                .child(key.to_lowercase()),
        )
        .child(div().flex_1().min_w_0().flex().justify_end().child(value))
}

/// A value in mono, or a quiet dash when there's none.
fn code(text: String) -> Div {
    if text.is_empty() {
        div()
            .text_size(px(12.5))
            .text_color(c(ui::faint()))
            .child("\u{2014}")
    } else {
        div()
            .font_family(ui::MONO)
            .text_size(px(12.5))
            .text_color(c(ui::ink()))
            .child(text)
    }
}

impl Shell {
    /// The Overview of the project you're in.
    pub(super) fn overview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(folder) = self.crew.store.folder().cloned() else {
            return self.empty_place(
                "No project yet",
                "Pick a folder and its Overview shows how it runs, its Git and what the crew did lately.",
                cx,
            );
        };
        let here = folder.path.clone();
        let jobs: Vec<Job> = self.crew.store.listed().into_iter().cloned().collect();

        // The project's own line: its branch and how clean it is.
        let repo = self.crew.repos.get(&here).cloned();
        let mut sub = vec![super::sidebar::short_path(&here)];
        if let Some(r) = repo.as_ref().filter(|r| r.is_repo) {
            if !r.state.branch.is_empty() {
                sub.push(r.state.branch.clone());
            }
            sub.push(match r.changed {
                0 => "clean".into(),
                1 => "1 file changed".into(),
                n => format!("{n} files changed"),
            });
        }
        // The header bar: the project, its line, and Reveal and Terminal.
        let reveal_path = std::path::PathBuf::from(&here);
        let head = div()
            .h(px(64.))
            .flex_none()
            .px(px(20.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(19.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(folder.name.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child(sub.join(" \u{b7} ")),
                    ),
            )
            .child(
                ui::dbtn_i("ov-reveal", IconName::FolderOpen, "Reveal", ui::Btn::Ghost)
                    .on_click(cx.listener(move |_, _, _, cx| cx.reveal_path(&reveal_path))),
            )
            .child(
                ui::dbtn_i(
                    "ov-term",
                    IconName::SquareTerminal,
                    "Terminal",
                    ui::Btn::Plain,
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_project_term(window, cx))),
            );

        let run = self.overview_run(&here, cx);
        let setup = self.overview_setup(&here, cx);
        let work = self.overview_work(&folder, cx);
        let lately = self.overview_lately(&folder.name, &jobs, cx);
        let issues = self.overview_issues(&here, &jobs, cx);
        // Git down the right, as a side panel with its own header.
        let git = div()
            .w(px(GIT_W))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(c(ui::panel()))
            .border_l_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .h(px(50.))
                    .flex_none()
                    .px(px(18.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(c(ui::line()))
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(ui::ink()))
                    .child("GIT"),
            )
            .child(
                div()
                    .id("overview-git")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(18.))
                    .pb(px(24.))
                    .child(self.git_sections(&here, &jobs, cx)),
            );

        div()
            .size_full()
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(head)
                    .child(
                        div()
                            .id("overview")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(24.))
                            .pt(px(20.))
                            .pb(px(40. + super::DOCK_ROOM))
                            .child(
                                div()
                                    .max_w(px(MAX_W))
                                    .flex()
                                    .flex_col()
                                    .gap(px(14.))
                                    .child(run)
                                    .child(
                                        div()
                                            .flex()
                                            .items_start()
                                            .gap(px(14.))
                                            .child(setup.flex_1().min_w_0())
                                            .child(work.flex_1().min_w_0()),
                                    )
                                    .child(lately)
                                    .child(issues),
                            ),
                    ),
            )
            .child(git)
            .into_any_element()
    }

    /// Run the app: whether it's up and where, its command, and the buttons.
    fn overview_run(&mut self, here: &str, cx: &mut Context<Self>) -> Div {
        let found = super::work::start_command(here, 0);
        let file = project::load(here);
        let app = self
            .runs
            .iter()
            .find(|r| r.spec.folder == here && r.spec.job.is_none() && !r.spec.setup && r.live())
            .map(|r| {
                (
                    r.id,
                    r.url().map(String::from),
                    r.state(),
                    r.spec.cmd.clone(),
                )
            });
        let copies = self
            .runs
            .iter()
            .filter(|r| r.spec.folder == here && r.spec.job.is_some() && r.live())
            .count();

        let state = app.as_ref().map(|(_, url, st, _)| {
            let port = url
                .as_deref()
                .and_then(|u| u.rsplit(':').next())
                .map(|p| p.trim_end_matches('/').to_string());
            match st {
                super::apps::State::Up => (
                    match port {
                        Some(p) => format!("up on :{p}"),
                        None => "up".into(),
                    },
                    ui::ok_tone(),
                ),
                super::apps::State::Starting => ("starting".into(), ui::ask_tone()),
                _ => ("running".into(), ui::run_tone()),
            }
        });
        let mut buttons = div().ml_auto().flex().items_center().gap(px(8.));
        match &app {
            Some((id, url, _, _)) => {
                let id = *id;
                buttons = buttons
                    .child(
                        ui::ghost_i("ov-restart", IconName::RotateCw, "Restart")
                            .on_click(cx.listener(move |this, _, _, cx| this.restart_run(id, cx))),
                    )
                    .child(
                        ui::ghost_i("ov-stop", IconName::Square, "Stop")
                            .on_click(cx.listener(move |this, _, _, cx| this.stop_run(id, cx))),
                    )
                    .when_some(url.clone(), |d, u| {
                        d.child(
                            ui::solid_i("ov-open", IconName::ExternalLink, "Open")
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&u))),
                        )
                    });
            }
            None if found.is_some() => {
                buttons = buttons.child(ui::solid_i("ov-run", IconName::Play, "Run").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.run_app(None, None, cx);
                    }),
                ));
            }
            None => {}
        }
        let cmd = app
            .as_ref()
            .map(|a| a.3.clone())
            .or_else(|| found.as_ref().map(|f| f.0.clone()));
        let note = match (&file, &found) {
            (_, None) => "Squad can't tell how to start this app yet. Add a run command to how every job sets up, below.".to_string(),
            (Some(p), Some(_)) if p.port > 0 => {
                let mut s = format!("Each job's copy gets its own port from {}.", p.port);
                match copies {
                    0 => {}
                    1 => s.push_str(" 1 job copy running."),
                    n => s.push_str(&format!(" {n} job copies running.")),
                }
                s
            }
            (Some(_), Some(_)) => format!("From {}.", project::FILE),
            (None, Some(_)) => format!(
                "Squad worked this out from the project. Save it in {} to change it.",
                project::FILE
            ),
        };
        panel()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(card_head("Run the app", None))
                    .when_some(state, |d, (word, tone)| {
                        d.child(super::home::pill(&word, tone))
                    })
                    .child(buttons),
            )
            .when_some(cmd, |d, cmd| {
                d.child(
                    div()
                        .px(px(12.))
                        .py(px(9.))
                        .rounded(px(6.))
                        .bg(c(ui::well()))
                        .font_family(ui::MONO)
                        .text_size(px(12.5))
                        .text_color(c(ui::ink()))
                        .child(cmd),
                )
            })
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::muted()))
                    .child(note),
            )
    }

    /// How every job sets up: `squad.toml`, read, or as a form to edit.
    fn overview_setup(&mut self, here: &str, cx: &mut Context<Self>) -> Div {
        let read = project::read(here);
        let (file, broken) = match &read {
            Ok(p) => (p.clone(), None),
            Err(why) => (None, Some(why.clone())),
        };
        let sub = match (&file, &broken) {
            (_, Some(_)) => format!("{} has a mistake", project::FILE),
            (Some(_), None) => format!("{} \u{b7} in the project", project::FILE),
            (None, None) => format!("No {} yet \u{b7} Squad works these out", project::FILE),
        };
        let mut card = panel().child(card_head("Setup", Some(sub)));
        if self.proj_form.editing {
            let row = |key: &str, input: &Entity<InputState>| {
                field(
                    key,
                    div()
                        .h(px(30.))
                        .px(px(2.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .bg(c(ui::raised()))
                        .border_1()
                        .border_color(c(ui::line()))
                        .font_family(ui::MONO)
                        .text_size(px(12.5))
                        .child(Input::new(input).appearance(false).bordered(false)),
                )
            };
            let f = &self.proj_form;
            card = card
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(row("Setup", &f.setup))
                        .child(row("Copy", &f.copy))
                        .child(row("Run", &f.run))
                        .child(row("Address", &f.url))
                        .child(row("Port", &f.port))
                        .child(row("Checks", &f.checks)),
                )
                .child(div().text_size(px(12.)).text_color(c(ui::faint())).child(
                    "Separate several commands or files with commas. $PORT is each job's own port.",
                ))
                .when_some(f.error.clone(), |d, e| {
                    d.child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(ui::t_red()))
                            .child(e),
                    )
                });
            let dir = here.to_string();
            card = card.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(ui::solid("ov-save", "Save").on_click(cx.listener(
                        move |this, _, _, cx| {
                            match this
                                .proj_form
                                .read(cx)
                                .and_then(|p| project::write(&dir, &p))
                            {
                                Ok(()) => {
                                    this.proj_form.editing = false;
                                    this.proj_form.error = None;
                                }
                                Err(why) => this.proj_form.error = Some(why),
                            }
                            cx.notify();
                        },
                    )))
                    .child(ui::ghost("ov-cancel", "Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.proj_form.editing = false;
                            this.proj_form.error = None;
                            cx.notify();
                        },
                    ))),
            );
            return card;
        }
        if let Some(why) = broken {
            card = card.child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::t_red()))
                    .child(why),
            );
        }
        // What jobs get now: the file's keys, or what Squad found.
        let shown = file.clone().unwrap_or_else(|| {
            let (run, url) = super::work::detect_start(here).unwrap_or_default();
            Project {
                run,
                url: url.unwrap_or_default(),
                checks: super::work::detect_checks(here),
                ..Project::default()
            }
        });
        let start = shown.clone();
        let dir = here.to_string();
        card.child(
            div()
                .flex()
                .flex_col()
                .child(field("Setup", code(shown.setup.join(", "))))
                .child(field("Copy", code(shown.copy.join(", "))))
                .child(field("Run", code(shown.run.clone())))
                .when(shown.port > 0, |d| {
                    d.child(field("Ports", code(format!("from {}", shown.port))))
                })
                .child(field("Checks", code(shown.checks.join(", ")))),
        )
        .child(
            div().flex().child(
                ui::ghost_i(
                    "ov-edit",
                    IconName::Pencil,
                    if file.is_some() { "Edit" } else { "Set up" },
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    let _ = &dir;
                    this.proj_form.fill(&start, window, cx);
                    this.proj_form.editing = true;
                    this.proj_form.error = None;
                    cx.notify();
                })),
            ),
        )
    }

    /// How work runs here: how often the crew checks with you, where jobs
    /// run and what Approve does.
    fn overview_work(&mut self, folder: &store::Folder, cx: &mut Context<Self>) -> Div {
        let st = cx.global::<crate::settings::AppSettings>().clone();
        let here = if cfg!(target_os = "macos") {
            "This Mac"
        } else {
            "This computer"
        };
        let level = match (st.v2_plan_first, st.v2_auto_merge) {
            (true, _) => "Ask before anything",
            (false, true) => "Merge when checks pass",
            (false, false) => "Ask before merging",
        }
        .to_string();
        let where_ = if self.crew.link.is_some() {
            format!("{here} or your server")
        } else {
            here.to_string()
        };
        let ship = match folder.ship.as_str() {
            "pr" => "Pull request",
            "merge" => "Merge here",
            _ => "Pull request or merge",
        }
        .to_string();
        panel()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(card_head("How work runs here", None))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("ov-work")
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::accent_text()))
                            .cursor_pointer()
                            .child("Change")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.go(Place::Settings(super::prefs::Pane::Work), cx)
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(field("level", code(level)))
                    .child(field("where", code(where_)))
                    .child(field("ship", code(ship)))
                    .child(field("seatbelt", code("On \u{b7} secrets, .env".into()))),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c(ui::faint()))
                    .child("Same as your defaults in Settings."),
            )
    }

    /// The project's open issues, each one a click from a job, what the crew
    /// closed this week, and whether pull requests close their issues.
    fn overview_issues(&mut self, here: &str, jobs: &[Job], cx: &mut Context<Self>) -> Div {
        let demo = self.crew.store.demo;
        let stale = self.issue_list.as_ref().is_none_or(|l| {
            l.folder != here || (!l.loading && l.at.elapsed() > std::time::Duration::from_secs(300))
        });
        if stale {
            self.read_issues(here.to_string(), demo, cx);
        }
        let close = cx.global::<crate::settings::AppSettings>().v2_close_issues;
        let quiet = |text: String| {
            div()
                .py(px(6.))
                .text_size(px(12.5))
                .text_color(c(ui::faint()))
                .child(text)
        };
        let (loading, error, items) = match self.issue_list.as_ref() {
            Some(l) => (l.loading, l.error.clone(), l.items.clone()),
            None => (true, None, Vec::new()),
        };
        let mut rows = div().flex().flex_col();
        if items.is_empty() {
            rows = rows.child(quiet(match (loading, error) {
                (true, _) => "Reading the open issues…".to_string(),
                (false, Some(e)) => e,
                (false, None) => "No open issues.".to_string(),
            }));
        }
        for (i, it) in items.iter().take(8).enumerate() {
            let n = it.number;
            let closes = format!("Closes #{n}");
            let taken = jobs.iter().find(|j| {
                j.issue == closes
                    && !matches!(
                        j.status,
                        Status::Failed | Status::Stopped | Status::Interrupted
                    )
            });
            let mut row = div()
                .h(px(36.))
                .flex()
                .items_center()
                .gap(px(10.))
                .border_t_1()
                .border_color(c(ui::line2()))
                .text_size(px(13.))
                .child(
                    div()
                        .w(px(40.))
                        .flex_none()
                        .font_family(ui::MONO)
                        .text_size(px(11.5))
                        .text_color(c(ui::faint()))
                        .child(format!("#{n}")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(c(ui::ink()))
                        .child(it.title.clone()),
                );
            for label in it.labels.iter().take(2) {
                row = row.child(
                    div()
                        .flex_none()
                        .px(px(6.))
                        .rounded(px(4.))
                        .bg(c(ui::well()))
                        .text_size(px(11.))
                        .text_color(c(ui::muted()))
                        .child(label.clone()),
                );
            }
            row = match taken {
                Some(j) => {
                    let id = j.id;
                    row.child(
                        ui::ghost(
                            ("ov-issue-job", i),
                            format!("#{id} \u{b7} {}", j.bot.name()),
                        )
                        .h(px(24.))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
                    )
                }
                None => row.child(
                    ui::ghost(("ov-issue-fix", i), "Give to the crew")
                        .h(px(24.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.go(Place::Today, cx);
                            this.hand_over(format!("Fix issue {n}"), cx);
                        })),
                ),
            };
            rows = rows.child(row);
        }
        // Closed by the crew: shipped jobs that named an issue, this week.
        let now = store::now();
        let closed: Vec<&Job> = jobs
            .iter()
            .filter(|j| {
                !j.issue.is_empty()
                    && j.status == Status::Shipped
                    && j.ended.is_some_and(|e| now.saturating_sub(e) < 7 * 86_400)
            })
            .collect();
        if !closed.is_empty() {
            rows = rows.child(
                div()
                    .pt(px(12.))
                    .pb(px(4.))
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::muted()))
                    .child("Closed by the crew this week"),
            );
        }
        for j in closed {
            let id = j.id;
            let face = self.avatar(j.bot, 18.);
            let key = j
                .issue
                .split_once(' ')
                .map(|(_, k)| k.to_string())
                .unwrap_or_default();
            let tail = if j.pr_number > 0 {
                format!("{key} \u{b7} PR #{}", j.pr_number)
            } else {
                key
            };
            rows = rows.child(
                div()
                    .id(("ov-closed", id as usize))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .child(face)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(c(ui::body()))
                            .child(super::channel::card_title(j)),
                    )
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .text_color(c(ui::t_green()))
                            .child(tail),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
            );
        }
        let sub = if close {
            "Open on GitHub. A job's pull request closes its issue when it merges."
        } else {
            "Open on GitHub. Pull requests link their issue and leave it open."
        };
        panel()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(card_head("Issues", Some(sub.to_string())))
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child("Close on merge")
                            .child(
                                div()
                                    .id("ov-close-issues")
                                    .cursor_pointer()
                                    .child(super::prefs::switch(close))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        super::issues::set_close_on_merge(!close);
                                        cx.update_global::<crate::settings::AppSettings, _>(
                                            |st, _| {
                                                st.v2_close_issues = !close;
                                                st.save();
                                            },
                                        );
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(rows)
    }

    /// Reads the project's open issues off the UI thread; the demo has its own.
    fn read_issues(&mut self, folder: String, demo: bool, cx: &mut Context<Self>) {
        let keep = self
            .issue_list
            .take()
            .filter(|l| l.folder == folder)
            .map(|l| l.items)
            .unwrap_or_default();
        self.issue_list = Some(super::IssueList {
            folder: folder.clone(),
            at: std::time::Instant::now(),
            loading: true,
            items: keep,
            error: None,
        });
        let task = cx.background_spawn(async move {
            if demo {
                return Ok(super::demo::issues());
            }
            super::issues::list_open(&folder)
        });
        cx.spawn(async move |this, cx| {
            let got = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(l) = this.issue_list.as_mut() {
                    l.loading = false;
                    l.at = std::time::Instant::now();
                    match got {
                        Ok(items) => {
                            l.items = items;
                            l.error = None;
                        }
                        Err(e) => l.error = Some(e),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Lately in the project: the latest jobs, newest first.
    fn overview_lately(&mut self, _name: &str, jobs: &[Job], cx: &mut Context<Self>) -> Div {
        let mut recent: Vec<&Job> = jobs.iter().collect();
        recent.sort_by_key(|j| std::cmp::Reverse(j.ended.unwrap_or(j.started)));
        recent.truncate(6);
        let mut list = div().flex().flex_col();
        if recent.is_empty() {
            list = list.child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child("Nothing yet. Jobs show here as the crew takes them on."),
            );
        }
        for j in recent {
            let id = j.id;
            let face = self.avatar(j.bot, 20.);
            list = list.child(
                div()
                    .id(("ov-lately", id as usize))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .mx(px(-8.))
                    .h(px(32.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .text_size(px(13.))
                    .child(face)
                    .child(div().flex_none().text_color(c(ui::muted())).child(format!(
                        "{} {}",
                        j.bot.name(),
                        did(j)
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(super::channel::card_title(j)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child(store::ago(j.ended.unwrap_or(j.started))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
            );
        }
        panel()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(card_head("Lately", None))
                    .child(
                        div().ml_auto().child(
                            ui::ghost("ov-all-jobs", "All jobs").on_click(
                                cx.listener(|this, _, _, cx| this.go(Place::History, cx)),
                            ),
                        ),
                    ),
            )
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::{Job, Status};
    use super::did;

    #[test]
    fn what_a_job_did_reads_plainly() {
        let mut j = Job {
            status: Status::Shipped,
            ..Job::default()
        };
        assert_eq!(did(&j), "shipped");
        j.status = Status::Shipping;
        j.pr_number = 31;
        assert_eq!(did(&j), "opened PR #31 for");
        j.status = Status::Failed;
        assert_eq!(did(&j), "failed on");
    }
}
