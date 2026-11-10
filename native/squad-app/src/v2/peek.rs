//! A job, two ways. The peek pane opens beside the thread when you click a
//! job's card: what it is, where it stands, its changes and the buttons to
//! decide, without leaving the conversation. Expand opens the job workspace:
//! the bot's conversation always on the left, and the job's Changes,
//! Checks, Browser, Terminal and Files as tabs on the right.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::{Framing, Mood};

use super::store::Job;
use super::ui::{self, c};
use super::{home, Place, Shell};

/// The peek pane's width.
pub(super) const PEEK_W: f32 = 352.;
/// The workspace's conversation column.
pub(super) const TALK_W: f32 = 460.;

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

/// A small mono label, like the peek's crumb and the tabs.
fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(ui::MONO)
        .text_size(px(11.5))
        .text_color(c(ui::faint()))
        .child(text.into())
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

    /// The peek pane: the job's crumb with Expand and close, the bot and the
    /// job's title, its state and branch, then its review and its plan.
    pub(super) fn peek_pane(&mut self, job: &Job, thread: &str, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let (word, tone) = home::status_of(job);
        let face = self.bot(job.bot, Mood::Idle, 40., Framing::Bust);
        let review = self.review_body(job, cx);
        let details = self.job_details(job, cx);
        let by = if job.engine.is_empty() {
            job.bot.name().to_string()
        } else {
            format!("{} \u{b7} {}", job.bot.name(), job.engine)
        };
        let head = div()
            .h(px(50.))
            .flex_none()
            .px(px(14.))
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
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child(format!("JOB #{id} / #{}", thread.to_uppercase())),
            )
            .child(
                ui::ghost_i("peek-expand", IconName::Maximize, "Expand")
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
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(17.))
                        .line_height(px(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(super::channel::card_title(job)),
                )
                .child(mono(by)),
        );
        let state = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(10.))
            .child(home::pill(&word, tone))
            .when(!job.branch.is_empty(), |d| {
                d.child(mono(if job.base.is_empty() {
                    job.branch.clone()
                } else {
                    format!("{} \u{2192} {}", job.branch, job.base)
                }))
            });
        div()
            .w(px(PEEK_W))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(c(ui::line()))
            .bg(c(ui::ground()))
            .child(head)
            .child(
                div()
                    .id("peek-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(16.))
                    .pt(px(14.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(title)
                    .child(state)
                    .children(review.map(|r| div().flex_none().child(r)))
                    .child(div().flex_none().child(details)),
            )
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
            .h(px(42.))
            .flex_none()
            .px(px(16.))
            .flex()
            .items_end()
            .gap(px(4.))
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
                    .h_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .cursor_pointer()
                    .border_b_2()
                    .map(|d| {
                        if on {
                            d.border_color(c(ui::pick_fill()))
                                .text_color(c(ui::ink()))
                                .font_weight(FontWeight::SEMIBOLD)
                        } else {
                            d.border_color(transparent_black())
                                .text_color(c(ui::muted()))
                                .hover(|s| s.text_color(c(ui::ink())))
                        }
                    })
                    .child(t.name())
                    .when_some(count, |d, n| {
                        d.child(
                            div()
                                .h(px(17.))
                                .px(px(6.))
                                .flex()
                                .items_center()
                                .rounded(px(3.))
                                .bg(c(ui::hover()))
                                .font_weight(FontWeight::NORMAL)
                                .text_size(px(10.5))
                                .text_color(c(ui::muted()))
                                .child(n.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ws_tab = t;
                        cx.notify();
                    })),
            );
        }
        let body: AnyElement = match tab {
            Tab::Changes => match self.review_body(&job, cx) {
                Some(review) => div()
                    .id("ws-changes")
                    .size_full()
                    .overflow_y_scroll()
                    .p(px(20.))
                    .child(review)
                    .into_any_element(),
                None => empty(
                    "No changes yet. They show here as the bot edits files.",
                    None,
                )
                .into_any_element(),
            },
            Tab::Checks => {
                // Checker's run when there is one, then where the job stands
                // and its plan.
                let checks = (!job.checks.is_empty() || job.checking).then(|| {
                    ui::card()
                        .overflow_hidden()
                        .child(self.checks_block(&job, cx))
                });
                let none = (job.checks.is_empty() && !job.checking).then(|| {
                    div().text_size(px(13.)).text_color(c(ui::muted())).child(
                        if job.mode.writes() {
                            "Checker runs the project's checks when the bot finishes."
                        } else {
                            "A job that only reads has no checks to run."
                        },
                    )
                });
                div()
                    .id("ws-checks")
                    .size_full()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .py(px(16.))
                    .child(
                        div()
                            .max_w(px(720.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .children(checks)
                            .children(none)
                            .child(self.job_details(&job, cx)),
                    )
                    .into_any_element()
            }
            Tab::Browser => {
                if self.web.job == id && self.web.live() {
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
            Tab::Terminal => match self.term_drawer(&job, cx) {
                Some(term) => div().size_full().child(term).into_any_element(),
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
            Tab::Files => {
                let marks = job
                    .changes
                    .iter()
                    .map(|ch| (ch.path.clone(), ch.status.chars().take(1).collect()))
                    .collect();
                let title = if job.branch.is_empty() {
                    job.folder.name.clone()
                } else {
                    job.branch.clone()
                };
                let dir = job.dir().to_string();
                self.files_side(&dir, title, marks, cx).into_any_element()
            }
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
                    .bg(c(ui::ground()))
                    .child(tabs)
                    .child(div().flex_1().min_h_0().child(body)),
            )
            .into_any_element()
    }
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
