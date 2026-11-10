//! Machines: where jobs run. This computer and your server as a list, and
//! the one you pick on the right: what runs there and its projects, and for
//! the server, how it's doing and its controls.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::prefs::Pane;
use super::store::Status;
use super::ui::{self, c};
use super::{Place, Shell};

/// The list's width.
const LIST_W: f32 = 280.;

/// This computer's name in the list.
pub(super) fn here_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "This Mac"
    } else {
        "This computer"
    }
}

/// "1 job", "3 jobs".
fn jobs_word(n: usize) -> String {
    match n {
        1 => "1 job".into(),
        n => format!("{n} jobs"),
    }
}

impl Shell {
    /// The Machines page.
    pub(super) fn machines(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let running_here = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.status == Status::Running && j.remote_on.is_empty())
            .count();
        let running_there = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.status == Status::Running && !j.remote_on.is_empty())
            .count();
        let server = self.crew.link.as_ref().map(|l| (l.cfg.label(), l.up()));
        // With no server the server's row is "Add a server", which opens
        // Settings rather than a page here.
        let pick = if server.is_some() {
            self.machine_pick
        } else {
            0
        };

        let item = |id: &'static str, on: bool, dot: u32, name: String, line: String| {
            div()
                .id(id)
                .px(px(12.))
                .py(px(9.))
                .flex()
                .items_start()
                .gap(px(10.))
                .rounded(px(8.))
                .border_1()
                .cursor_pointer()
                .map(|d| {
                    if on {
                        d.relative()
                            .bg(c(ui::raised()))
                            .border_color(c(ui::line()))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(-1.))
                                    .top(px(-1.))
                                    .bottom(px(-1.))
                                    .w(px(4.))
                                    .rounded_l(px(8.))
                                    .bg(c(ui::pick_fill())),
                            )
                    } else {
                        d.border_color(transparent_black())
                            .hover(|s| s.bg(c(ui::hover())))
                    }
                })
                .child(div().mt(px(6.)).size(px(7.)).flex_none().bg(c(dot)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(ui::ink()))
                                .child(name),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(c(ui::muted()))
                                .child(line),
                        ),
                )
        };
        let mut list = div()
            .id("machines-list")
            .w(px(LIST_W))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .px(px(10.))
            .pt(px(14.))
            .pb(px(24. + super::DOCK_ROOM))
            .flex()
            .flex_col()
            .gap(px(4.))
            .border_r_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .px(px(8.))
                    .pt(px(8.))
                    .pb(px(12.))
                    .text_size(px(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child("Machines"),
            )
            .child(
                item(
                    "machine-here",
                    pick == 0,
                    ui::t_green(),
                    here_name().to_string(),
                    format!("{} \u{b7} {}", os_name(), jobs_word(running_here)),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.machine_pick = 0;
                    cx.notify();
                })),
            );
        list = match server.clone() {
            Some((label, up)) => list.child(
                item(
                    "machine-server",
                    pick == 1,
                    if up { ui::t_green() } else { ui::t_amber() },
                    label,
                    format!(
                        "{} \u{b7} {}",
                        if up { "Connected" } else { "Not reachable" },
                        jobs_word(running_there)
                    ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.machine_pick = 1;
                    cx.notify();
                })),
            ),
            None => list.child(
                div()
                    .id("machine-add")
                    .px(px(12.))
                    .h(px(40.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_dashed()
                    .border_color(c(ui::line_hi()))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .text_size(px(13.))
                    .text_color(c(ui::muted()))
                    .child(Icon::new(IconName::Plus).size(px(14.)))
                    .child("Add a server")
                    .on_click(
                        cx.listener(|this, _, _, cx| this.go(Place::Settings(Pane::Server), cx)),
                    ),
            ),
        };
        list = list.child(
            div()
                .mt(px(18.))
                .px(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::faint()))
                        .child("WHERE JOBS RUN"),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(18.))
                        .text_color(c(ui::muted()))
                        .child(match &server {
                            Some((label, _)) => format!(
                                "Jobs run on {} unless you send one to {label}. Long jobs there carry on with this computer closed.",
                                here_name().to_lowercase()
                            ),
                            None => format!(
                                "Every job runs on {}. Add a server for jobs that carry on with the lid closed.",
                                here_name().to_lowercase()
                            ),
                        }),
                ),
        );

        let detail: AnyElement = if pick == 1 {
            self.server_page(cx)
        } else {
            self.this_machine(cx)
        };
        div()
            .id("machines")
            .size_full()
            .flex()
            .child(list)
            .child(div().flex_1().min_w_0().h_full().child(detail))
            .into_any_element()
    }

    /// This computer: the jobs and apps running here, and its projects.
    fn this_machine(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let jobs: Vec<(u64, super::data::Member, String, String)> = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.status == Status::Running && j.remote_on.is_empty())
            .map(|j| {
                let step = match j.progress() {
                    Some((d, n)) => format!("step {} of {n}", (d + 1).min(n)),
                    None => "working".into(),
                };
                (j.id, j.bot, super::channel::card_title(j), step)
            })
            .collect();
        let runs: Vec<(u64, String, Option<String>)> = self
            .runs
            .iter()
            .filter(|r| r.live() && !r.spec.setup && r.spec.remote.is_none())
            .map(|r| (r.id, r.spec.cmd.clone(), r.url().map(String::from)))
            .collect();
        let projects: Vec<String> = self
            .crew
            .store
            .folders
            .iter()
            .map(|f| f.name.clone())
            .collect();

        let mut running = div().flex().flex_col();
        if jobs.is_empty() && runs.is_empty() {
            running = running.child(
                div()
                    .py(px(4.))
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child("Nothing running here right now."),
            );
        }
        for (id, bot, title, step) in jobs {
            let face = self.avatar(bot, 20.);
            running = running.child(
                div()
                    .id(("machine-job", id as usize))
                    .h(px(34.))
                    .px(px(8.))
                    .mx(px(-8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .text_size(px(13.))
                    .child(face)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(c(ui::ink()))
                            .child(title),
                    )
                    .child(super::home::pill(&step, ui::run_tone()))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
            );
        }
        for (id, cmd, url) in runs {
            running = running.child(
                div()
                    .id(("machine-run", id as usize))
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(13.))
                    .child(
                        div()
                            .size(px(8.))
                            .mx(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(c(ui::t_green())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(12.5))
                            .text_color(c(ui::ink()))
                            .child(cmd),
                    )
                    .when_some(url, |d, u| {
                        let open = u.clone();
                        d.child(
                            div()
                                .font_family(ui::MONO)
                                .text_size(px(11.5))
                                .text_color(c(ui::faint()))
                                .child(u.trim_start_matches("http://").to_string()),
                        )
                        .child(
                            ui::ghost(("machine-open", id as usize), "Open")
                                .h(px(26.))
                                .text_size(px(12.))
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&open))),
                        )
                    })
                    .child(
                        ui::ghost(("machine-stop", id as usize), "Stop")
                            .h(px(26.))
                            .text_size(px(12.))
                            .on_click(cx.listener(move |this, _, _, cx| this.stop_run(id, cx))),
                    ),
            );
        }
        let card = |title: &'static str, body: Div| {
            ui::card()
                .p(px(18.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(ui::heading(title, None, None))
                .child(body)
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(card("Running here", running))
            .child(card(
                "Projects here",
                div()
                    .text_size(px(13.))
                    .text_color(c(ui::body()))
                    .child(if projects.is_empty() {
                        "No projects yet.".to_string()
                    } else {
                        projects.join(" \u{b7} ")
                    }),
            ))
            .child(
                div().flex().child(
                    ui::ghost_i(
                        "machine-term",
                        IconName::SquareTerminal,
                        "Open a terminal here",
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_project_term(window, cx)),
                    ),
                ),
            );
        Self::page(
            "machine-here-page",
            here_name(),
            "Jobs run here in their own copy of the project, and stop when this computer sleeps.",
            body,
        )
    }
}

/// This computer's system, in a word.
fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    }
}
