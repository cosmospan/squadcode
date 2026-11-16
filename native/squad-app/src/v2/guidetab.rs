//! The review guide on screen: the workspace's Guide tab, a tour of a
//! finished job's changes in reading order with the places to look closely
//! marked, and its short card in the peek pane. The guide itself is written
//! by Guard or Explainer once Checker's checks are back (`brain::guide`).

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::brain::guide::Stop;
use super::data::Member;
use super::peek::Tab;
use super::store::{Job, Status};
use super::ui::{self, c, ca};
use super::{Place, Shell};

/// A list label (`.lbl`): mono capitals, faint.
fn label(text: &str) -> Div {
    div()
        .font_family(ui::mono())
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(ui::faint()))
        .child(text.to_uppercase())
}

/// Where a stop points: "src/cart.ts · 40-62".
fn place_of(stop: &Stop) -> String {
    match (stop.file.is_empty(), stop.lines.is_empty()) {
        (true, _) => "The change as a whole".into(),
        (false, true) => stop.file.clone(),
        (false, false) => format!("{} \u{b7} {}", stop.file, stop.lines),
    }
}

impl Shell {
    /// Opens a changed file's diff in the workspace's Changes tab.
    fn open_guide_file(&mut self, id: u64, path: String, cx: &mut Context<Self>) {
        self.diff_text = self
            .crew
            .store
            .job(id)
            .map(|j| super::work::diff(j, &path))
            .unwrap_or_default();
        self.diff_open = Some((id, path));
        self.ws_tab = Tab::Changes;
        cx.notify();
    }

