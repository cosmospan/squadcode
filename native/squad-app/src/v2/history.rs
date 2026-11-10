//! Inbox (finished work you haven't looked at, and jobs that stopped with a
//! problem) and All jobs (every job, grouped by where it stands).

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::{Framing, Mood};

use super::store::{self, Job, Status};
use super::ui::{self, c, ca};
use super::Shell;

impl Shell {
    /// The jobs in the Inbox, in the order the list shows them.
    pub(super) fn inbox_ids(&self) -> Vec<u64> {
        let mut jobs: Vec<&Job> = self
            .crew
            .store
            .listed()
            .into_iter()
            .filter(|j| j.needs_you())
            .filter(|j| {
                self.inbox_project
                    .as_ref()
                    .is_none_or(|p| &j.folder.path == p)
            })
            .collect();
        jobs.sort_by_key(|j| inbox_group(j).0);
        jobs.into_iter().map(|j| j.id).collect()
    }

    /// Needs you from the keyboard: arrows (or J and K) move, Enter opens the
    /// job, A approves changes that can go, E marks the rest seen. True when
    /// the key was used.
    pub(super) fn inbox_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let ids = self.inbox_ids();
        if ids.is_empty() {
            return false;
        }
        let at = self
            .inbox_pick
            .and_then(|p| ids.iter().position(|i| *i == p))
            .unwrap_or(0);
        let id = ids[at];
        match key {
            "down" | "j" => self.inbox_pick = Some(ids[(at + 1).min(ids.len() - 1)]),
            "up" | "k" => self.inbox_pick = Some(ids[at.saturating_sub(1)]),
            "enter" => self.open_job(id, cx),
            "a" => {
                let Some(j) = self.crew.store.job(id) else {
                    return false;
                };
                let failing = !j.checking
                    && super::work::verdict(&j.checks, false).1 == super::work::Verdict::Fail;
                if j.status != Status::Review
                    || j.changes.is_empty()
                    || !j.conflict.is_empty()
                    || failing
                {
                    return false;
                }
                self.inbox_pick = ids
                    .get(at + 1)
                    .or(at.checked_sub(1).and_then(|p| ids.get(p)))
                    .copied();
                self.approve(id, cx);
            }
            "e" => {
                let Some(j) = self.crew.store.job_mut(id) else {
                    return false;
                };
                if j.status == Status::Review {
                    return false;
                }
                j.seen = true;
                self.inbox_pick = ids
                    .get(at + 1)
                    .or(at.checked_sub(1).and_then(|p| ids.get(p)))
                    .copied();
                self.crew.store.save();
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Inbox: what needs you on the left, the picked item on the right with what
    /// the bot said, what it changed, and what you can do about it.
    pub(super) fn inbox(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let every: Vec<Job> = self
            .crew
            .store
            .listed()
            .into_iter()
            .filter(|j| j.needs_you())
            .cloned()
            .collect();
        // One Inbox for every project, with a chip per project to narrow it.
        let mut projects: Vec<(String, String, usize)> = Vec::new();
        for j in &every {
            match projects.iter_mut().find(|p| p.0 == j.folder.path) {
                Some(p) => p.2 += 1,
                None => projects.push((j.folder.path.clone(), j.folder.name.clone(), 1)),
            }
        }
        if self
            .inbox_project
            .as_ref()
            .is_some_and(|p| !projects.iter().any(|x| &x.0 == p))
        {
            self.inbox_project = None;
        }
        let only = self.inbox_project.clone();
        let mut jobs: Vec<Job> = every
            .iter()
            .filter(|j| only.as_ref().is_none_or(|p| &j.folder.path == p))
            .cloned()
            .collect();
        // Ready for review first, then what stopped, then the rest.
        jobs.sort_by_key(|j| inbox_group(j).0);
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
            .px(px(10.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(2.));
        let mut group = u8::MAX;
        for (n, j) in jobs.iter().enumerate() {
            let (g, heading) = inbox_group(j);
            if g != group {
                group = g;
                let in_group = jobs.iter().filter(|x| inbox_group(x).0 == g).count();
                list = list.child(
                    div()
                        .pt(px(if n == 0 { 4. } else { 14. }))
                        .pb(px(4.))
                        .px(px(6.))
                        .flex()
                        .justify_between()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::faint()))
                        .child(heading.to_uppercase())
                        .child(in_group.to_string()),
                );
            }
            let id = j.id;
            let on = id == pick;
            let face = self.avatar(j.bot, 26.);
            let (word, tone) = super::home::status_of(j);
            list = list.child(
                div()
                    .id(("inbox", id as usize))
                    .map(|d| super::motion::enter(d, super::motion::key(("inbox", id)), n))
                    .relative()
                    .flex()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(11.))
                    .rounded(px(7.))
                    .border_1()
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            // The one you're reading: lifted, with the
                            // accent bar down its left edge.
                            d.bg(c(ui::raised())).border_color(c(ui::line())).child(
                                div()
                                    .absolute()
                                    .left(px(-1.))
                                    .top(px(-1.))
                                    .bottom(px(-1.))
                                    .w(px(4.))
                                    .rounded_l(px(7.))
                                    .bg(c(ui::pick_fill())),
                            )
                        } else {
                            d.border_color(transparent_black())
                                .hover(|s| s.bg(c(ui::hover())))
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
                                    .font_family(ui::MONO)
                                    .text_size(px(11.))
                                    .text_color(c(ui::faint()))
                                    .truncate()
                                    .child(format!(
                                        "{} · #{} · {}",
                                        j.bot.name(),
                                        id,
                                        j.folder.name
                                    )),
                            )
                            .child(div().mt(px(3.)).child(ui::mono_status(tone.0, &word))),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        self.on_menu(super::menu::Ctx::Job(id), cx),
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
                                    .child("Inbox"),
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
            .when(projects.len() > 1, |d| {
                let mut chips = div()
                    .px(px(20.))
                    .pb(px(8.))
                    .flex()
                    .flex_wrap()
                    .gap(px(6.));
                let all = every.len();
                let rows = std::iter::once((None, "All".to_string(), all)).chain(
                    projects
                        .iter()
                        .map(|(p, name, n)| (Some(p.clone()), name.clone(), *n)),
                );
                for (k, (path, name, n)) in rows.enumerate() {
                    let on = path == only;
                    chips = chips.child(
                        div()
                            .id(("inbox-project", k))
                            .h(px(26.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(5.))
                            .border_1()
                            .text_size(px(12.))
                            .cursor_pointer()
                            .map(|d| {
                                if on {
                                    d.bg(c(ui::ink()))
                                        .border_color(c(ui::ink()))
                                        .text_color(c(ui::theme().bg & 0xff_ffff))
                                } else {
                                    d.border_color(c(ui::line()))
                                        .text_color(c(ui::muted()))
                                        .hover(|s| s.text_color(c(ui::ink())))
                                }
                            })
                            .child(name)
                            .child(div().font_family(ui::MONO).opacity(0.7).child(n.to_string()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.inbox_project = path.clone();
                                this.inbox_pick = None;
                                cx.notify();
                            })),
                    );
                }
                d.child(chips)
            })
            .child(list)
            // The keyboard, said once at the foot.
            .child(
                div()
                    .flex_none()
                    .px(px(20.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(c(ui::line2()))
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(ui::faint()))
                    .child("\u{2191}\u{2193} move \u{b7} \u{21b5} open \u{b7} A approve \u{b7} E mark seen"),
            );

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

    /// The picked job, the design's way: its crumb with Open job, the bot and
    /// title, what the bot said, what holds it, its checks and changes side by
    /// side, and the decision at the foot.
    fn inbox_detail(&mut self, j: &Job, cx: &mut Context<Self>) -> impl IntoElement {
        let id = j.id;
        let face = self.bot(
            j.bot,
            squad_holo::Mood::Idle,
            44.,
            squad_holo::Framing::Bust,
        );
        let project = j.folder.name.to_uppercase();
        let thread = self.crew.store.thread_label(j.thread).to_uppercase();
        let head = div()
            .h(px(50.))
            .flex_none()
            .px(px(20.))
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
                    .child(format!("/ {project} / #{thread}")),
            )
            .child(
                ui::dbtn("inbox-open", "Open job", ui::Btn::Plain)
                    .child(ui::kbd("\u{21b5}"))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
            );
        let mut meta = vec![j.bot.name().to_string()];
        if !j.engine.is_empty() {
            meta.push(j.engine.clone());
        }
        if !j.summary.is_empty() {
            meta.push(j.summary.clone());
        }
        let title = div().flex().items_start().gap(px(14.)).child(face).child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(20.))
                        .line_height(px(26.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(super::channel::card_title(j)),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .truncate()
                        .font_family(ui::MONO)
                        .text_size(px(11.5))
                        .text_color(c(ui::body()))
                        .child(meta.join(" \u{b7} ")),
                ),
        );
        let said = j
            .last_words()
            .map(super::live::plain)
            .unwrap_or_else(|| "No reply was kept for this job.".into());
        let mut said_lines = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(14.))
            .line_height(px(22.))
            .text_color(c(ui::ink()));
        for para in said.split("\n\n").filter(|p| !p.trim().is_empty()).take(6) {
            said_lines = said_lines.child(div().child(para.trim().to_string()));
        }
        let held = j.status == Status::Review && !j.held.is_empty() && !j.belt_ok;
        let belt = held.then(|| self.belt_box(j, cx));
        let clash = self.clash_box(j, cx);
        let trouble = self.trouble_card(j, cx).map(|t| t.ml(px(0.)));
        let in_the_way = self.in_the_way_card(j, cx);
        let shipping = matches!(j.status, Status::Shipping | Status::Shipped)
            .then(|| self.review_body(j, cx))
            .flatten();
        let checks = super::peek::peek_checks(j);
        let changes = self.peek_changes(j, cx);
        let lists = (checks.is_some() || changes.is_some()).then(|| {
            div()
                .flex()
                .gap(px(20.))
                .child(div().flex_1().min_w_0().children(checks))
                .child(div().flex_1().min_w_0().children(changes))
        });

        // The foot: a hint, then the decision.
        let review = j.status == Status::Review && !j.changes.is_empty();
        let blocked = held || !j.conflict.is_empty();
        let full = self.approve_label(j);
        let hint = match j.status {
            Status::Review => "A approves \u{b7} \u{21b5} opens the job to send it back",
            Status::Done => "E marks it seen \u{b7} \u{21b5} opens the job",
            _ => "E marks it seen \u{b7} \u{21b5} opens the job to carry on",
        };
        let foot = div()
            .flex_none()
            .px(px(28.))
            .py(px(12.))
            .flex()
            .items_center()
            .gap(px(4.))
            .border_t_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(ui::faint()))
                    .child(hint),
            )
            .when(review, |d| {
                d.child(
                    ui::dbtn("inbox-discard", "Discard", ui::Btn::Ghost)
                        .on_click(cx.listener(move |this, _, _, cx| this.discard_job(id, cx))),
                )
                .child(
                    ui::dbtn("inbox-send-back", "Send back", ui::Btn::Plain)
                        .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
                )
                .when(blocked, |d| {
                    d.child(
                        div()
                            .ml(px(6.))
                            .font_family(ui::MONO)
                            .text_size(px(10.5))
                            .text_color(c(ui::faint()))
                            .child(if held { "HELD" } else { "CLASH" }),
                    )
                })
                .child(
                    ui::dbtn("inbox-approve", "Approve", ui::Btn::Primary)
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(full.clone())
                                .build(window, cx)
                        })
                        .when(blocked, |d| d.opacity(0.35).cursor_default())
                        .when(!blocked, |d| {
                            d.on_click(cx.listener(move |this, _, _, cx| this.approve(id, cx)))
                        }),
                )
            })
            .when(!review, |d| {
                d.child(
                    ui::dbtn("inbox-seen", "Mark as seen", ui::Btn::Primary).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if let Some(j) = this.crew.store.jobs.iter_mut().find(|j| j.id == id) {
                                j.seen = true;
                            }
                            this.inbox_pick = None;
                            this.crew.store.save();
                            cx.notify();
                        },
                    )),
                )
            });
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(c(ui::panel()))
            .child(head)
            .child(
                div()
                    .id("inbox-detail")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(28.))
                    .py(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(title)
                    .child(said_lines)
                    .children(belt)
                    .children(clash)
                    .children(trouble)
                    .children(in_the_way)
                    .children(shipping)
                    .children(lists),
            )
            .child(foot)
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
        // This runs on every frame: the cards need a job's fields but not its
        // whole conversation, so each job is copied with only its last few
        // feed lines (enough for what it's doing now).
        let order: Vec<u64> = self.crew.store.listed().into_iter().map(|j| j.id).collect();
        let mut light: std::collections::HashMap<u64, Job> =
            std::collections::HashMap::with_capacity(order.len());
        {
            let want: std::collections::HashSet<u64> = order.iter().copied().collect();
            for j in self
                .crew
                .store
                .jobs
                .iter_mut()
                .filter(|j| want.contains(&j.id))
            {
                let feed = std::mem::take(&mut j.feed);
                let mut copy = j.clone();
                copy.feed = feed[feed.len().saturating_sub(8)..].to_vec();
                j.feed = feed;
                light.insert(copy.id, copy);
            }
        }
        let everything: Vec<Job> = order.iter().filter_map(|id| light.remove(id)).collect();
        let words: Vec<&str> = query.split_whitespace().collect();
        let all: Vec<Job> = everything
            .iter()
            .filter(|j| {
                if words.is_empty() {
                    return true;
                }
                let hay = format!(
                    "{} {} {} {} {}",
                    j.title,
                    j.short,
                    j.bot.name(),
                    j.folder.name,
                    j.mode.label()
                )
                .to_lowercase();
                words.iter().all(|w| hay.contains(w))
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
            "{} {} kept",
            everything.len(),
            if everything.len() == 1 { "job" } else { "jobs" }
        ));
        if spend > 0.0 {
            said.push(format!("${spend:.2} spent on them"));
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
            // Needs you has its own page, so it isn't a filter here too.
            (0usize, "All", all.len()),
            (2, "Working", going.len()),
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
            .w(px(220.))
            .h(px(32.))
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
            (2usize, "Working", ui::t_blue(), going),
            (1, "To review", ui::t_amber(), needs),
            (4, "Stopped", ui::t_red(), trouble),
            (3, "Done", ui::t_green(), done),
        ];
        // The board: a column per state, side by side. The list: one under another.
        let board = !self.jobs_list;
        let mut body = if board {
            div().flex().items_start().gap(px(16.))
        } else {
            div().flex().flex_col().gap(px(26.))
        };
        let mut any = false;
        let mut shown = 0;
        for (key, label, dot, rows) in groups {
            if board {
                // Every column shows, empty or not, so the board keeps its shape.
                any = true;
                let mut cards = div().flex().flex_col().gap(px(8.));
                for j in &rows {
                    cards = cards.child(self.board_card(j, cx));
                }
                if rows.is_empty() {
                    cards = cards.child(
                        div()
                            .py(px(18.))
                            .flex()
                            .justify_center()
                            .rounded(px(7.))
                            .border_1()
                            .border_dashed()
                            .border_color(c(ui::line()))
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child("Nothing here"),
                    );
                }
                body = body.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(col_head(label, dot, rows.len()))
                        .child(cards),
                );
                continue;
            }
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

        // Under the board: what waits for the night shift, and what it did.
        let night = self.night_shift(cx);
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
                    .when(!board, |d| d.max_w(px(MAX_W)))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
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
                                            .text_size(px(22.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Jobs"),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(13.))
                                            .text_color(c(ui::muted()))
                                            .child(said.join(" \u{b7} ")),
                                    ),
                            )
                            .child(self.board_toggle(board, cx))
                            .child(search)
                            .child(ui::solid_i("jobs-new", IconName::Plus, "New job").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.set_mode(super::data::Mode::Build, window, cx);
                                    this.go(super::Place::Today, cx);
                                    this.composer.update(cx, |i, cx| i.focus(window, cx));
                                    this.focus_moved = std::time::Instant::now();
                                }),
                            )),
                    )
                    .when(!board, |d| d.child(div().flex().child(filters)))
                    .child(body)
                    .children(night),
            )
            .into_any_element()
    }

    /// Board or List, as two halves of one switch.
    fn board_toggle(&mut self, board: bool, cx: &mut Context<Self>) -> Div {
        let mut d = div()
            .flex()
            .flex_none()
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(6.))
            .border_1()
            .border_color(c(ui::line()));
        for (i, label, icon) in [
            (0usize, "Board", IconName::Columns2),
            (1, "List", IconName::ListTodo),
        ] {
            let on = (i == 0) == board;
            d = d.child(
                div()
                    .id(("jobs-view", i))
                    .h(px(26.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(4.))
                    .text_size(px(12.5))
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.bg(c(ui::pick_fill()))
                                .text_color(c(ui::pick_on()))
                                .font_weight(FontWeight::SEMIBOLD)
                        } else {
                            d.text_color(c(ui::muted()))
                                .hover(|s| s.text_color(c(ui::ink())))
                        }
                    })
                    .child(gpui_kit::component::Icon::new(icon).size(px(13.)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.jobs_list = i == 1;
                        cx.notify();
                    })),
            );
        }
        d
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
                Some(f) => d.child(super::channel::dashed_bar(f, ui::t_blue())),
                None => d.child(super::channel::dashed_bar(0.5, ui::t_blue())),
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
            .on_mouse_down(
                MouseButton::Right,
                self.on_menu(super::menu::Ctx::Job(id), cx),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx)))
    }

    /// A board column's card: narrow, so it stacks. The bot and the job's
    /// number, the title (two lines at most), a dashed bar while it runs,
    /// then how it stands in small mono capitals, with the lines it changed
    /// and when on the right.
    fn board_card(&mut self, j: &Job, cx: &mut Context<Self>) -> impl IntoElement {
        let id = j.id;
        let running = j.status == Status::Running;
        let face = self.avatar(j.bot, 20.);
        let (word, tone) = super::home::status_of(j);
        let (added, removed) = j.lines();
        let when = match store::ago(j.ended.unwrap_or(j.started)).as_str() {
            "just now" => "now".to_string(),
            a => a.to_string(),
        };
        let frac = if running {
            j.progress()
                .map(|(d, n)| self.glide_bar(id, d as f32 / n.max(1) as f32))
        } else {
            None
        };
        let pulse = 0.4 + 0.6 * super::motion::breath();
        let (state, col, right) = if running {
            let what = match j.progress() {
                Some((d, n)) => format!("Step {} of {n}", (d + 1).min(n)),
                None => "Working".to_string(),
            };
            (what, ui::t_blue(), store::span(j.seconds()))
        } else {
            let mut right = Vec::new();
            if added + removed > 0 {
                right.push(format!("+{added} \u{2212}{removed}"));
            }
            right.push(when);
            (word, tone.0, right.join(" \u{b7} "))
        };
        let foot = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .font_family(ui::MONO)
            .text_size(px(10.5))
            .child(
                div()
                    .size(px(6.))
                    .flex_none()
                    .bg(c(col))
                    .when(running, |d| d.opacity(pulse)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(col))
                    .child(state.to_uppercase()),
            )
            .child(div().flex_none().text_color(c(ui::faint())).child(right));
        div()
            .id(("jobs-board", id as usize))
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .rounded(px(7.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .cursor_pointer()
            .hover(|s| s.border_color(c(ui::line_hi())))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w_0()
                    .child(div().flex_none().child(face))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(j.bot.name()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(ui::MONO)
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child(format!("#{}", j.id)),
                    ),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .line_height(px(19.))
                    .max_h(px(38.))
                    .overflow_hidden()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(super::channel::card_title(j)),
            )
            .when(running, |d| {
                d.child(match frac {
                    Some(f) => super::channel::dashed_bar(f, ui::t_blue()),
                    None => super::channel::dashed_bar(0.5, ui::t_blue()),
                })
            })
            .child(foot)
            .on_mouse_down(
                MouseButton::Right,
                self.on_menu(super::menu::Ctx::Job(id), cx),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx)))
    }

    /// A job that isn't running: the bot's face on its colour, the title, how it
    /// ended, the lines it changed and when. Files, time and cost are on its page.
    fn job_card(&mut self, j: &Job, cx: &mut Context<Self>) -> impl IntoElement {
        let id = j.id;
        let accent = j.bot.accent();
        let face = self.avatar(j.bot, 30.);
        let (word, tone) = super::home::status_of(j);
        let needs = j.needs_you();
        let (added, removed) = j.lines();
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
            .on_mouse_down(
                MouseButton::Right,
                self.on_menu(super::menu::Ctx::Job(id), cx),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx)))
    }
}

