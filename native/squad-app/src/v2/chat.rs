//! Chat: ask any bot in plain words. It reads the code in the current folder
//! and answers; nothing changes. Each bot keeps its own conversation.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::Mood;

use super::channel::{self, Msg};
use super::data::Member;
use super::live::{note_line, plain, typing};
use super::store::Said;
use super::ui::{self, c, ca};
use super::Pane;
use super::{Place, Shell};

/// Where an answer with a job offer lives: a line in a bot's DM (who, when,
/// the words), or a question's answer in a thread.
#[derive(Clone)]
pub(super) enum OfferAt {
    Dm(Member, u64, String),
    Talk(u64),
}

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

        let mut body = div().flex().flex_col().px(px(24.)).py(px(16.));
        body = body.child(self.dm_intro(who, log.is_empty() && jobs.is_empty(), cx));
        // The bot's jobs sit among the lines, by when they started, with a
        // line between days as in a thread.
        let mut jobs = jobs.into_iter().peekable();
        let mut day = String::new();
        self.streak = None;
        // A line between days, which also starts a new run of messages.
        let day_line = |this: &mut Self, day: &mut String, at: u64, body: Div| -> Div {
            let label = channel::day_label(at);
            if at > 0 && !label.is_empty() && label != *day {
                *day = label.clone();
                this.streak = None;
                return body.child(channel::divider(&label, None));
            }
            body
        };
        // The newest line slides in when it arrives.
        let last = log.len().saturating_sub(1);
        let fresh = if self.motion {
            squad_holo::anim::spring(self.chat_at.elapsed().as_secs_f64(), 0.3) as f32
        } else {
            1.0
        };
        let mut asked = String::new();
        for (i, m) in log.iter().enumerate() {
            while let Some(j) = jobs.next_if(|j| m.at > 0 && j.started <= m.at) {
                body = day_line(self, &mut day, j.started, body);
                body = body.child(self.job_entry(&j, cx));
            }
            body = day_line(self, &mut day, m.at, body);
            let k = if i == last { fresh } else { 1.0 };
            let key = format!("dm-{}-{}-{i}", who.name(), m.at);
            let row: Div = if m.mine {
                asked = m.text.clone();
                let lead = !self.continues("You", m.at);
                self.message(
                    key,
                    m.text.clone(),
                    Msg::Mine(m.text.clone()),
                    (!lead).then_some(m.at),
                    channel::mine(m.text.clone(), m.at, lead),
                    cx,
                )
            } else if m.note {
                self.streak = None;
                div().py(px(4.)).child(note_line(&m.text))
            } else {
                let words = self.rich(format!("dm-{i}"), &plain(&m.text), &dir, cx);
                let offer = self.job_offer(who, m, OfferAt::Dm(who, m.at, m.text.clone()), cx);
                let said = div()
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .text_color(c(ui::body()))
                    .child(words)
                    .children(offer);
                let lead = !self.continues(who.name(), m.at);
                let line = self.bot_line(
                    who,
                    super::text_mood(&m.text),
                    m.at,
                    lead,
                    said,
                    m.via.clone(),
                );
                self.message(
                    key,
                    plain(&m.text),
                    Msg::Answer(who, asked.clone()),
                    (!lead).then_some(m.at),
                    line,
                    cx,
                )
            };
            body = body.child(ui::rise(row, k));
        }
        for j in jobs {
            body = day_line(self, &mut day, j.started, body);
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
            let lead = !self.continues(who.name(), super::store::now());
            body = body.child(self.bot_line(who, Mood::Thinking, 0, lead, dots, None));
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
                    .child(body),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(24.))
                    .pb(px(super::DOCK_ROOM))
                    .child(self.dm_composer(who, cx)),
            )
    }

    /// A bot's message in its DM, laid out as in a thread: face, name and
    /// time, the words, and which engine answered.
    fn bot_line(
        &mut self,
        who: Member,
        mood: Mood,
        at: u64,
        lead: bool,
        said: Div,
        via: Option<String>,
    ) -> Div {
        let head = lead.then(|| {
            let at = if at > 0 { at } else { super::store::now() };
            (
                self.face(who, mood, 34.).into_any_element(),
                channel::byline(who.name(), at).children(via.clone().map(channel::engine_note)),
            )
        });
        // A line that carries on a run has no byline: its engine goes under it.
        let under = via.filter(|_| !lead);
        let words = div()
            .flex()
            .flex_col()
            .child(said)
            .when_some(under, |d, via| {
                d.child(channel::engine_note(via).pt(px(2.)))
            });
        channel::line(head, words)
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
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.5))
                    .text_color(c(ui::muted()))
                    .child(
                        div()
                            .flex_none()
                            .when(busy, |d| d.text_color(c(tint)))
                            .child(if busy { word } else { who.role().to_string() }),
                    )
                    .child(div().text_color(c(ui::faint())).child("\u{b7}"))
                    .child(
                        div()
                            .id("chat-engine")
                            .truncate()
                            .cursor_pointer()
                            .hover(|s| s.text_color(c(ui::ink())))
                            .child(engine)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.go(Place::Settings(Pane::Engines), cx)
                            })),
                    ),
            )
            .child(div().flex_1())
            .child(
                ui::ghost_i("chat-profile", IconName::Users, "Profile")
                    .h(px(30.))
                    .flex_none()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.crew_pick = Some(who);
                        this.go(Place::Crew, cx);
                    })),
            )
            .child(
                ui::solid_i("chat-give", IconName::Plus, "Give a job")
                    .h(px(30.))
                    .flex_none()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.dm_do = true;
                        this.chat_input.update(cx, |i, cx| i.focus(window, cx));
                        cx.notify();
                    })),
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

    /// The top of a DM: a quiet note that only you and the bot see it, and
    /// while it's empty, a few things to ask it.
    fn dm_intro(&mut self, who: Member, empty: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let name = who.name();
        div()
            .w_full()
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .w_full()
                    .py(px(8.))
                    .flex()
                    .justify_center()
                    .rounded(px(6.))
                    .bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_size(px(12.5))
                    .text_color(c(ui::body()))
                    .child(format!(
                        "Direct messages with {name} \u{b7} only you and {name} see this"
                    )),
            )
            .when(empty, |d| d.child(self.chat_empty(who, cx)))
    }

    /// The box you type in, as a thread's: the words, who answers, and Send.
    fn dm_composer(&mut self, who: Member, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.chat_input.read(cx).value().to_string();
        let ready = !value.trim().is_empty();
        let face = self.avatar(who, 20.);
        // Ask has the bot answer; Do starts a job, as the design's chips.
        let do_it = self.dm_do;
        let mut hows = div().flex().items_center().gap(px(2.));
        for (i, name) in ["Ask", "Do"].into_iter().enumerate() {
            let on = (i == 1) == do_it;
            hows = hows.child(
                div()
                    .id(("chat-how", i))
                    .h(px(26.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .rounded(px(5.))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.bg(c(ui::pick_fill())).text_color(c(ui::pick_on()))
                        } else {
                            d.text_color(c(ui::muted()))
                                .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                        }
                    })
                    .child(name)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.dm_do = i == 1;
                        cx.notify();
                    })),
            );
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(14.))
            .pt(px(12.))
            .pb(px(10.))
            .rounded(px(8.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(if ready {
                super::theme::mix(ui::raised(), ui::pick_fill(), ui::pick_k(0.55).min(0.9))
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
                    .gap(px(6.))
                    .child(
                        div()
                            .id("chat-to")
                            .h(px(26.))
                            .pl(px(5.))
                            .pr(px(9.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(5.))
                            .bg(c(ui::well()))
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(ui::ink()))
                            .child(face)
                            .child(format!("To {}", who.name()))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(if do_it {
                                    format!(
                                        "{} works on it in its own copy of the code, and you review the changes.",
                                        who.name()
                                    )
                                } else {
                                    format!(
                                        "{} reads the code and answers. Nothing changes unless you pick Do.",
                                        who.name()
                                    )
                                })
                                .build(window, cx)
                            }),
                    )
                    .child(div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(c(ui::line())))
                    .child(hows)
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("chat-send")
                            .size(px(32.))
                            .rounded(px(7.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(c(ui::pick_fill()))
                            .map(|d| {
                                if ready {
                                    d.cursor_pointer().hover(|s| s.opacity(0.9))
                                } else {
                                    d.opacity(0.45)
                                }
                            })
                            .child(
                                Icon::new(IconName::ArrowUp)
                                    .size(px(16.))
                                    .text_color(c(ui::pick_on())),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let text = this.chat_input.read(cx).value().trim().to_string();
                                if !text.is_empty() {
                                    this.chat_input
                                        .update(cx, |i, cx| i.set_value("", window, cx));
                                    if this.dm_do {
                                        this.job_from_dm(who, text, cx);
                                    } else {
                                        this.ask(who, text, cx);
                                    }
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

    /// Who does a job a bot offered, and as what kind of work: the bot itself
    /// when it writes code, else the crew member for that work.
    fn offer_doer(bot: Member, task: &str) -> (Member, super::data::Mode) {
        if bot.mode().writes() {
            return (bot, bot.mode());
        }
        let mode = match super::data::Mode::guess(task) {
            m if m.writes() => m,
            _ => super::data::Mode::Build,
        };
        (
            Member::doer_for(mode).unwrap_or(Member::for_mode(mode)),
            mode,
        )
    }

    /// The job a bot offers under its answer: what it would do, what a job
    /// is, and Start or Not now. Once taken, a one-line receipt in its place,
    /// so the same offer never starts two jobs.
    pub(super) fn job_offer(
        &mut self,
        bot: Member,
        said: &Said,
        at: OfferAt,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if said.job.is_empty() || said.note || said.job_off {
            return None;
        }
        let key = match &at {
            OfferAt::Dm(who, when, _) => format!("{}-{when}", who.name()),
            OfferAt::Talk(id) => format!("talk-{id}"),
        };
        if said.job_taken {
            let id = said.job_id;
            let words = if id > 0 {
                format!("Started as job #{id}")
            } else {
                "Handed to the crew".to_string()
            };
            let mut chip = div()
                .id(SharedString::from(format!("offer-done-{key}")))
                .mt(px(8.))
                .h(px(26.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded_full()
                .bg(c(ui::well()))
                .text_size(px(12.5))
                .text_color(c(ui::muted()))
                .child(Icon::new(IconName::Check).size(px(13.)))
                .child(words);
            if id > 0 {
                chip = chip
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::VIOLET))
                            .child("Open"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Live(id), cx)));
            }
            return Some(div().flex().child(chip));
        }
        let (doer, _) = Self::offer_doer(bot, &said.job);
        let task = said.job.clone();
        let (go_at, off_at) = (at.clone(), at);
        Some(
            div()
                .mt(px(10.))
                .px(px(14.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .rounded(px(12.))
                .bg(ca(ui::VIOLET, 0.06))
                .border_1()
                .border_color(ca(ui::VIOLET, 0.28))
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::VIOLET))
                        .child(format!("{} can do this as a job", doer.name())),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(task.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .child(format!(
                            "{} works on its own copy of the code. You review the changes before anything lands.",
                            doer.name()
                        )),
                )
                .child(
                    div()
                        .pt(px(8.))
                        .flex()
                        .gap(px(8.))
                        .child(
                            ui::solid(SharedString::from(format!("offer-go-{key}")), "Start the job")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.take_job_offer(bot, task.clone(), go_at.clone(), cx)
                                })),
                        )
                        .child(
                            ui::ghost(SharedString::from(format!("offer-off-{key}")), "Not now")
                                .border_color(gpui::transparent_black())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(s) = this.offer_said(&off_at) {
                                        s.job_off = true;
                                    }
                                    this.crew.store.save();
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }

    /// The answer an offer sits under.
    fn offer_said(&mut self, at: &OfferAt) -> Option<&mut Said> {
        match at {
            OfferAt::Dm(who, when, text) => self.crew.store.dm_line_mut(*who, *when, text),
            OfferAt::Talk(id) => self
                .crew
                .store
                .talks
                .iter_mut()
                .find(|t| t.id == *id)
                .and_then(|t| t.answer.as_mut()),
        }
    }

    /// Starts the job a bot offered, once: the offer becomes its receipt.
    fn take_job_offer(&mut self, bot: Member, task: String, at: OfferAt, cx: &mut Context<Self>) {
        if self.offer_said(&at).is_none_or(|s| s.job_taken) {
            return;
        }
        let id = self.start_dm_job(bot, task, cx);
        if id.is_none() && self.sampling {
            return;
        }
        if let Some(s) = self.offer_said(&at) {
            s.job_taken = true;
            s.job_id = id.unwrap_or(0);
        }
        self.crew.store.save();
        // The job's card lands in this DM.
        if let OfferAt::Dm(who, ..) = at {
            self.go(Place::Chat(who), cx);
        }
        cx.notify();
    }

    /// "Start a job" in a DM's box: the words go straight to work.
    pub(super) fn job_from_dm(&mut self, who: Member, text: String, cx: &mut Context<Self>) {
        self.start_dm_job(who, text, cx);
        self.go(Place::Chat(who), cx);
        cx.notify();
    }

    /// Hands `task` to the bot that does it for `bot`, and returns the job it
    /// started, if one did (a plan or a missing bot goes another way).
    fn start_dm_job(&mut self, bot: Member, task: String, cx: &mut Context<Self>) -> Option<u64> {
        let (doer, mode) = Self::offer_doer(bot, &task);
        let before = self.crew.store.jobs.iter().map(|j| j.id).max().unwrap_or(0);
        let route = super::brain::route::Route {
            mode,
            bot: doer,
            sure: 1.0,
            why: String::new(),
            by: "dm".into(),
            site: String::new(),
        };
        self.hand_to(route, task, cx);
        self.crew
            .store
            .jobs
            .iter()
            .map(|j| j.id)
            .filter(|id| *id > before)
            .max()
    }
}
