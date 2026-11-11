//! A job, two ways. The peek pane opens beside the thread when you click a
//! job's card: what it is, where it stands, its changes and the buttons to
//! decide, without leaving the conversation. Expand opens the job workspace:
//! the bot's conversation always on the left, and the job's Changes,
//! Checks, Browser, Terminal and Files as tabs on the right.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::{Framing, Mood};

use gpui_kit::component::Icon;

use super::data::Member;
use super::store::{Job, Status};
use super::ui::{self, c};
use super::{home, Place, Shell};

/// The peek pane's width.
pub(super) const PEEK_W: f32 = 352.;
/// The workspace's conversation column.
pub(super) const TALK_W: f32 = 430.;

/// The workspace's tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Tab {
    #[default]
    Changes,
    Checks,
    Browser,
    Terminal,
    Files,
}

impl Tab {
    pub(super) const ALL: [Tab; 5] = [
        Tab::Changes,
        Tab::Checks,
        Tab::Browser,
        Tab::Terminal,
        Tab::Files,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Tab::Changes => "Changes",
            Tab::Checks => "Checks",
            Tab::Browser => "Browser",
            Tab::Terminal => "Terminal",
            Tab::Files => "Files",
        }
    }
}

/// The design's list label (`.lbl`): mono capitals, faint.
fn label(text: &str) -> Div {
    div()
        .mb(px(6.))
        .font_family(ui::MONO)
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(ui::faint()))
        .child(text.to_uppercase())
}

