//! A job, two ways. The peek pane opens beside the thread when you click a
//! job's card: what it is, where it stands, its changes and the buttons to
//! decide, without leaving the conversation. Expand opens the job workspace:
//! the bot's conversation always on the left, and the job's Changes,
//! Checks, Browser, Terminal and Files as tabs on the right.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
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
        let ships = matches!(job.status, Status::Shipping | Status::Shipped);
        let shipped = ships.then(|| self.peek_ship(job, cx));

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
        let checks = peek_checks(job).filter(|_| !ships);
        // A job waiting for you shows its diff right here, to comment on.
        let reviewing = job.status == Status::Review && !job.changes.is_empty();
        let changes = if reviewing {
            self.peek_diff(job, cx)
        } else {
            self.peek_changes(job, cx)
        };
        let more = reviewing.then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(label("Anything else to say"))
                .child(
                    div()
                        .h(px(54.))
                        .px(px(4.))
                        .pt(px(4.))
                        .rounded(px(5.))
                        .bg(c(ui::raised()))
                        .border_1()
                        .border_color(c(ui::line_hi()))
                        .text_size(px(13.))
                        .child(Input::new(&self.peek_note).appearance(false).bordered(false)),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(c(ui::body()))
                        .child(format!(
                            "{} gets your comment with the line it's on, and keeps working on the same branch.",
                            job.bot.name()
                        )),
                )
        });

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
                    .children(changes.filter(|_| !ships))
                    .children(more)
                    .children(log.filter(|_| !reviewing && !ships))
                    .children(stats),
            )
            .children(foot)
    }

    /// A job on its way in, in the side panel: Shipper's steps while it
    /// pushes, the pull request's checks while it's open, and what happened
    /// once it's merged.
    fn peek_ship(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let mut out = div().flex().flex_col().gap(px(14.));
        let said: Vec<String> = job
            .feed
            .iter()
            .filter_map(|it| match it {
                super::store::Item::By(Member::Shipper, t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        if job.status == Status::Shipped {
            // What happened, from the asking to the merge.
            let mut events: Vec<String> = Vec::new();
            for it in &job.feed {
                match it {
                    super::store::Item::You(t) => {
                        let t: String = t.lines().next().unwrap_or("").chars().take(60).collect();
                        events.push(format!("You: {t}"));
                    }
                    super::store::Item::By(m, t) => {
                        let t: String = t.lines().next().unwrap_or("").chars().take(60).collect();
                        events.push(format!("{}: {t}", m.name()));
                    }
                    _ => {}
                }
            }
            events.push(if job.shipped.is_empty() {
                "Merged".to_string()
            } else {
                job.shipped.clone()
            });
            let from = events.len().saturating_sub(6);
            let last = events.len() - 1;
            let mut line = div()
                .flex()
                .flex_col()
                .border_l_1()
                .border_color(c(ui::line_hi()))
                .pl(px(12.));
            for (n, e) in events.into_iter().enumerate().skip(from) {
                let end = n == last;
                line = line.child(
                    div()
                        .relative()
                        .min_h(px(23.))
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
                                .bg(c(if end { ui::t_green() } else { ui::faint() })),
                        )
                        .child(div().min_w_0().truncate().child(e)),
                );
            }
            return out
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(label("What happened"))
                        .child(line),
                )
                .child(note_text(
                    "The job leaves the Inbox. You'll find it under Jobs, Done.".into(),
                ));
        }
        if job.pr.is_empty() {
            // Shipper pushing: what it did, what it's doing, what's next.
            let way = self.way(job);
            let pr_way = matches!(way, super::ship::Way::PullRequest);
            let face = self.avatar(Member::Shipper, 22.);
            out = out.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label("Shipper"))
                    .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .child(div().flex_none().child(face))
                        .child(note_text(if pr_way {
                            "The one bot that touches GitHub. It opens the pull request for you."
                                .into()
                        } else {
                            format!("It merges the change into {} here.", job.base)
                        })),
                ),
            );
            let mut steps = div().flex().flex_col();
            let mut rows: Vec<(u8, String)> = said.into_iter().map(|t| (0u8, t)).collect();
            if job.shipping {
                rows.push((
                    1,
                    if pr_way {
                        "Opening the pull request".into()
                    } else {
                        format!("Merging into {}", job.base)
                    },
                ));
            }
            if pr_way {
                rows.push((2, "Looking at its checks".into()));
            }
            for (i, (state, text)) in rows.into_iter().enumerate() {
                steps = steps.child(step_row(state, text, i > 0));
            }
            return out.child(steps).children(peek_checks(job));
        }
        // The pull request is open: its checks, merging on its own, and what
        // it says.
        let (done, total) = pr_counts(&job.pr_line);
        let mut checks = div().flex().flex_col().child(
            div()
                .flex()
                .justify_between()
                .child(label("Checks on GitHub"))
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::faint()))
                        .child(match (done, total) {
                            (Some(d), Some(t)) => format!("{d} of {t} done"),
                            _ => String::new(),
                        }),
                ),
        );
        let mut first = true;
        for f in &job.pr_failed {
            checks = checks.child(step_row(3, f.clone(), !first));
            first = false;
        }
        if !job.pr_line.is_empty() {
            let state = match (done, total) {
                (Some(d), Some(t)) if d >= t && job.pr_failed.is_empty() => 0,
                _ if !job.pr_failed.is_empty() => 3,
                _ => 1,
            };
            checks = checks.child(step_row(state, job.pr_line.clone(), !first));
        }
        let auto = job.auto_merge;
        let merge_card = div()
            .id("peek-auto-merge")
            .px(px(12.))
            .py(px(10.))
            .flex()
            .items_start()
            .gap(px(10.))
            .rounded(px(5.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .cursor_pointer()
            .child(div().mt(px(2.)).child(super::prefs::switch(auto)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("Merge when checks pass"),
                    )
                    .child(note_text(
                        "Then Shipper deletes the branch and its worktree.".into(),
                    )),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_auto_merge(id, !auto, cx)));
        let words: String = job
            .last_words()
            .unwrap_or_default()
            .chars()
            .take(320)
            .collect();
        let written = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(label("Written from the job"))
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(5.))
                    .bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(job.label().to_string()),
                    )
                    .when(!words.trim().is_empty(), |d| {
                        d.child(
                            div()
                                .mt(px(3.))
                                .text_size(px(12.5))
                                .line_height(px(19.))
                                .text_color(c(ui::body()))
                                .child(words),
                        )
                    }),
            );
        out.child(checks).child(merge_card).child(written)
    }

    /// The diff of one changed file, a window of it, with the file's place
    /// among the others; click a line to comment on it.
    fn peek_diff(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        let id = job.id;
        let n = job.changes.len();
        let at = self
            .diff_open
            .as_ref()
            .filter(|(j, _)| *j == id)
            .and_then(|(_, p)| job.changes.iter().position(|f| &f.path == p));
        let at = match at {
            Some(i) => i,
            None => {
                let first = job.changes.first()?.path.clone();
                self.diff_text = super::work::diff(job, &first);
                self.diff_open = Some((id, first));
                0
            }
        };
        let f = job.changes[at].clone();
        let (added, removed) = job.lines();
        let text = self.diff_text.clone();
        let total = super::review::numbered(&text).len();
        const SHOWN: usize = 14;
        let lines = self.diff_lines(id, &f.path, &text, SHOWN, cx);
        let next = job.changes[(at + 1) % n].path.clone();
        let head = div()
            .h(px(30.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(c(ui::line()))
            .font_family(ui::MONO)
            .text_size(px(12.))
            .child(
                Icon::new(IconName::File)
                    .size(px(13.))
                    .text_color(c(ui::faint())),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(ui::ink()))
                    .child(f.path.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(c(ui::t_green()))
                    .child(format!("+{}", f.added)),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(c(ui::t_red()))
                    .child(format!("\u{2212}{}", f.removed)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("peek-next-file")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_color(c(ui::body()))
                    .when(n > 1, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.text_color(c(ui::ink())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.diff_text = this
                                    .crew
                                    .store
                                    .job(id)
                                    .map(|j| super::work::diff(j, &next))
                                    .unwrap_or_default();
                                this.diff_open = Some((id, next.clone()));
                                this.line_at = None;
                                cx.notify();
                            }))
                    })
                    .child(format!("{} of {n}", at + 1))
                    .when(n > 1, |d| {
                        d.child(Icon::new(IconName::ChevronRight).size(px(13.)))
                    }),
            );
        let path = f.path.clone();
        let foot = div()
            .h(px(28.))
            .px(px(10.))
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(c(ui::line()))
            .font_family(ui::MONO)
            .text_size(px(11.))
            .text_color(c(ui::faint()))
            .child(if total > SHOWN {
                format!("\u{2026} {} more lines", total - SHOWN)
            } else {
                String::new()
            })
            .child(
                div()
                    .id("peek-full-diff")
                    .cursor_pointer()
                    .hover(|s| s.text_color(c(ui::ink())))
                    .child("Open full diff \u{2197}")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.diff_open = Some((id, path.clone()));
                        this.ws_tab = Tab::Changes;
                        this.reply_open = None;
                        this.go(Place::Live(id), cx);
                    })),
            );
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(label("Changes"))
                        .child(
                            div()
                                .font_family(ui::MONO)
                                .text_size(px(10.5))
                                .text_color(c(ui::faint()))
                                .child(format!(
                                    "{n} {} \u{b7} +{added} \u{2212}{removed}",
                                    if n == 1 { "file" } else { "files" }
                                )),
                        ),
                )
                .child(
                    div()
                        .rounded(px(5.))
                        .bg(c(ui::raised()))
                        .border_1()
                        .border_color(c(ui::line()))
                        .overflow_hidden()
                        .child(head)
                        .child(lines)
                        .child(foot),
                ),
        )
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
                let notes = self.line_notes.iter().filter(|n| n.job == id).count();
                let words = !self.peek_note.read(cx).value().trim().is_empty();
                let sending = notes > 0 || words;
                let approve = ui::dbtn(
                    "peek-approve",
                    "Approve",
                    if sending {
                        ui::Btn::Plain
                    } else {
                        ui::Btn::Primary
                    },
                )
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                })
                .when(blocked, |d| d.opacity(0.35).cursor_default())
                .when(!blocked, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| this.approve(id, cx)))
                });
                let back = match notes {
                    0 => "Send back".to_string(),
                    1 => "Send back \u{b7} 1 comment".to_string(),
                    n => format!("Send back \u{b7} {n} comments"),
                };
                Some(
                    foot.child(
                        ui::dbtn("peek-discard", "Discard", ui::Btn::Ghost).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.reply_open = None;
                                this.discard_job(id, cx)
                            },
                        )),
                    )
                    .child(div().flex_1())
                    .when(blocked && !sending, |d| {
                        d.child(
                            div()
                                .mr(px(4.))
                                .font_family(ui::MONO)
                                .text_size(px(10.5))
                                .text_color(c(ui::faint()))
                                .child(if held { "HELD" } else { "CLASH" }),
                        )
                    })
                    .child(
                        ui::dbtn(
                            "peek-send-back",
                            back,
                            if sending {
                                ui::Btn::Primary
                            } else {
                                ui::Btn::Plain
                            },
                        )
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "Click a line in the diff to comment on it, or write in the box",
                            )
                            .build(window, cx)
                        })
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                let words = this.peek_note.read(cx).value().to_string();
                                if this.line_notes.iter().any(|n| n.job == id)
                                    || !words.trim().is_empty()
                                {
                                    this.peek_note
                                        .update(cx, |i, cx| i.set_value("", window, cx));
                                    this.send_back_with(id, words, cx);
                                } else {
                                    this.peek_note.update(cx, |i, cx| i.focus(window, cx));
                                }
                            },
                        )),
                    )
                    .child(approve),
                )
            }
            Status::Shipping if !job.pr.is_empty() => {
                let url = job.pr.clone();
                Some(
                    foot.child(
                        ui::dbtn("peek-close-pr", "Close PR", ui::Btn::Ghost).on_click(
                            cx.listener(move |this, _, _, cx| this.land_pr(id, false, cx)),
                        ),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::dbtn("peek-gh", "View on GitHub", ui::Btn::Plain)
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                    )
                    .child(
                        ui::dbtn("peek-merge", "Merge now", ui::Btn::Primary).on_click(
                            cx.listener(move |this, _, _, cx| this.land_pr(id, true, cx)),
                        ),
                    ),
                )
            }
            Status::Shipping => Some(
                foot.child(
                    ui::dbtn("peek-ship-stop", "Stop", ui::Btn::Ghost)
                        .on_click(cx.listener(move |this, _, _, cx| this.stop_job(id, cx))),
                )
                .child(div().flex_1())
                .child(div().text_size(px(12.5)).text_color(c(ui::body())).child(
                    if job.shipping {
                        "Opening a pull request\u{2026}"
                    } else {
                        "Waiting for Shipper\u{2026}"
                    },
                )),
            ),
            Status::Shipped => {
                let url = job.pr.clone();
                Some(
                    foot.child(div().flex_1())
                        .when(!url.is_empty(), |d| {
                            d.child(
                                ui::dbtn("peek-gh", "View on GitHub", ui::Btn::Plain)
                                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                            )
                        })
                        .child(ui::dbtn("peek-done", "Close", ui::Btn::Plain).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.reply_open = None;
                                cx.notify();
                            }),
                        )),
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

