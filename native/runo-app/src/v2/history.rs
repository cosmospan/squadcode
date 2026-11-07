//! Inbox (finished work you haven't looked at, and jobs that stopped with a
//! problem) and All jobs (every job, grouped by where it stands).

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use runo_holo::{Framing, Mood};

use super::store::{self, Job, Status};
use super::ui::{self, c, ca};
use super::Shell;

impl Shell {
    /// Inbox: what needs you on the left, the picked item on the right with what
    /// the bot said, what it changed, and what you can do about it.
    pub(super) fn inbox(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let jobs: Vec<Job> = self
            .crew
            .store
            .listed()
            .into_iter()
            .filter(|j| j.needs_you())
            .cloned()
            .collect();
        if jobs.is_empty() {
            return self.empty_place(
                "You're all caught up",
                "Finished work you haven't looked at, and anything that needs you, shows up here.",
                cx,
            );
        }
        let pick = self
            .inbox_pick
            .filter(|id| jobs.iter().any(|j| j.id == *id))
            .unwrap_or(jobs[0].id);
        let count = jobs.len();
        let problems = jobs
            .iter()
            .filter(|j| {
                matches!(
                    j.status,
                    Status::Failed | Status::Stopped | Status::Interrupted
                )
            })
            .count();

        let mut list = div()
            .id("inbox-list")
            .flex_1()
            .overflow_y_scroll()
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.));
        for (n, j) in jobs.iter().enumerate() {
            let id = j.id;
            let on = id == pick;
            let face = self.avatar(j.bot, 30.);
            let (word, tone) = super::home::status_of(j);
            list = list.child(
                div()
                    .id(("inbox", id as usize))
                    .map(|d| super::motion::enter(d, super::motion::key(("inbox", id)), n))
                    .flex()
                    .gap(px(12.))
                    .p(px(14.))
                    .rounded(px(10.))
                    .border_1()
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            // The one you're reading: lifted, with a firmer edge.
                            d.bg(c(ui::raised())).border_color(c(ui::line_hi()))
                        } else {
                            d.bg(c(ui::panel()))
                                .border_color(c(ui::line2()))
                                .hover(|s| s.border_color(c(ui::line_hi())))
                        }
                    })
                    .child(div().flex_none().child(face))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(j.label().to_string()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.))
                                            .text_color(c(ui::faint()))
                                            .child(store::ago(j.ended.unwrap_or(j.started))),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(c(ui::muted()))
                                    .truncate()
                                    .child(format!(
                                        "{} · #{} · {}",
                                        j.bot.name(),
                                        id,
                                        j.folder.name
                                    )),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .flex()
                                    .child(super::home::pill(&word, tone)),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.inbox_pick = Some(id);
                        cx.notify();
                    })),
            );
        }

        let left = div()
            .w(px(380.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(c(ui::line2()))
            .child(
                div()
                    .px(px(20.))
                    .pt(px(22.))
                    .pb(px(10.))
                    .flex()
                    .items_end()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Needs you"),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text_size(px(12.5))
                                    .text_color(c(ui::muted()))
                                    .child(match problems {
                                        0 => format!("{count} waiting on you"),
                                        p => {
                                            format!("{} to look at · {p} stopped early", count - p)
                                        }
                                    }),
                            ),
                    )
                    .child(
                        ui::ghost_i("mark-all", IconName::CheckCheck, "Mark all seen")
                            .h(px(28.))
                            .text_size(px(12.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                for j in &mut this.crew.store.jobs {
                                    if j.status != Status::Running {
                                        j.seen = true;
                                    }
                                }
                                this.crew.store.save();
                                cx.notify();
                            })),
                    ),
            )
            .child(list);

        let j = jobs
            .iter()
            .find(|j| j.id == pick)
            .cloned()
            .unwrap_or_else(|| jobs[0].clone());
        let right = self.inbox_detail(&j, cx);
        div()
            .size_full()
            .flex()
            .child(left)
            .child(right)
            .into_any_element()
    }

    fn inbox_detail(&mut self, j: &Job, cx: &mut Context<Self>) -> impl IntoElement {
        let id = j.id;
        let face = self.bot(j.bot, runo_holo::Mood::Idle, 56., runo_holo::Framing::Bust);
        let headline = match j.status {
            Status::Review => format!("{} has changes for you to review", j.bot.name()),
            Status::Done if j.files.is_empty() => format!("{} finished", j.bot.name()),
            Status::Done => format!(
                "{} changed {} {}",
                j.bot.name(),
                j.files.len(),
                if j.files.len() == 1 { "file" } else { "files" }
            ),
            Status::Failed => format!("{} hit a problem", j.bot.name()),
            _ => format!("{} stopped before the end", j.bot.name()),
        };
        let said = j
            .last_words()
            .map(super::live::plain)
            .unwrap_or_else(|| "No reply was kept for this job.".into());
        let mut said_lines = div().flex().flex_col().gap(px(8.));
        for para in said.split("\n\n").filter(|p| !p.trim().is_empty()).take(6) {
            said_lines = said_lines.child(div().child(para.trim().to_string()));
        }
        let mut review = self.review_card(j, cx);
        let mut files = ui::card().overflow_hidden().child(
            div()
                .h(px(40.))
                .px(px(16.))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(c(ui::line()))
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Files changed")
                .child(div().flex_1())
                .child(
                    div()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(c(ui::faint()))
                        .child(j.files.len().to_string()),
                ),
        );
        if j.files.is_empty() {
            files = files.child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child("Nothing was changed."),
            );
        }
        for f in j.files.iter().take(12) {
            files = files.child(
                div()
                    .h(px(34.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(c(ui::line2()))
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .child(f.clone()),
            );
        }
        let mut plan = div().flex().flex_col().gap(px(7.));
        for (s, state) in &j.plan {
            plan = plan.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .text_size(px(12.5))
                    .text_color(c(if *state == 2 { ui::body() } else { ui::faint() }))
                    .child(if *state == 2 { "✓" } else { "·" })
                    .child(s.clone()),
            );
        }
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("inbox-detail")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(28.))
                    .pt(px(24.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div().flex().items_center().gap(px(16.)).child(face).child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(headline),
                                )
                                .child(
                                    div()
                                        .mt(px(3.))
                                        .text_size(px(12.5))
                                        .text_color(c(ui::muted()))
                                        .truncate()
                                        .child(format!(
                                            "{} · Job #{id} · {}{}",
                                            j.label(),
                                            if j.engine.is_empty() {
                                                "—"
                                            } else {
                                                &j.engine
                                            },
                                            if j.summary.is_empty() {
                                                String::new()
                                            } else {
                                                format!(" · {}", j.summary)
                                            }
                                        )),
                                ),
                        ),
                    )
                    .child(
                        ui::card()
                            .px(px(18.))
                            .py(px(14.))
                            .text_size(px(13.5))
                            .line_height(px(21.))
                            .text_color(c(ui::body()))
                            .child(said_lines),
                    )
                    .children(self.trouble_card(j, cx).map(|t| t.ml(px(0.))))
                    .map(|d| match review.take() {
                        Some(card) => d.child(card.ml(px(0.))),
                        None => d.child(files),
                    })
                    .when(!j.plan.is_empty(), |d| {
                        d.child(
                            ui::card()
                                .px(px(18.))
                                .py(px(14.))
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(c(ui::faint()))
                                        .child("PLAN"),
                                )
                                .child(plan),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(60.))
                    .px(px(28.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .bg(c(ui::panel()))
                    .border_t_1()
                    .border_color(c(ui::line()))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .text_color(c(ui::muted()))
                            .child(match j.status {
                                Status::Review => {
                                    "Approve it on the card above, or send it back with changes."
                                }
                                Status::Done => "Look it over, then mark it seen or ask for more.",
                                _ => "Open the job to see where it stopped, or ask it to carry on.",
                            }),
                    )
                    .child(
                        ui::ghost(
                            "inbox-open",
                            if matches!(j.status, Status::Done | Status::Review) {
                                "Send back"
                            } else {
                                "Open job"
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
                    )
                    .when(j.status != Status::Review, |d| {
                        d.child(
                            ui::solid_i("inbox-seen", IconName::Check, "Mark as seen").on_click(
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(j) =
                                        this.crew.store.jobs.iter_mut().find(|j| j.id == id)
                                    {
                                        j.seen = true;
                                    }
                                    this.inbox_pick = None;
                                    this.crew.store.save();
                                    cx.notify();
                                }),
                            ),
                        )
                    }),
            )
    }

    /// All jobs: what needs you, what's running and what finished, as cards in
    /// each bot's colour. Only what a job has shows; nothing is a column of dashes.
    pub(super) fn history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.crew.store.jobs.is_empty() {
            return self.empty_place(
                "No jobs yet",
                "Every job the crew takes on is kept here, with what it said and changed.",
                cx,
            );
        }
        let query = self.history_input.read(cx).value().to_lowercase();
        let everything: Vec<Job> = self.crew.store.listed().into_iter().cloned().collect();
        let all: Vec<Job> = everything
            .iter()
            .filter(|j| {
                let hay = format!(
                    "{} {} {} {} {}",
                    j.title,
                    j.short,
                    j.bot.name(),
                    j.folder.name,
                    j.mode.label()
                )
                .to_lowercase();
                query.split_whitespace().all(|w| hay.contains(w))
            })
            .cloned()
            .collect();
        let (needs, going, done, trouble) = sort_jobs(&all);

        // The line under the title: how things stand, then what it all cost.
        let spend: f64 = everything
            .iter()
            .filter_map(|j| super::home::cost(&j.summary))
            .sum();
        let (n_need, n_run) = {
            let (n, g, _, _) = sort_jobs(&everything);
            (n.len(), g.len())
        };
        let mut said: Vec<String> = Vec::new();
        if n_run > 0 {
            said.push(format!("{n_run} running"));
        }
        if n_need > 0 {
            said.push(format!("{n_need} waiting on you"));
        }
        said.push(format!(
            "{} {} in all",
            everything.len(),
            if everything.len() == 1 { "job" } else { "jobs" }
        ));
        if spend > 0.0 {
            said.push(format!("${spend:.2} spent"));
        }

        let filter = self.history_filter.min(4);
        let mut filters = div()
            .flex()
            .items_center()
            .p(px(3.))
            .gap(px(2.))
            .rounded(px(10.))
            .bg(c(ui::well()))
            .border_1()
            .border_color(c(ui::line2()));
        for (i, label, n) in [
            (0usize, "All", all.len()),
            (1, "Needs you", needs.len()),
            (2, "Running", going.len()),
            (3, "Done", done.len()),
            (4, "Problems", trouble.len()),
        ] {
            let on = filter == i;
            filters = filters.child(
                div()
                    .id(("hist-filter", i))
                    .h(px(28.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(7.))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            let k = super::motion::shown(
                                super::motion::key(("hist-filter-on", i)),
                                super::motion::QUICK,
                                0.,
                            );
                            d.bg(ca(ui::raised(), k)).text_color(c(ui::ink()))
                        } else {
                            d.text_color(c(ui::muted()))
                                .hover(|s| s.text_color(c(ui::ink())))
                        }
                    })
                    .child(label)
                    .when(n > 0 && i > 0, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(c(if i == 4 { ui::t_red() } else { ui::faint() }))
                                .child(n.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.history_filter = i;
                        cx.notify();
                    })),
            );
        }

        let search = div()
            .w(px(280.))
            .h(px(36.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(10.))
            .bg(c(ui::panel()))
            .border_1()
            .border_color(c(ui::line()))
            .child(
                gpui_kit::component::Icon::new(IconName::Search)
                    .size(px(14.))
                    .text_color(c(ui::faint())),
            )
            .child(
                div().flex_1().text_size(px(13.)).child(
                    gpui_kit::component::input::Input::new(&self.history_input)
                        .appearance(false)
                        .bordered(false),
                ),
            );

        let groups = [
            (1usize, "Needs you", ui::AMBER, needs),
            (2, "Running", ui::BLUE, going),
            (4, "Stopped early", ui::RED, trouble),
            (3, "Done", ui::GREEN, done),
        ];
        let mut body = div().flex().flex_col().gap(px(26.));
        let mut any = false;
        let mut shown = 0;
        for (key, label, dot, rows) in groups {
            if (filter != 0 && filter != key) || rows.is_empty() {
                continue;
            }
            any = true;
            let mut cards = div().flex().flex_col().gap(px(8.));
            for j in &rows {
                let card = if j.status == Status::Running {
                    self.running_job_card(j, cx).into_any_element()
                } else {
                    self.job_card(j, cx).into_any_element()
                };
                let k = super::motion::key(("history-card", j.id));
                cards = cards.child(super::motion::enter(div().child(card), k, shown));
                shown += 1;
            }
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(ui::heading(label, Some(dot), Some(rows.len())).px(px(4.)))
                    .child(cards),
            );
        }
        if !any {
            body = body.child(
                div()
                    .py(px(28.))
                    .flex()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(c(ui::faint()))
                    .child(if query.trim().is_empty() {
                        "Nothing here.".to_string()
                    } else {
                        format!("No jobs match \u{201c}{}\u{201d}.", query.trim())
                    }),
            );
        }

        div()
            .id("jobs-page")
            .size_full()
            .overflow_y_scroll()
            .px(px(28.))
            .pt(px(26.))
            .pb(px(40. + super::DOCK_ROOM))
            .child(
                div()
                    .w_full()
                    .max_w(px(MAX_W))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(16.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(5.))
                                    .child(
                                        div()
                                            .text_size(px(24.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Jobs"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .text_color(c(ui::muted()))
                                            .child(said.join(" \u{b7} ")),
                                    ),
                            )
                            .child(search),
                    )
                    .child(div().flex().child(filters))
                    .child(body),
            )
            .into_any_element()
    }

    /// A job at work: one card in the bot's colour with the step it's on, how long
    /// so far, and a thin bar while the plan fills in.
    fn running_job_card(&mut self, j: &Job, cx: &mut Context<Self>) -> impl IntoElement {
        let id = j.id;
        let accent = j.bot.accent();
        let face = self.bot(j.bot, Mood::Working, 34., Framing::Bust);
        let frac = j
            .progress()
            .map(|(d, n)| self.glide_bar(id, d as f32 / n.max(1) as f32));
        let pulse = 0.4 + 0.6 * super::motion::breath();
        let what = match j.progress() {
            Some((d, n)) => format!("Step {} of {n}", (d + 1).min(n)),
            None => "Working".to_string(),
        };
        let edge = super::theme::mix(ui::line(), accent, 0.45);
        div()
            .id(("jobs-run", id as usize))
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(14.))
            .rounded(px(12.))
            .bg(c(ui::panel()))
            .border_1()
            .border_color(c(edge))
            .cursor_pointer()
            .hover(|s| s.border_color(c(accent)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(face_tile(face, accent, 44.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(title_line(j))
                            .child(meta_line(j, None)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .h(px(24.))
                            .px(px(10.))
                            .rounded_full()
                            .bg(ca(accent, 0.12))
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(super::theme::mix(accent, ui::ink(), 0.3)))
                            .child(
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(c(accent))
                                    .opacity(pulse),
                            )
                            .child(format!("{what} \u{b7} {}", store::span(j.seconds()))),
                    ),
            )
            .map(|d| match frac {
                Some(f) => d.child(ui::bar(f, accent).h(px(3.))),
                None => d.child(ui::sweep(accent, 0.).h(px(3.))),
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(8.))
                    .bg(ca(accent, 0.06))
                    .text_size(px(12.5))
                    .text_color(c(ui::body()))
                    .child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(c(accent))
                            .opacity(pulse),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(store::plain_step(&j.doing(), true)),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx)))
    }

    /// A job that isn't running: the bot's face on its colour, the title, how it
    /// ended, and the few facts it has (lines, files, time, cost) as chips.
    fn job_card(&mut self, j: &Job, cx: &mut Context<Self>) -> impl IntoElement {
        let id = j.id;
        let accent = j.bot.accent();
        let face = self.avatar(j.bot, 30.);
        let (word, tone) = super::home::status_of(j);
        let needs = j.needs_you();
        let files = if j.changes.is_empty() {
            j.files.len()
        } else {
            j.changes.len()
        };
        let (added, removed) = j.lines();
        let secs = j.seconds();
        let mut facts = div().flex_none().flex().items_center().gap(px(6.));
        if added + removed > 0 {
            facts = facts.child(
                chip()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_color(c(ui::t_green()))
                            .child(format!("+{added}")),
                    )
                    .child(
                        div()
                            .text_color(c(ui::t_red()))
                            .child(format!("\u{2212}{removed}")),
                    ),
            );
        }
        if files > 0 {
            facts = facts.child(chip().child(format!(
                "{files} {}",
                if files == 1 { "file" } else { "files" }
            )));
        }
        if secs > 0 && j.ended.is_some() {
            facts = facts.child(
                chip()
                    .gap(px(5.))
                    .child(
                        gpui_kit::component::Icon::new(IconName::Clock)
                            .size(px(11.))
                            .text_color(c(ui::faint())),
                    )
                    .child(store::span(secs)),
            );
        }
        if let Some(cost) = super::home::cost(&j.summary) {
            facts = facts.child(chip().child(format!("${cost:.2}")));
        }
        let edge = if needs {
            super::theme::mix(ui::line(), tone.0, 0.35)
        } else {
            ui::line2()
        };
        let hover_edge = super::theme::mix(ui::line(), accent, 0.6);
        div()
            .id(("jobs-card", id as usize))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(11.))
            .rounded(px(12.))
            .bg(c(ui::panel()))
            .border_1()
            .border_color(c(edge))
            .cursor_pointer()
            .hover(|s| s.bg(c(ui::raised())).border_color(c(hover_edge)))
            .child(face_tile(face, accent, 40.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(title_line(j))
                    .child(meta_line(j, Some((word, tone.0)))),
            )
            .child(facts)
            .child(
                div()
                    .w(px(34.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child(match store::ago(j.ended.unwrap_or(j.started)).as_str() {
                        "just now" => "now".to_string(),
                        a => a.to_string(),
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx)))
    }
}

/// How wide the jobs page's column grows: past this, cards only stretch empty space.
const MAX_W: f32 = 980.;

/// Splits jobs into needs you, running, done, and stopped early, keeping order.
fn sort_jobs(jobs: &[Job]) -> (Vec<Job>, Vec<Job>, Vec<Job>, Vec<Job>) {
    let mut out = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for j in jobs {
        let bin = if j.needs_you() {
            &mut out.0
        } else if matches!(j.status, Status::Running | Status::Shipping) {
            &mut out.1
        } else if matches!(
            j.status,
            Status::Failed | Status::Interrupted | Status::Stopped
        ) {
            &mut out.3
        } else {
            &mut out.2
        };
        bin.push(j.clone());
    }
    out
}

/// The bot's face on a soft square of its own colour.
fn face_tile(face: Img, accent: u32, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.27))
        .bg(ca(accent, 0.10))
        .border_1()
        .border_color(ca(accent, 0.24))
        .child(face)
}