/// One row of a peek list (`.kv`): a name on the left, a value on the
/// right, a dashed line under (drawn solid here).
fn kv(left: Div, right: Div) -> Div {
    div()
        .h(px(23.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .border_b_1()
        .border_color(c(ui::line()))
        .child(left.min_w_0().truncate())
        .child(right.flex_none())
}

fn mono_ink(text: String) -> Div {
    div()
        .font_family(ui::MONO)
        .text_size(px(12.))
        .text_color(c(ui::ink()))
        .child(text)
}

fn mono_tone(text: String, tone: u32) -> Div {
    div()
        .font_family(ui::MONO)
        .text_size(px(12.))
        .text_color(c(tone))
        .child(text)
}

/// "44s", "4m20s".
fn secs(s: u64) -> String {
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m{:02}s", s / 60, s % 60)
    }
}

/// A quiet sentence in the middle of an empty tab, with an optional button.
fn empty(line: &'static str, button: Option<Stateful<Div>>) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .text_size(px(13.))
        .text_color(c(ui::muted()))
        .child(line)
        .children(button)
}

impl Shell {
    /// Opens a job's peek beside its thread. A race with more than one try
    /// still opens its comparison page, since that's where you pick one.
    pub(super) fn peek_job(&mut self, id: u64, cx: &mut Context<Self>) {
        let race = self.crew.store.job(id).map(|j| j.race).unwrap_or(0);
        if race != 0 && self.tries(race).len() > 1 {
            self.go(Place::Race(race), cx);
            return;
        }
        if self.place != Place::Today {
            self.go(Place::Today, cx);
        }
        self.reply_open = Some(id);
        cx.notify();
    }

    /// The peek pane, as the design draws it: the job's crumb with Expand and
    /// close; the bot and the title; where it stands; what holds it (the
    /// seatbelt, a clash); its checks, changes and log; time, turns and cost;
    /// and the buttons to decide at the foot.
    pub(super) fn peek_pane(&mut self, job: &Job, thread: &str, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let (word, tone) = home::status_of(job);
        let col = if job.status == Status::Running {
            ui::t_blue()
        } else {
            tone.0
        };
        let face = self.bot(job.bot, Mood::Idle, 40., Framing::Bust);
        let by = if job.engine.is_empty() {
            job.bot.name().to_string()
        } else {
            format!("{} \u{b7} {}", job.bot.name(), job.engine)
        };
        let head = div()
            .h(px(50.))
            .flex_none()
            .pl(px(18.))
            .pr(px(14.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .flex()
                    .gap(px(6.))
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child(
                        div()
                            .text_color(c(ui::ink()))
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("JOB #{id}")),
                    )
                    .child(format!("/ #{}", thread.to_uppercase())),
            )
            .child(
                ui::dbtn("peek-expand", "Expand", ui::Btn::Plain)
                    .child(ui::kbd("\u{21b5}"))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "Open the job's workspace: its conversation, changes, browser and terminal",
                        )
                        .build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reply_open = None;
                        this.open_job(id, cx);
                    })),
            )
            .child(
                ui::icon_btn("peek-close", IconName::X).on_click(cx.listener(|this, _, _, cx| {
                    this.reply_open = None;
                    cx.notify();
                })),
            );

        let title = div().flex().items_start().gap(px(12.)).child(face).child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(18.))
                        .line_height(px(22.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(super::channel::card_title(job)),
                )
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(11.5))
                        .text_color(c(ui::body()))
                        .child(by),
                ),
        );

        let state_word = if job.status == Status::Running {
            match job.progress() {
                Some((d, n)) => format!("Working \u{b7} {}/{n}", (d + 1).min(n)),
                None => "Working".to_string(),
            }
        } else {
            word
        };
        let place = if job.remote_on.is_empty() {
            super::machines::here_name().to_string()
        } else {
            job.remote_on.clone()
        };
        let state = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .font_family(ui::MONO)
            .text_size(px(12.))
            .text_color(c(ui::body()))
            .child(ui::mono_status(col, &state_word).flex_none())
            .when(!job.branch.is_empty(), |d| {
                d.child(div().min_w_0().truncate().child(if job.base.is_empty() {
                    job.branch.clone()
                } else {
                    format!("{} \u{2192} {}", job.branch, job.base)
                }))
            })
            .child(div().flex_none().child(place));

        let held = job.status == Status::Review && !job.held.is_empty() && !job.belt_ok;
        let belt = held.then(|| self.belt_box(job, cx));
        let clash = self.clash_box(job, cx);
        let in_the_way = self.in_the_way_card(job, cx);
        let shipped = matches!(job.status, Status::Shipping | Status::Shipped)
            .then(|| self.review_body(job, cx))
            .flatten();

        let stopped = matches!(
            job.status,
            Status::Failed | Status::Interrupted | Status::Stopped
        );
        let why = stopped.then(|| {
            let reason = job
                .feed
                .iter()
                .rev()
                .find_map(|it| match it {
                    super::store::Item::Error(e) => Some(e.clone()),
                    _ => None,
                })
                .or_else(|| {
                    job.last_words()
                        .map(|w| w.lines().next().unwrap_or("").to_string())
                })
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| "It ended before it finished.".into());
            let red = ui::t_red();
            div()
                .px(px(14.))
                .py(px(11.))
                .flex()
                .flex_col()
                .gap(px(3.))
                .rounded(px(5.))
                .bg(c(super::theme::mix(ui::raised(), red, 0.06)))
                .border_1()
                .border_color(c(super::theme::mix(ui::raised(), red, 0.3)))
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(red))
                        .child(if job.status == Status::Stopped {
                            "You stopped it"
                        } else {
                            "Stopped"
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(18.))
                        .text_color(c(ui::body()))
                        .child(reason),
                )
        });
        let checks = peek_checks(job);
        let changes = self.peek_changes(job, cx);

        // Log: the job's last few steps, on a line.
        let steps: Vec<(String, u8)> = job
            .feed
            .iter()
            .filter_map(|it| match it {
                super::store::Item::Tool { label, state, .. } => Some((label.clone(), *state)),
                _ => None,
            })
            .collect();
        let log = (!steps.is_empty()).then(|| {
            let from = steps.len().saturating_sub(4);
            let last = steps.len() - 1;
            let mut line = div()
                .flex()
                .flex_col()
                .border_l_1()
                .border_color(c(ui::line_hi()))
                .pl(px(12.));
            for (n, (what, st)) in steps.iter().enumerate().skip(from) {
                let end = n == last;
                let dot = match st {
                    0 => ui::t_blue(),
                    2 => ui::t_red(),
                    _ if end && !stopped => tone.0,
                    _ => ui::faint(),
                };
                line = line.child(
                    div()
                        .relative()
                        .h(px(23.))
                        .flex()
                        .items_center()
                        .text_size(px(12.5))
                        .text_color(c(if end { ui::ink() } else { ui::body() }))
                        .child(
                            div()
                                .absolute()
                                .left(px(-15.5))
                                .top(px(8.5))
                                .size(px(6.))
                                .bg(c(dot)),
                        )
                        .child(div().min_w_0().truncate().child(what.clone())),
                );
            }
            if stopped {
                line = line.child(
                    div()
                        .relative()
                        .h(px(23.))
                        .flex()
                        .items_center()
                        .text_size(px(12.5))
                        .text_color(c(ui::ink()))
                        .child(
                            div()
                                .absolute()
                                .left(px(-15.5))
                                .top(px(8.5))
                                .size(px(6.))
                                .bg(c(ui::t_red())),
                        )
                        .child(if job.status == Status::Stopped {
                            "You stopped it"
                        } else {
                            "Stopped and asked you"
                        }),
                );
            }
            div().flex().flex_col().child(label("Log")).child(line)
        });

        // Time, turns and cost, from "4m20s · 9 turns · $0.22".
        let stats = (!job.summary.is_empty()).then(|| {
            let parts: Vec<&str> = job.summary.split(" \u{b7} ").collect();
            let time = parts.first().copied().unwrap_or("");
            let turns = parts
                .iter()
                .find(|p| p.contains("turn"))
                .map(|p| p.split_whitespace().next().unwrap_or("").to_string())
                .unwrap_or_else(|| "1".into());
            let cost = parts
                .iter()
                .find(|p| p.starts_with('$'))
                .map(|p| p.to_string())
                .unwrap_or_else(|| "\u{2013}".into());
            let cell = |v: String, k: &'static str, first: bool| {
                div()
                    .flex_1()
                    .px(px(12.))
                    .py(px(9.))
                    .when(!first, |d| d.border_l_1().border_color(c(ui::line())))
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(15.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(ui::ink()))
                            .child(v),
                    )
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(10.5))
                            .text_color(c(ui::faint()))
                            .child(k.to_uppercase()),
                    )
            };
            div()
                .flex()
                .rounded(px(5.))
                .border_1()
                .border_color(c(ui::line()))
                .child(cell(time.to_string(), "time", true))
                .child(cell(turns, "turns", false))
                .child(cell(cost, "cost", false))
        });

        let foot = self.peek_foot(job, held, cx);

        div()
            .w(px(PEEK_W))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(c(ui::line()))
            .bg(c(ui::panel()))
            .child(head)
            .child(
                div()
                    .id("peek-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(18.))
                    .py(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(title)
                    .child(state)
                    .children(why)
                    .children(belt)
                    .children(clash)
                    .children(in_the_way)
                    .children(shipped)
                    .children(checks)
                    .children(changes)
                    .children(log)
                    .children(stats),
            )
            .children(foot)
    }

    /// A peek list of the files the job changed; a click opens its diff.
    pub(super) fn peek_changes(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        let id = job.id;
        // Changes: each file with its lines.
        (!job.changes.is_empty()).then(|| {
            let n = job.changes.len();
            let mut list = div().flex().flex_col().child(label(&format!(
                "Changes \u{b7} {n} {}",
                if n == 1 { "file" } else { "files" }
            )));
            for (i, f) in job.changes.iter().enumerate() {
                let path = f.path.clone();
                list = list.child(
                    kv(
                        mono_ink(f.path.clone()),
                        div()
                            .flex()
                            .flex_none()
                            .gap(px(6.))
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .when(f.added > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(c(ui::t_green()))
                                        .child(format!("+{}", f.added)),
                                )
                            })
                            .when(f.removed > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(c(ui::t_red()))
                                        .child(format!("\u{2212}{}", f.removed)),
                                )
                            }),
                    )
                    .id(("peek-file", i))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.diff_text = this
                            .crew
                            .store
                            .job(id)
                            .map(|j| super::work::diff(j, &path))
                            .unwrap_or_default();
                        this.diff_open = Some((id, path.clone()));
                        this.ws_tab = Tab::Changes;
                        this.reply_open = None;
                        this.open_job(id, cx);
                    })),
                );
            }
            list
        })
    }

    /// The seatbelt's catch, red: what it found and where, then the two ways on.
    pub(super) fn belt_box(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let red = ui::t_red();
        let mut found = div().flex().flex_col().gap(px(2.)).mt(px(4.)).mb(px(10.));
        for line in &job.held {
            let (place, what) = line
                .split_once(" \u{b7} ")
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .unwrap_or_else(|| (String::new(), line.clone()));
            found = found.child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(4.))
                    .text_size(px(12.5))
                    .text_color(c(ui::body()))
                    .child(div().flex_none().child(what))
                    .when(!place.is_empty(), |d| {
                        let short = place.rsplit('/').next().unwrap_or(&place).to_string();
                        d.child(div().flex_none().child("in")).child(
                            div()
                                .id(SharedString::from(format!("belt-at-{place}")))
                                .min_w_0()
                                .truncate()
                                .font_family(ui::MONO)
                                .text_size(px(12.))
                                .text_color(c(ui::ink()))
                                .child(short)
                                .tooltip(move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(place.clone())
                                        .build(window, cx)
                                }),
                        )
                    }),
            );
        }
        let anyway = match self.crew.way(job) {
            super::ship::Way::PullRequest => "Push anyway",
            super::ship::Way::Merge => "Merge anyway",
        };
        div()
            .px(px(14.))
            .py(px(12.))
            .rounded(px(5.))
            .bg(ui::ca(red, 0.05))
            .border_1()
            .border_color(ui::ca(red, 0.30))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(red))
                    .child(Icon::new(IconName::ShieldCheck).size(px(14.)))
                    .child("Seatbelt is holding this"),
            )
            .child(found)
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        ui::dbtn(
                            "peek-take-out",
                            format!("Ask {} to remove it", job.bot.name()),
                            ui::Btn::Primary,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.take_out(id, cx))),
                    )
                    .child(
                        ui::dbtn("peek-anyway", anyway, ui::Btn::Ghost).on_click(
                            cx.listener(move |this, _, _, cx| this.approve_anyway(id, cx)),
                        ),
                    ),
            )
    }

    /// Another job changes the same lines: which one, and where.
    pub(super) fn clash_box(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if job.clash_with.is_empty()
            || !matches!(job.status, Status::Running | Status::Done | Status::Review)
        {
            return None;
        }
        let mut b = div()
            .px(px(14.))
            .py(px(12.))
            .rounded(px(5.))
            .border_1()
            .border_color(c(ui::line()))
            .flex()
            .flex_col()
            .gap(px(2.));
        for (other, words) in &job.clash_with {
            let other = *other;
            b = b
                .child(
                    div()
                        .id(("peek-clash", other as usize))
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .cursor_pointer()
                        .hover(|s| s.text_color(c(ui::accent_text())))
                        .child(format!("Clash ahead with #{other}"))
                        .on_click(cx.listener(move |this, _, _, cx| this.peek_job(other, cx))),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(c(ui::body()))
                        .child(format!(
                            "Both change {words}. Whichever is approved second needs a rebase."
                        )),
                );
        }
        Some(b)
    }

    /// The peek's foot: Discard and Send back, then Approve on the right,
    /// dimmed with HELD beside it while the seatbelt holds it. A running job
    /// gets Stop; one that ended badly, Try again.
    fn peek_foot(&mut self, job: &Job, held: bool, cx: &mut Context<Self>) -> Option<Div> {
        let id = job.id;
        let foot = div()
            .flex_none()
            .px(px(14.))
            .py(px(12.))
            .flex()
            .items_center()
            .gap(px(4.))
            .border_t_1()
            .border_color(c(ui::line()));
        match job.status {
            Status::Review if !job.changes.is_empty() => {
                let full = self.approve_label(job);
                let blocked = held || !job.conflict.is_empty();
                let approve = ui::dbtn("peek-approve", "Approve", ui::Btn::Primary)
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                    })
                    .when(blocked, |d| d.opacity(0.35).cursor_default())
                    .when(!blocked, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| this.approve(id, cx)))
                    });
                Some(
                    foot.child(
                        ui::dbtn("peek-discard", "Discard", ui::Btn::Ghost).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.reply_open = None;
                                this.discard_job(id, cx)
                            },
                        )),
                    )
                    .child(
                        ui::dbtn("peek-send-back", "Send back", ui::Btn::Plain).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.reply_open = None;
                                this.open_job(id, cx)
                            }),
                        ),
                    )
                    .child(div().flex_1())
                    .when(blocked, |d| {
                        d.child(
                            div()
                                .mr(px(4.))
                                .font_family(ui::MONO)
                                .text_size(px(10.5))
                                .text_color(c(ui::faint()))
                                .child(if held { "HELD" } else { "CLASH" }),
                        )
                    })
                    .child(approve),
                )
            }
            Status::Running => Some(
                foot.child(div().flex_1()).child(
                    ui::dbtn("peek-stop", "Stop", ui::Btn::Plain)
                        .on_click(cx.listener(move |this, _, _, cx| this.stop_job(id, cx))),
                ),
            ),
            Status::Failed | Status::Interrupted | Status::Stopped => {
                let fixer = job.bot != Member::Fixer;
                let ask = format!(
                    "@Fixer finish job #{id}, {}: it stopped before it was done",
                    job.label()
                );
                Some(
                    foot.child(
                        ui::dbtn("peek-discard", "Discard", ui::Btn::Ghost).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.reply_open = None;
                                this.discard_job(id, cx)
                            },
                        )),
                    )
                    .child(
                        ui::dbtn(
                            "peek-retry",
                            "Try again",
                            if fixer {
                                ui::Btn::Plain
                            } else {
                                ui::Btn::Primary
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.retry_job(id, cx))),
                    )
                    .child(div().flex_1())
                    .when(fixer, |d| {
                        d.child(
                            ui::dbtn("peek-fixer", "Ask Fixer", ui::Btn::Primary).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.hand_over(ask.clone(), cx);
                                }),
                            ),
                        )
                    }),
                )
            }
            _ => None,
        }
    }

    /// The job workspace: the conversation on the left, always, so you can
    /// watch and talk at once, and the job's tabs on the right.
    pub(super) fn workspace(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(job) = self.crew.store.job(id).cloned() else {
            return self.live(id, cx);
        };
        let talk = self.live_talk(&job, cx);
        let tab = self.ws_tab;
        let mut tabs = div()
            .h(px(50.))
            .flex_none()
            .px(px(14.))
            .flex()
            .gap(px(2.))
            .border_b_1()
            .border_color(c(ui::line()));
        for t in Tab::ALL {
            let on = t == tab;
            let count = match t {
                Tab::Changes if !job.changes.is_empty() => Some(job.changes.len()),
                Tab::Checks if !job.checks.is_empty() => Some(job.checks.len()),
                _ => None,
            };
            tabs = tabs.child(
                div()
                    .id(("ws-tab", t as usize))
                    .relative()
                    .h_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.text_color(c(ui::ink())).child(
                                div()
                                    .absolute()
                                    .left(px(8.))
                                    .right(px(8.))
                                    .bottom(px(-1.))
                                    .h(px(2.))
                                    .bg(c(ui::accent_bar())),
                            )
                        } else {
                            d.text_color(c(ui::faint()))
                                .hover(|s| s.text_color(c(ui::ink())))
                        }
                    })
                    .child(t.name())
                    .when_some(count, |d, n| {
                        d.child(
                            div()
                                .px(px(5.))
                                .line_height(px(16.))
                                .rounded(px(3.))
                                .bg(c(ui::hover()))
                                .text_size(px(10.))
                                .text_color(c(ui::body()))
                                .child(n.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.ws_tab = t;
                        // The Terminal tab opens straight onto a shell.
                        if t == Tab::Terminal && this.term_open != Some(id) {
                            this.toggle_term(id, window, cx);
                        }
                        cx.notify();
                    })),
            );
        }
        // Close goes back to the thread, with the job open in its peek.
        let tabs = tabs.child(div().flex_1()).child(
            div().h_full().flex().items_center().child(
                ui::icon_btn("ws-close", IconName::X)
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Back to the thread")
                            .build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.peek_job(id, cx))),
            ),
        );
        let body: AnyElement = match tab {
            Tab::Changes => match self.changes_pane(&job, cx) {
                Some(pane) => div()
                    .id("ws-changes")
                    .size_full()
                    .overflow_y_scroll()
                    .px(px(18.))
                    .py(px(16.))
                    .child(pane)
                    .into_any_element(),
                None => empty(
                    "No changes yet. They show here as the bot edits files.",
                    None,
                )
                .into_any_element(),
            },
            Tab::Checks => div()
                .id("ws-checks")
                .size_full()
                .overflow_y_scroll()
                .px(px(18.))
                .py(px(16.))
                .child(self.checks_tab(&job, cx))
                .into_any_element(),
            Tab::Browser => {
                if self.web.job == id && self.web.shown() {
                    self.web_page(window, cx)
                } else {
                    empty(
                        "The bot's own browser, to watch it work or try the app yourself.",
                        Some(
                            ui::solid_i("ws-browser", IconName::Globe, "Open its browser")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_job_browser(id, None, cx);
                                    // Stay in the workspace: the browser opens in its tab.
                                    this.place = Place::Live(id);
                                    this.ws_tab = Tab::Browser;
                                    cx.notify();
                                })),
                        ),
                    )
                    .into_any_element()
                }
            }
            Tab::Terminal => match self.term_tab(&job, cx) {
                Some(term) => term.into_any_element(),
                None => empty(
                    "A shell in the job's own copy of the project.",
                    Some(
                        ui::solid_i("ws-term", IconName::SquareTerminal, "Open a terminal")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_term(id, window, cx)
                            })),
                    ),
                )
                .into_any_element(),
            },
            Tab::Files => self.job_files(&job, cx).into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(TALK_W))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(c(ui::line()))
                    .child(talk),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .bg(c(ui::panel()))
                    .child(tabs)
                    .child(div().flex_1().min_h_0().child(body)),
            )
            .into_any_element()
    }
}