/// A step or check as a row: a filled circle with a tick when done (0), a
/// ring while it runs (1), an empty ring still to come (2), red when it
/// failed (3).
fn step_row(state: u8, text: String, divided: bool) -> Div {
    let (ring, fill) = match state {
        0 => (ui::t_green(), Some(ui::t_green())),
        1 => (ui::t_blue(), None),
        3 => (ui::t_red(), Some(ui::t_red())),
        _ => (ui::line_hi(), None),
    };
    let mark = div()
        .size(px(15.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border(px(1.5))
        .border_color(c(ring))
        .when_some(fill, |d, f| d.bg(c(f)))
        .map(|d| match state {
            0 => d.child(
                Icon::new(IconName::Check)
                    .size(px(10.))
                    .text_color(c(0xFFFFFF)),
            ),
            3 => d.child(Icon::new(IconName::X).size(px(10.)).text_color(c(0xFFFFFF))),
            1 => d.child(div().size(px(6.)).rounded_full().bg(c(ring))),
            _ => d,
        });
    div()
        .py(px(6.))
        .flex()
        .items_center()
        .gap(px(9.))
        .when(divided, |d| {
            d.border_t_1().border_color(c(ui::line())).border_dashed()
        })
        .child(mark)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(13.))
                .when(state == 1, |d| d.font_weight(FontWeight::SEMIBOLD))
                .text_color(c(if state == 2 { ui::faint() } else { ui::ink() }))
                .child(text),
        )
}