/// A card's title: Peek's short one or what was typed, or a plain name when that
/// was keyboard mash.
fn title_line(j: &Job) -> Div {
    div()
        .min_w_0()
        .truncate()
        .text_size(px(13.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(ui::ink()))
        .child(super::channel::card_title(j))
}

/// Under the title: how it ended (when it has), the bot in its colour, the kind
/// of job, the folder and its number.
fn meta_line(j: &Job, status: Option<(String, u32)>) -> Div {
    let dot = || div().flex_none().text_color(c(ui::faint())).child("\u{b7}");
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w_0()
        .text_size(px(12.))
        .text_color(c(ui::muted()))
        .when_some(status, |d, (word, col)| {
            d.child(div().size(px(6.)).flex_none().rounded_full().bg(c(col)))
                .child(div().flex_none().text_color(c(col)).child(word))
                .child(dot())
        })
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(super::theme::mix(j.bot.accent(), ui::ink(), 0.35)))
                .child(j.bot.name()),
        )
        .child(dot())
        .child(div().flex_none().child(j.mode.label()))
        .child(dot())
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(format!("{} \u{b7} #{}", j.folder.name, j.id)),
        )
}

/// A small fact on a card, like "3 files" or "$0.41".
fn chip() -> Div {
    div()
        .h(px(22.))
        .px(px(8.))
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(6.))
        .bg(c(ui::well()))
        .border_1()
        .border_color(c(ui::line2()))
        .font_family(ui::MONO)
        .text_size(px(11.))
        .text_color(c(ui::muted()))
}

#[cfg(test)]
mod tests {
    use super::super::store::{Job, Status};
    use super::sort_jobs;

    #[test]
    fn jobs_land_in_one_group_each() {
        let job = |id, status, seen| Job {
            id,
            status,
            seen,
            ..Job::default()
        };
        let jobs = vec![
            job(1, Status::Running, true),
            job(2, Status::Done, false),
            job(3, Status::Review, true),
            job(4, Status::Failed, true),
            job(5, Status::Done, true),
            job(6, Status::Shipping, true),
        ];
        let (needs, going, done, trouble) = sort_jobs(&jobs);
        let ids = |v: Vec<Job>| v.iter().map(|j| j.id).collect::<Vec<_>>();
        assert_eq!(ids(needs), [2, 3]);
        assert_eq!(ids(going), [1, 6]);
        assert_eq!(ids(done), [5]);
        assert_eq!(ids(trouble), [4]);
    }
}