    /// The workspace's Guide tab.
    pub(super) fn guide_tab(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let writing = self.crew.guiding(id);
        let guide = job.guide.clone();
        let by = writing
            .or_else(|| guide.as_ref().and_then(|g| g.by))
            .unwrap_or(Member::Explainer);
        let can_write = job.status == Status::Review && writing.is_none();
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(self.avatar(by, 22.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(if writing.is_some() {
                        format!("{} is reading the change", by.name())
                    } else if guide.is_some() {
                        format!("{}'s guide to this change", by.name())
                    } else {
                        "No guide yet".to_string()
                    }),
            )
            .when(can_write, |d| {
                d.child(
                    ui::dbtn_i(
                        "guide-again",
                        IconName::RotateCw,
                        if guide.is_some() {
                            "Write it again"
                        } else {
                            "Write it now"
                        },
                        ui::Btn::Plain,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.rewrite_guide(id, cx))),
                )
            });
        let mut out = div().flex().flex_col().gap(px(14.)).child(head);
        if writing.is_some() {
            return out.child(ui::sweep(ui::BLUE, 0.).h(px(3.))).child(
                div()
                    .text_size(px(13.))
                    .text_color(c(ui::body()))
                    .child(
                        "It reads what changed and what Checker found, then walks you through it in the order to read it.",
                    ),
            );
        }
        let Some(guide) = guide else {
            return out.child(
                div()
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_color(c(ui::body()))
                    .child(if job.checking {
                        "It's written once Checker's checks are back."
                    } else {
                        "Guard or Explainer writes one when a job is ready to review: a tour of the change in the order to read it, with the places to look closely marked."
                    }),
            );
        };
        if !guide.intro.is_empty() {
            out = out.child(
                div()
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .text_color(c(ui::ink()))
                    .child(guide.intro.clone()),
            );
        }
        let look = guide.closely();
        out = out.child(
            div()
                .flex()
                .gap(px(6.))
                .font_family(ui::mono())
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child(format!(
                    "{} stop{}",
                    guide.stops.len(),
                    if guide.stops.len() == 1 { "" } else { "s" }
                ))
                .when(look > 0, |d| {
                    d.child("\u{b7}").child(
                        div()
                            .text_color(c(ui::t_amber()))
                            .child(format!("{look} to look closely at")),
                    )
                }),
        );
        let count = guide.stops.len();
        let mut list = div().flex().flex_col();
        for (n, stop) in guide.stops.iter().enumerate() {
            let closely = stop.closely();
            let tone = if closely { ui::t_amber() } else { ui::faint() };
            let last = n + 1 == count;
            // The stop's number, amber where to look closely.
            let rail = div()
                .w(px(24.))
                .flex_none()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .size(px(24.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(c(tone))
                        .when(closely, |d| d.bg(ca(ui::t_amber(), 0.14)))
                        .font_family(ui::mono())
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(if closely { ui::t_amber() } else { ui::body() }))
                        .child((n + 1).to_string()),
                );
            let file = stop.file.clone();
            let place = div()
                .id(("guide-file", n))
                .flex()
                .items_center()
                .gap(px(5.))
                .min_w_0()
                .font_family(ui::mono())
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child(div().min_w_0().truncate().child(place_of(stop)))
                .when(!file.is_empty(), |d| {
                    d.cursor_pointer()
                        .hover(|s| s.text_color(c(ui::ink())))
                        .child("\u{2197}")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_guide_file(id, file.clone(), cx)
                        }))
                });
            let body = div()
                .flex_1()
                .min_w_0()
                .pb(px(if last { 0. } else { 18. }))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(ui::ink()))
                                .child(stop.title.clone()),
                        )
                        .when(closely, |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .px(px(6.))
                                    .rounded(px(3.))
                                    .bg(ca(ui::t_amber(), 0.14))
                                    .font_family(ui::mono())
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::t_amber()))
                                    .child("LOOK CLOSELY"),
                            )
                        }),
                )
                .child(place)
                .when(!stop.note.is_empty(), |d| {
                    d.child(
                        div()
                            .mt(px(2.))
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .text_color(c(ui::body()))
                            .child(stop.note.clone()),
                    )
                })
                .when(closely, |d| {
                    d.child(
                        div()
                            .mt(px(4.))
                            .px(px(10.))
                            .py(px(8.))
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .rounded(px(5.))
                            .border_l_2()
                            .border_color(c(ui::t_amber()))
                            .bg(ca(ui::t_amber(), 0.08))
                            .child(
                                Icon::new(IconName::Eye)
                                    .size(px(14.))
                                    .mt(px(2.))
                                    .text_color(c(ui::t_amber())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(13.))
                                    .line_height(px(19.))
                                    .text_color(c(ui::ink()))
                                    .child(stop.look.clone()),
                            ),
                    )
                });
            list = list.child(
                div()
                    .flex()
                    .items_stretch()
                    .gap(px(12.))
                    .child(rail)
                    .child(body),
            );
        }
        out.child(list)
    }

    /// The guide's card in the peek pane, for a job waiting for review: who
    /// wrote it and how many places to look closely at, opening the Guide tab.
    pub(super) fn peek_guide(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        let id = job.id;
        let writing = self.crew.guiding(id);
        let guide = job.guide.as_ref();
        let (by, line, flagged) = match (writing, guide) {
            (Some(by), _) => (by, format!("{} is writing it", by.name()), false),
            (None, Some(g)) => (g.by.unwrap_or(Member::Explainer), g.line(), g.closely() > 0),
            (None, None) => return None,
        };
        let card = div()
            .id("peek-guide")
            .px(px(12.))
            .py(px(10.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(5.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(if flagged { ui::t_amber() } else { ui::line() }))
            .cursor_pointer()
            .hover(|s| s.border_color(c(ui::line_hi())))
            .child(self.avatar(by, 20.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("Read the guide"),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(c(if flagged { ui::t_amber() } else { ui::faint() }))
                            .child(line),
                    ),
            )
            .child(
                Icon::new(IconName::ArrowRight)
                    .size(px(14.))
                    .text_color(c(ui::faint())),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.ws_tab = Tab::Guide;
                this.reply_open = None;
                this.go(Place::Live(id), cx);
            }));
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(label("Review guide"))
                .child(card),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{place_of, Stop};

    #[test]
    fn a_stop_says_where_it_points() {
        let mut s = Stop::default();
        assert_eq!(place_of(&s), "The change as a whole");
        s.file = "src/cart.ts".into();
        assert_eq!(place_of(&s), "src/cart.ts");
        s.lines = "40-62".into();
        assert_eq!(place_of(&s), "src/cart.ts \u{b7} 40-62");
    }
}