/// A quiet line of explanation.
fn note_text(text: String) -> Div {
    div()
        .text_size(px(12.5))
        .line_height(px(18.))
        .text_color(c(ui::body()))
        .child(text)
}

/// "Checks running: 3 of 5 done" as (3, 5).
fn pr_counts(line: &str) -> (Option<usize>, Option<usize>) {
    let words: Vec<&str> = line.split_whitespace().collect();
    for w in words.windows(3) {
        if w[1] == "of" {
            if let (Ok(a), Ok(b)) = (
                w[0].trim_start_matches(':').parse::<usize>(),
                w[2].trim_end_matches(',').parse::<usize>(),
            ) {
                return (Some(a), Some(b));
            }
        }
    }
    (None, None)
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
    use super::{pr_counts, Tab};

    #[test]
    fn reads_how_many_checks_are_done() {
        assert_eq!(pr_counts("Checks running: 3 of 5 done"), (Some(3), Some(5)));
        assert_eq!(pr_counts("All checks passed"), (None, None));
    }

    #[test]
    fn every_tab_has_a_name_and_changes_comes_first() {
        assert_eq!(Tab::default(), Tab::Changes);
        assert_eq!(Tab::ALL[0], Tab::Changes);
        for t in Tab::ALL {
            assert!(!t.name().is_empty());
        }
    }
}
