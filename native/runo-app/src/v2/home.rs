//! Home's working half: a strip of numbers, and the job list beside a job.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use runo_holo::{Framing, Mood};

use super::store::{self, Job, Status};
use super::ui::{self, c, ca};
use super::{motion, Place, Shell};

impl Shell {
    /// Running, needs you, done today and what today cost.
    pub(super) fn kpis(&mut self) -> impl IntoElement {
        let day = 86_400;
        let today: Vec<&Job> = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| store::now().saturating_sub(j.ended.unwrap_or(j.started)) < day)
            .collect();
        let running: Vec<&Job> = self.crew.store.running().collect();
        let needs: Vec<&Job> = self
            .crew
            .store
            .listed()
            .into_iter()
            .filter(|j| j.needs_you())
            .collect();
        let done = today
            .iter()
            .filter(|j| matches!(j.status, Status::Done | Status::Shipped))
            .count();
        let ended = today.iter().filter(|j| j.ended.is_some()).count();
        let spend: f64 = today.iter().filter_map(|j| cost(&j.summary)).sum::<f64>() + 0.0;
        let mut names: Vec<&str> = Vec::new();
        for j in &running {
            if !names.contains(&j.bot.name()) {
                names.push(j.bot.name());
            }
        }
        let who = match names.len() {
            0 => "nobody busy".to_string(),
            1..=2 => names.join(", "),
            n => format!("{}, {} and {} more", names[0], names[1], n - 2),
        };
        let oldest = needs
            .iter()
            .map(|j| j.ended.unwrap_or(j.started))
            .min()
            .map(|t| match store::ago(t).as_str() {
                "just now" => "just now".to_string(),
                a => format!("oldest {a}"),
            })
            .unwrap_or_else(|| "all clear".into());
        div()
            .w_full()
            .flex()
            .gap(px(12.))
            .child(ui::stat(
                (IconName::LoaderCircle, ui::t_blue()),
                "Running",
                running.len().to_string(),
                who,
            ))
            .child(ui::stat(
                (IconName::Inbox, ui::t_amber()),
                "Needs you",
                needs.len().to_string(),
                oldest,
            ))
            .child(ui::stat(
                (IconName::CircleCheck, ui::t_green()),
                "Done today",
                done.to_string(),
                format!("of {ended} finished"),
            ))
            .child(ui::stat(
                (IconName::Wallet, ui::t_violet()),
                "Spend today",
                format!("${spend:.2}"),
                "across engines".into(),
            ))
    }
}

/// A job's status as a word and pill colours.
pub(super) fn status_of(j: &Job) -> (String, (u32, u32, u32)) {
    match j.status {
        Status::Running => ("Running".to_string(), ui::run_tone()),
        Status::Done if j.needs_you() => ("Ready to review".to_string(), ui::ask_tone()),
        Status::Done => ("Done".to_string(), ui::ok_tone()),
        Status::Failed => ("Failed".to_string(), ui::bad_tone()),
        Status::Interrupted => ("Interrupted".to_string(), ui::ask_tone()),
        Status::Review => ("Ready for review".to_string(), ui::ask_tone()),
        Status::Shipped => ("Shipped".to_string(), ui::ok_tone()),
        s => (s.label().to_string(), ui::grey_tone()),
    }
}

/// A status pill: a dot and a word on a tinted ground.
pub(super) fn pill(word: &str, tone: (u32, u32, u32)) -> Div {
    ui::badge(tone, word.to_string())
}

/// The dollars in a summary like "12s · 3 turns · $0.04".
pub(super) fn cost(summary: &str) -> Option<f64> {
    summary
        .split('·')
        .map(str::trim)
        .find_map(|p| p.strip_prefix('$'))
        .and_then(|v| v.parse().ok())
}

/// How wide the job list beside a job is.
const LIST_W: f32 = 360.;

