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
const LIST_W: f32 = 300.;

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

        let item =
            |id: &'static str, on: bool, icon: IconName, dot: u32, name: String, line: String| {
                div()
                    .id(id)
                    .relative()
                    .px(px(12.))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(6.))
                    .border_1()
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.bg(c(ui::raised()))
                                .border_color(c(ui::line()))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(4.))
                                        .bg(c(ui::accent_bar())),
                                )
                        } else {
                            d.border_color(transparent_black())
                                .hover(|s| s.bg(c(ui::hover())))
                        }
                    })
                    .child(icon_box(icon, 32., 15.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(px(12.))
                                    .text_color(c(ui::body()))
                                    .child(div().size(px(6.)).flex_none().bg(c(dot)))
                                    .child(div().truncate().child(line)),
                            ),
                    )
            };
        let mut list = div()
            .id("machines-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pt(px(14.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .px(px(8.))
                    .pt(px(6.))
                    .pb(px(10.))
                    .text_size(px(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child("Machines"),
            )
            .child(
                item(
                    "machine-here",
                    pick == 0,
                    IconName::Monitor,
                    ui::t_green(),
                    crate::hardware::name().unwrap_or_else(|| here_name().to_string()),
                    format!("{} \u{b7} online", here_name()),
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
                    IconName::Server,
                    if up { ui::t_green() } else { ui::t_amber() },
                    label,
                    format!(
                        "Server \u{b7} {} \u{b7} {}",
                        if up { "online" } else { "not reachable" },
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
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .child(icon_box(IconName::Plus, 32., 15.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::accent_text()))
                            .child("Add a server"),
                    )
                    .on_click(
                        cx.listener(|this, _, _, cx| this.go(Place::Settings(Pane::Server), cx)),
                    ),
            ),
        };
        let note = div()
            .flex_none()
            .m(px(12.))
            .mb(px(12. + super::DOCK_ROOM))
            .px(px(14.))
            .py(px(12.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .rounded(px(6.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .font_family(ui::MONO)
                    .text_size(px(10.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(ui::faint()))
                    .child("WHERE JOBS RUN"),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .line_height(px(18.))
                    .text_color(c(ui::body()))
                    .child(match &server {
                        Some(_) => "Each project picks a machine in Settings. Jobs on a server keep going when your laptop sleeps.".to_string(),
                        None => format!(
                            "Every job runs on {}. Add a server for jobs that keep going when your laptop sleeps.",
                            here_name().to_lowercase()
                        ),
                    }),
            )
            .child(
                div()
                    .id("machine-settings")
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::accent_text()))
                    .child("Settings, Servers \u{2197}")
                    .on_click(
                        cx.listener(|this, _, _, cx| this.go(Place::Settings(Pane::Server), cx)),
                    ),
            );
        let side = div()
            .w(px(LIST_W))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(c(ui::line()))
            .child(list)
            .child(note);

        let detail: AnyElement = if pick == 1 {
            self.server_page(cx)
        } else {
            self.this_machine(cx)
        };
        div()
            .id("machines")
            .size_full()
            .flex()
            .child(side)
            .child(div().flex_1().min_w_0().h_full().child(detail))
            .into_any_element()
    }

    /// This computer: its numbers, the jobs and apps running here, and its
    /// projects.
    fn this_machine(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let folders: Vec<(String, String)> = self
            .crew
            .store
            .folders
            .iter()
            .map(|f| (f.path.clone(), f.name.clone()))
            .collect();
        let folder_name = |path: &str| {
            folders
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| {
                    std::path::Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
        };
        let jobs: Vec<_> = self
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
                (
                    j.id,
                    j.bot,
                    super::channel::card_title(j),
                    step,
                    format!(
                        "#{} \u{b7} {} \u{b7} {}",
                        j.id,
                        j.folder.name,
                        mins(j.seconds())
                    ),
                )
            })
            .collect();
        let runs: Vec<(u64, String, Option<String>, String)> = self
            .runs
            .iter()
            .filter(|r| r.live() && !r.spec.setup && r.spec.remote.is_none())
            .map(|r| {
                let name = folder_name(&r.spec.folder);
                let title = if name.is_empty() {
                    r.spec.cmd.clone()
                } else {
                    format!("{name} \u{b7} {}", r.spec.cmd)
                };
                (
                    r.id,
                    title,
                    r.url().map(String::from),
                    mins(r.age().as_secs()),
                )
            })
            .collect();
        let n_jobs = jobs.len();
        let n_apps = runs.len();

        let row = |first: bool| {
            div()
                .py(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .when(!first, |d| {
                    d.border_t_1().border_color(c(ui::line())).border_dashed()
                })
        };
        let two_lines = |title: String, sub: String| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(title),
                )
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(11.))
                        .text_color(c(ui::faint()))
                        .child(sub),
                )
        };
        let mut running = div().flex().flex_col();
        if jobs.is_empty() && runs.is_empty() {
            running = running.child(
                div()
                    .py(px(6.))
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child("Nothing running here right now."),
            );
        }
        for (i, (id, bot, title, step, sub)) in jobs.into_iter().enumerate() {
            let face = self.avatar(bot, 20.);
            running = running.child(
                row(i == 0)
                    .child(face)
                    .child(two_lines(title, sub))
                    .child(ui::mono_status(ui::t_blue(), &step))
                    .child(
                        ui::dbtn(("machine-open-job", id as usize), "Open", ui::Btn::Plain)
                            .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
                    )
                    .child(
                        ui::dbtn(("machine-stop-job", id as usize), "Stop", ui::Btn::Ghost)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.crew.stop(id);
                                cx.notify();
                            })),
                    ),
            );
        }
        for (i, (id, title, url, age)) in runs.into_iter().enumerate() {
            let sub = match &url {
                Some(u) => format!(
                    "{} \u{b7} {age}",
                    u.trim_start_matches("http://")
                        .trim_start_matches("https://")
                ),
                None => age,
            };
            running = running.child(
                row(i == 0 && n_jobs == 0)
                    .child(
                        div()
                            .w(px(20.))
                            .flex()
                            .justify_center()
                            .child(div().size(px(8.)).rounded_full().bg(c(ui::t_green()))),
                    )
                    .child(two_lines(title, sub))
                    .child(ui::mono_status(ui::t_green(), "running"))
                    .when_some(url, |d, u| {
                        d.child(
                            ui::dbtn(("machine-open", id as usize), "Open", ui::Btn::Plain)
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&u))),
                        )
                    })
                    .child(
                        ui::dbtn(("machine-stop", id as usize), "Stop", ui::Btn::Ghost)
                            .on_click(cx.listener(move |this, _, _, cx| this.stop_run(id, cx))),
                    ),
            );
        }
        let mut projects = div().flex().flex_col();
        if folders.is_empty() {
            projects = projects.child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child("No projects yet."),
            );
        }
        for (i, (path, name)) in folders.iter().enumerate() {
            projects = projects.child(
                div()
                    .py(px(5.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .when(i > 0, |d| {
                        d.border_t_1().border_color(c(ui::line())).border_dashed()
                    })
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(c(ui::ink()))
                            .child(name.clone()),
                    )
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .text_color(c(ui::faint()))
                            .child(super::sidebar::short_path(path)),
                    ),
            );
        }
        let card = |title: &'static str, body: Div| {
            div()
                .px(px(15.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .rounded(px(6.))
                .bg(c(ui::raised()))
                .border_1()
                .border_color(c(ui::line()))
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::faint()))
                        .child(title.to_uppercase()),
                )
                .child(body)
        };

        // The numbers: jobs and apps running, how busy, room on disk.
        let hw = crate::hardware::here();
        let disk = folders
            .first()
            .map(|(p, _)| std::path::PathBuf::from(p))
            .or_else(dirs_home)
            .and_then(|p| crate::hardware::disk_free(&p));
        let stat = |n: String, k: &'static str, first: bool| {
            div()
                .flex_1()
                .px(px(12.))
                .py(px(9.))
                .when(!first, |d| d.border_l_1().border_color(c(ui::line())))
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(16.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::ink()))
                        .child(n),
                )
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::faint()))
                        .child(k.to_uppercase()),
                )
        };
        let strip = div()
            .flex()
            .rounded(px(6.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .child(stat(n_jobs.to_string(), "Jobs running", true))
            .child(stat(
                n_apps.to_string(),
                if n_apps == 1 {
                    "App running"
                } else {
                    "Apps running"
                },
                false,
            ))
            .child(stat(
                crate::hardware::busy_pct().map_or("\u{2014}".into(), |p| format!("{p:.0}%")),
                "CPU",
                false,
            ))
            .child(stat(
                disk.map_or("\u{2014}".into(), |b| format!("{} GB", b >> 30)),
                "Disk free",
                false,
            ));

        let mut spec = vec![here_name().to_string(), crate::hardware::os_version()];
        if hw.cores > 0 {
            spec.push(format!("{} cores", hw.cores));
        }
        if hw.ram_gb > 0 {
            spec.push(format!("{} GB", hw.ram_gb));
        }
        let head = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(icon_box(IconName::Monitor, 46., 20.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(
                                crate::hardware::name().unwrap_or_else(|| here_name().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child(spec.join(" \u{b7} ")),
                    ),
            )
            .child(
                ui::dbtn_i(
                    "machine-term",
                    IconName::SquareTerminal,
                    "Open a terminal here",
                    ui::Btn::Plain,
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_project_term(window, cx))),
            );
        div()
            .id("machine-here-page")
            .size_full()
            .overflow_y_scroll()
            .px(px(28.))
            .pt(px(24.))
            .pb(px(24. + super::DOCK_ROOM))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(head)
            .child(strip)
            .child(card("Running here", running))
            .child(card("Projects here", projects))
            .into_any_element()
    }
}

/// A square with a hairline round an icon, as the machines list leads with.
fn icon_box(icon: IconName, size: f32, glyph: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(if size > 40. { 8. } else { 6. }))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line_hi()))
        .child(Icon::new(icon).size(px(glyph)).text_color(c(ui::ink())))
}

/// The home folder, for when there's no project to measure the disk by.
fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// How long something has run, in short: "4m", "2h 5m", "12s".
fn mins(secs: u64) -> String {
    match secs {
        s if s >= 3600 => format!("{}h {}m", s / 3600, s % 3600 / 60),
        s if s >= 60 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}
