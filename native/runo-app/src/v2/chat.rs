//! Chat: ask any bot in plain words. It reads the code in the current folder
//! and answers; nothing changes. Each bot keeps its own conversation.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use runo_holo::{Framing, Mood};

use super::channel::{self, Msg};
use super::data::Member;
use super::live::{note_line, plain, typing};
use super::ui::{self, c, ca};
use super::Pane;
use super::{Place, Shell};

impl Shell {
    pub(super) fn chat(&mut self, who: Member, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            // Bots are picked in the sidebar's crew list.
            .child(self.chat_thread(who, cx))
            .into_any_element()
    }

    fn chat_thread(&mut self, who: Member, cx: &mut Context<Self>) -> impl IntoElement {
        let thinking = self.thinking(who);
        let engine = super::engine::label(cx.global::<crate::settings::AppSettings>(), who);
        let log = self.crew.store.chat(who);
        let dir = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        let jobs: Vec<super::store::Job> =
            self.crew.store.dm_jobs(who).into_iter().cloned().collect();
        let header = self.dm_head(who, engine, !log.is_empty(), cx);

        let mut body = div().flex().flex_col().px(px(20.)).py(px(16.));
        body = body.child(self.dm_intro(who, log.is_empty() && jobs.is_empty(), cx));
        // The bot's jobs sit among the lines, by when they started, with a
        // line between days as in a thread.
        let mut jobs = jobs.into_iter().peekable();
        let mut day = String::new();
        let mut day_line = |at: u64, body: Div| -> Div {
            let label = channel::day_label(at);
            if at > 0 && !label.is_empty() && label != day {
                day = label.clone();
                return body.child(channel::divider(&label, None));
            }
            body
        };
        // The newest line slides in when it arrives.
        let last = log.len().saturating_sub(1);
        let fresh = if self.motion {
            runo_holo::anim::spring(self.chat_at.elapsed().as_secs_f64(), 0.3) as f32
        } else {
            1.0
        };
        let mut asked = String::new();
        for (i, m) in log.iter().enumerate() {
            while let Some(j) = jobs.next_if(|j| m.at > 0 && j.started <= m.at) {
                body = day_line(j.started, body);
                body = body.child(self.job_entry(&j, cx));
            }
            body = day_line(m.at, body);
            let k = if i == last { fresh } else { 1.0 };
            let key = format!("dm-{}-{}-{i}", who.name(), m.at);
            let row: Div = if m.mine {
                asked = m.text.clone();
                self.message(
                    key,
                    m.text.clone(),
                    Msg::Mine(m.text.clone()),
                    channel::mine(m.text.clone(), m.at),
                    cx,
                )
            } else if m.note {
                div().py(px(4.)).child(note_line(&m.text))
            } else {
                let words = self.rich(format!("dm-{i}"), &plain(&m.text), &dir, cx);
                let said = div()
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .text_color(c(ui::body()))
                    .child(words);
                let line = self.bot_line(who, m.at, said, m.via.clone());
                self.message(
                    key,
                    plain(&m.text),
                    Msg::Answer(who, asked.clone()),
                    line,
                    cx,
                )
            };
            body = body.child(ui::rise(row, k));
        }
        for j in jobs {
            body = day_line(j.started, body);
            body = body.child(self.job_entry(&j, cx));
        }
        if thinking {
            let secs = self
                .crew
                .asking
                .as_ref()
                .map(|a| a.started.elapsed().as_secs())
                .unwrap_or(0);
            let dots = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(typing(self.now()))
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .child(format!("reading the code \u{b7} {secs}s")),
                );
            body = body.child(self.bot_line(who, 0, dots, None));
        }
        if !log.is_empty() && !thinking && !matches!(who.like(), Member::Explainer | Member::Fin) {
            // Turn the conversation into work: a made bot takes the job itself.
            let mode = who.mode();
            let last_q = log.iter().rev().find(|m| m.mine).map(|m| m.text.clone());
            if let Some(q) = last_q {
                let doer = if who.is_made() {
                    who
                } else {
                    Member::for_mode(mode)
                };
                body = body.child(div().pl(px(46.)).pt(px(4.)).flex().gap(px(8.)).child(
                    ui::ghost("hand-off", format!("Make it a job for {}", doer.name())).on_click(
                        cx.listener(move |this, _, _, cx| {
                            let route = super::brain::route::Route {
                                mode,
                                bot: doer,
                                sure: 1.0,
                                why: String::new(),
                                by: "dm".into(),
                            };
                            this.hand_to(route, q.clone(), cx);
                            // The job's card lands in this DM.
                            if doer == who {
                                this.go(Place::Chat(who), cx);
                            }
                        }),
                    ),
                ));
            }
        }
        let len = log.len() + usize::from(thinking);
        if len != self.chat_len {
            if len > self.chat_len {
                self.chat_at = std::time::Instant::now();
            }
            self.chat_len = len;
            self.chat_scroll.scroll_to_bottom();
        }
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("chat-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.chat_scroll)
                    .child(div().max_w(px(980.)).w_full().mx_auto().child(body)),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(20.))
                    .pb(px(super::DOCK_ROOM))
                    .child(
                        div()
                            .max_w(px(980.))
                            .w_full()
                            .mx_auto()
                            .child(self.dm_composer(who, cx)),
                    ),
            )
    }

    /// A bot's message in its DM, laid out as in a thread: face, name and
    /// time, the words, and which engine answered.
    fn bot_line(&mut self, who: Member, at: u64, said: Div, via: Option<String>) -> Div {
        let face = self.avatar(who, 34.);
        let accent = who.accent();
        let mut by = if at > 0 {
            channel::byline(who.name(), at)
        } else {
            channel::byline(who.name(), super::store::now())
        };
        by = by.child(
            div()
                .px(px(6.))
                .rounded(px(5.))
                .bg(ca(accent, 0.12))
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(super::theme::mix(accent, ui::ink(), 0.35)))
                .child(who.role()),
        );
        div()
            .flex()
            .gap(px(12.))
            .py(px(8.))
            .child(
                div()
                    .size(px(34.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(face),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(by)
                    .child(said)
                    .when_some(via, |d, via| {
                        d.child(
                            div()
                                .pt(px(2.))
                                .text_size(px(11.))
                                .text_color(c(ui::faint()))
                                .child(via),
                        )
                    }),
            )
    }

    /// The DM's header line, as a thread's: who, what it's doing, which
    /// engine it runs on, and Clear.
    fn dm_head(
        &mut self,
        who: Member,
        engine: String,
        said: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let face = self.avatar(who, 22.);
        let (word, tint) = self.status_of(who);
        let busy = self.thinking(who) || self.crew.store.busy(who).is_some();
        let dot = if busy { ui::BLUE } else { ui::GREEN };
        ui::chrome(div())
            .h(px(46.))
            .flex_none()
            .w_full()
            .px(px(20.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(c(ui::line2()))
            .child(
                div().relative().child(face).child(
                    div()
                        .absolute()
                        .right(px(-2.))
                        .bottom(px(-2.))
                        .size(px(9.))
                        .rounded_full()
                        .border_2()
                        .border_color(c(ui::bg()))
                        .bg(c(dot)),
                ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(15.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(ui::ink()))
                    .child(who.name()),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(c(tint))
                    .child(word),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("chat-engine")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .h(px(30.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_size(px(12.))
                    .text_color(c(ui::muted()))
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(ui::line_hi())).text_color(c(ui::ink())))
                    .child(Icon::new(IconName::Cpu).size(px(13.)))
                    .child(engine)
                    .on_click(
                        cx.listener(|this, _, _, cx| this.go(Place::Settings(Pane::Engines), cx)),
                    ),
            )
            .when(said, |d| {
                d.child(
                    ui::ghost_i("chat-clear", IconName::Eraser, "Clear")
                        .h(px(30.))
                        .flex_none()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.crew.store.clear_dm(who);
                            this.crew.store.save();
                            cx.notify();
                        })),
                )
            })
    }

    /// The top of a DM, as the top of a thread: the bot, what it's for, and
    /// while it's empty, a few things to ask it.
    fn dm_intro(&mut self, who: Member, empty: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let mood = if self.thinking(who) {
            Mood::Thinking
        } else {
            self.mood(who)
        };
        let hero = self.bot(who, mood, 72., Framing::Full);
        let project = self
            .crew
            .store
            .folder()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "this project".into());
        div()
            .w_full()
            .pt(px(8.))
            .pb(px(16.))
            .mb(px(4.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_b_1()
            .border_color(c(ui::line2()))
            .child(
                div().flex().items_center().gap(px(14.)).child(hero).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(24.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(ui::ink()))
                                .child(who.name()),
                        )
                        .child(div().text_size(px(13.5)).text_color(c(ui::muted())).child(
                            format!(
                                "Your DM with {} in {project}. {} Its jobs here show up too.",
                                who.name(),
                                who.pitch()
                            ),
                        )),
                ),
            )
            .when(empty, |d| d.child(self.chat_empty(who, cx)))
    }

    /// The box you type in, as a thread's: the words, who answers, and Send.
    fn dm_composer(&mut self, who: Member, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.chat_input.read(cx).value().to_string();
        let ready = !value.trim().is_empty();
        let face = self.avatar(who, 20.);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(14.))
            .pt(px(12.))
            .pb(px(10.))
            .rounded(px(14.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(if ready {
                super::theme::mix(ui::raised(), ui::VIOLET, 0.55)
            } else {
                ui::line_hi()
            }))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.05),
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .min_h(px(32.))
                    .text_size(px(14.5))
                    .child(Input::new(&self.chat_input).appearance(false)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child(format!(
                                "Message {}. Nothing changes until you make it a job.",
                                who.name()
                            )),
                    )
                    .child(
                        div()
                            .h(px(28.))
                            .pl(px(5.))
                            .pr(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded_full()
                            .bg(c(ui::well()))
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child(face)
                            .child(format!("{} answers", who.name())),
                    )
                    .child(
                        div()
                            .id("chat-send")
                            .size(px(32.))
                            .rounded(px(9.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .map(|d| {
                                if ready {
                                    d.bg(c(ui::VIOLET))
                                        .cursor_pointer()
                                        .hover(|s| s.opacity(0.9))
                                } else {
                                    d.bg(c(ui::press()))
                                }
                            })
                            .child(
                                Icon::new(IconName::ArrowUp)
                                    .size(px(16.))
                                    .text_color(c(if ready { 0xFFFFFF } else { ui::faint() })),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let text = this.chat_input.read(cx).value().trim().to_string();
                                if !text.is_empty() {
                                    this.chat_input
                                        .update(cx, |i, cx| i.set_value("", window, cx));
                                    this.ask(who, text, cx);
                                }
                            })),
                    ),
            )
    }

    /// While a DM is empty: a few things to ask the bot.
    fn chat_empty(&mut self, who: Member, cx: &mut Context<Self>) -> impl IntoElement {
        let mut asks = div().mt(px(6.)).flex().flex_wrap().gap(px(8.));
        for (i, q) in who.starters().into_iter().enumerate() {
            asks = asks.child(
                div()
                    .id(("chat-starter", i))
                    .px(px(12.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_size(px(13.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .child(q)
                    .on_click(cx.listener(move |this, _, _, cx| this.ask(who, q.to_string(), cx))),
            );
        }
        asks
    }
}