/// How wide the jobs page's column grows: past this, cards only stretch empty space.
const MAX_W: f32 = 980.;

/// Where a job sits in the Inbox: ready for review first, then what stopped,
/// then anything else that wants a look.
fn inbox_group(j: &Job) -> (u8, &'static str) {
    match j.status {
        Status::Review => (0, "Ready for review"),
        Status::Failed | Status::Stopped | Status::Interrupted => (1, "Stopped"),
        _ => (2, "To look at"),
    }
}

/// Splits jobs into to review, working, done, and stopped, keeping order. A
/// job that stopped goes with the stopped ones even before you've seen it.
fn sort_jobs(jobs: &[Job]) -> (Vec<Job>, Vec<Job>, Vec<Job>, Vec<Job>) {
    let mut out = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for j in jobs {
        let stopped = matches!(
            j.status,
            Status::Failed | Status::Interrupted | Status::Stopped
        );
        let bin = if j.needs_you() && !stopped {
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
/// A board column's head: a square in the column's colour, its name in small
/// mono capitals, and how many cards it holds.
fn col_head(label: &str, col: u32, n: usize) -> Div {
    div()
        .px(px(2.))
        .flex()
        .items_center()
        .gap(px(7.))
        .font_family(ui::MONO)
        .text_size(px(10.5))
        .child(div().size(px(6.)).flex_none().bg(c(col)))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(col))
                .child(label.to_uppercase()),
        )
        .child(div().text_color(c(ui::faint())).child(n.to_string()))
}

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
            job(7, Status::Failed, false),
            job(5, Status::Done, true),
            job(6, Status::Shipping, true),
        ];
        let (needs, going, done, trouble) = sort_jobs(&jobs);
        let ids = |v: Vec<Job>| v.iter().map(|j| j.id).collect::<Vec<_>>();
        assert_eq!(ids(needs), [2, 3]);
        assert_eq!(ids(going), [1, 6]);
        assert_eq!(ids(done), [5]);
        assert_eq!(ids(trouble), [4, 7]);
    }
}