impl Shell {
    /// A page with the job list beside it: the split view every job page uses.
    /// `pick` is the job open on the right, if any.
    pub(super) fn split(
        &mut self,
        pick: Option<u64>,
        detail: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Crew Chat: the conversation in front, and the Jobs panel on its right
        // only when you open it from the top bar.
        let list = self.jobs_open.then(|| {
            // Slides in from the right edge when opened.
            motion::panel(
                div().h_full().flex_none().child(self.job_list(pick, cx)),
                motion::key("jobs-panel"),
                18.,
                0.,
            )
        });
        div()
            .size_full()
            .flex()
            .child(div().flex_1().min_w_0().h_full().child(detail))
            .children(list)
            .into_any_element()
    }

    /// The job list: filters, then what needs you, what's running and what
    /// finished, one row per job with the bot's face.
    fn job_list(&mut self, pick: Option<u64>, cx: &mut Context<Self>) -> impl IntoElement {
        // The thread in view: the picked job's, else the one you're in.
        let store = &self.crew.store;
        let (folder, thread) = match pick.and_then(|id| store.job(id)) {
            Some(j) => (
                j.folder.path.clone(),
                store.thread_for(&j.folder.path, j.thread),
            ),
            None => (
                store.folder().map(|f| f.path.clone()).unwrap_or_default(),
                store.thread_here(),
            ),
        };
        let thread_name = store.thread_label(thread);
        let scoped = !self.list_all;
        let all: Vec<Job> = store
            .listed()
            .into_iter()
            .filter(|j| {
                !scoped
                    || (j.folder.path == folder && store.thread_for(&folder, j.thread) == thread)
            })
            .cloned()
            .collect();
        let running = |j: &Job| matches!(j.status, Status::Running | Status::Shipping);
        let needs: Vec<Job> = all.iter().filter(|j| j.needs_you()).cloned().collect();
        let going: Vec<Job> = all
            .iter()
            .filter(|j| !j.needs_you() && running(j))
            .cloned()
            .collect();
        let done: Vec<Job> = all
            .iter()
            .filter(|j| !j.needs_you() && !running(j))
            .cloned()
            .collect();
        let filter = self.list_filter;

        let mut filters = div()
            .flex()
            .items_center()
            .p(px(3.))
            .gap(px(2.))
            .rounded(px(9.))
            .bg(c(ui::well()))
            .border_1()
            .border_color(c(ui::line2()));
        for (i, label, n) in [
            (0u8, "All", all.len()),
            (1, "Needs you", needs.len()),
            (2, "Running", going.len()),
            (3, "Done", done.len()),
        ] {
            let on = filter == i;
            filters = filters.child(
                div()
                    .id(("list-filter", i as usize))
                    .flex_1()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(5.))
                    .rounded(px(6.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            // The picked filter's chip fades in as it moves.
                            let k = motion::shown(
                                motion::key(("list-filter-on", i)),
                                motion::QUICK,
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
                                .text_color(c(ui::faint()))
                                .child(n.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.list_filter = i;
                        cx.notify();
                    })),
            );
        }

        let head = div()
            .flex_none()
            .px(px(16.))
            .pt(px(16.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Jobs"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child(all.len().to_string()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("list-scope")
                            .flex()
                            .items_center()
                            .p(px(2.))
                            .rounded(px(8.))
                            .bg(c(ui::press()))
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .children(
                                [
                                    (false, format!("#{thread_name}")),
                                    (true, "All".to_string()),
                                ]
                                .map(|(all, label)| {
                                    let on = self.list_all == all;
                                    div()
                                        .max_w(px(120.))
                                        .truncate()
                                        .px(px(8.))
                                        .h(px(22.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(6.))
                                        .map(|d| {
                                            if on {
                                                d.bg(c(ui::raised())).text_color(c(ui::ink()))
                                            } else {
                                                d.text_color(c(ui::muted()))
                                            }
                                        })
                                        .child(label)
                                }),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.list_all = !this.list_all;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::icon_btn("list-all", IconName::ListFilter)
                            .size(px(28.))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("All jobs")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.go(Place::History, cx))),
                    ),
            )
            .child(filters);

        let mut body = div()
            .id("job-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(2.));
        let groups = [
            (1u8, "Needs you", ui::AMBER, needs),
            (2, "Running", ui::BLUE, going),
            (3, "Done", ui::GREEN, done),
        ];
        let mut any = false;
        let mut shown = 0;
        for (key, label, color, rows) in groups {
            if (filter != 0 && filter != key) || rows.is_empty() {
                continue;
            }
            any = true;
            body = body.child(
                ui::heading(label, Some(color), Some(rows.len()))
                    .px(px(10.))
                    .pt(px(14.))
                    .pb(px(6.)),
            );
            for j in &rows {
                let row = self.list_row(j, pick == Some(j.id), cx);
                body = body.child(motion::enter(row, motion::key(("list-row", j.id)), shown));
                shown += 1;
            }
        }
        if !any {
            body = body.child(
                div()
                    .px(px(10.))
                    .py(px(20.))
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child("Nothing here."),
            );
        }

        div()
            .w(px(LIST_W))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(c(ui::ground()))
            .border_l_1()
            .border_color(c(ui::line()))
            .child(head)
            .child(body)
    }

    /// One job in the list: the bot's face, the title, its state, and a thin
    /// progress line while it runs.
    fn list_row(&mut self, j: &Job, on: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = j.id;
        let is_running = j.status == Status::Running;
        let face = if is_running {
            self.bot(j.bot, Mood::Working, 34., Framing::Bust)
        } else {
            self.avatar(j.bot, 34.)
        };
        let (word, tone) = status_of(j);
        let frac = j.progress().map(|(d, n)| {
            let f = d as f32 / n.max(1) as f32;
            if is_running {
                self.glide_bar(id, f)
            } else {
                f
            }
        });
        let when = match store::ago(j.ended.unwrap_or(j.started)).as_str() {
            "just now" => "now".to_string(),
            a => a.to_string(),
        };
        let sub = if is_running {
            j.doing()
        } else {
            format!("{} · #{}", j.bot.name(), id)
        };
        let accent = j.bot.accent();
        div()
            .id(("list-row", id as usize))
            .relative()
            .flex()
            .gap(px(11.))
            .px(px(10.))
            .py(px(10.))
            .rounded(px(10.))
            .cursor_pointer()
            .map(|d| {
                if on {
                    // Picked: the tint and the accent mark grow in.
                    let k = motion::shown(motion::key(("list-row-on", id)), motion::BASE, 0.);
                    d.bg(ca(ui::raised(), k)).child(
                        div()
                            .absolute()
                            .left(px(-8.))
                            .top(px(12. + 8. * (1. - k)))
                            .bottom(px(12. + 8. * (1. - k)))
                            .w(px(3.))
                            .rounded_r(px(3.))
                            .bg(ca(accent, k)),
                    )
                } else {
                    motion::hover_fill(d, None, ui::hover())
                }
            })
            .child(
                div()
                    .size(px(38.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .bg(ca(accent, 0.10))
                    .border_1()
                    .border_color(ca(accent, 0.22))
                    .child(face),
            )
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
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::ink()))
                                    .child(j.label().to_string()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(c(ui::faint()))
                                    .child(when),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(c(ui::muted()))
                            .child(motion::dot(tone.0, 6., is_running || j.needs_you()))
                            .child(div().flex_none().text_color(c(tone.0)).child(word))
                            .child(div().flex_none().text_color(c(ui::faint())).child("·"))
                            .child(div().min_w_0().truncate().child(sub)),
                    )
                    .when_some(frac.filter(|_| is_running), |d, f| {
                        d.child(div().mt(px(4.)).child(ui::bar(f, ui::BLUE).h(px(3.))))
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Live(id), cx)))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_cost() {
        assert_eq!(super::cost("12s · 3 turns · $0.04"), Some(0.04));
        assert_eq!(super::cost("12s"), None);
    }
}