/// A peek list of the job's checks: each command and how it went.
pub(super) fn peek_checks(job: &Job) -> Option<Div> {
    // Checks: each command and how it went.
    (!job.checks.is_empty() || job.checking).then(|| {
        let mut list = div().flex().flex_col().child(label("Checks"));
        for ch in &job.checks {
            let (mark, tone) = if ch.ok {
                ("\u{2713}", ui::t_green())
            } else if !ch.infra.is_empty() {
                ("\u{2013}", ui::faint())
            } else {
                ("\u{2715}", ui::t_red())
            };
            list = list.child(kv(
                mono_ink(ch.command.clone()),
                mono_tone(format!("{mark} {}", secs(ch.secs)), tone),
            ));
        }
        if job.checking {
            list = list.child(kv(
                mono_ink("Checker is running them".to_string()),
                mono_tone("\u{2026}".to_string(), ui::faint()),
            ));
        }
        list
    })
}

#[cfg(test)]
mod tests {
    use super::Tab;

    #[test]
    fn every_tab_has_a_name_and_changes_comes_first() {
        assert_eq!(Tab::default(), Tab::Changes);
        assert_eq!(Tab::ALL[0], Tab::Changes);
        for t in Tab::ALL {
            assert!(!t.name().is_empty());
        }
    }
}
