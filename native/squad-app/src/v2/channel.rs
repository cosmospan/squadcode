//! A # thread, Crew Chat style: your asks and the crew's replies as a chat.
//! Peek's plans arrive as plan cards you start or drop; each job arrives as a
//! card that shows its progress live, with the buttons it needs right there.
//! The composer sits at the bottom, with `/` commands.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::{Framing, Mood};

use super::data::{Member, Mode};
use super::goal::{Goal, State};
use super::home::status_of;
use super::store::{self, Item, Job, Status};
use super::ui::{self, c, ca};
use super::{Place, Shell};

/// The commands the composer knows, with what each does.
pub(super) const COMMANDS: [(&str, &str); 9] = [
    ("/plan", "Peek plans it first, you press Start"),
    ("/race", "Try it on two engines, keep the best"),
    (
        "/later",
        "Start it later: /later tonight, 2am, in 2h or tomorrow",
    ),
    ("/ask", "A question, no code changes"),
    ("/note", "Pin a note every bot in this thread reads"),
    ("/thread", "Start a new thread: /thread gift cards @Builder"),
    ("/rename", "Rename this thread"),
    (
        "/archive",
        "Put this thread away; its jobs move to #general",
    ),
    ("/delete", "Delete this thread; its jobs move to #general"),
];

/// How wide the chat column reads at most.
const COLUMN: f32 = 760.;

/// How many entries a thread shows at first, and adds with each "Show earlier".
const THREAD_PAGE: usize = 30;

/// One thing in the thread, by when it happened.
enum Entry {
    Job(Box<Job>),
    Plan(Goal),
    Talk(store::Talk),
}

impl Entry {
    fn at(&self) -> u64 {
        match self {
            Entry::Job(j) => j.started,
            Entry::Talk(t) => t.at,
            Entry::Plan(g) => g.started,
        }
    }
}

/// Who a reply to a message goes to: the job's own conversation, or a bot
/// asked in the thread.
#[derive(Clone, Copy)]
pub(super) enum Reply {
    Job(u64),
    Bot(Member),
}

/// What a message in a thread is, for what its hover bar offers.
#[derive(Clone)]
pub(super) enum Msg {
    /// Your own words: ask again, edit and resend.
    Mine(String),
    /// A bot's reply about a job, in the state the job is in.
    Job(u64, Status, Member, String),
    /// Peek's plan; `true` while it waits for you to start it.
    Plan(u64, bool),
    /// A bot's answer to a question, and the question.
    Answer(Member, String),
}

/// A quick reaction that does something: its emoji, what it does (the
/// tooltip), the word it leaves on the message, and the action.
#[derive(Clone)]
struct Quick {
    emoji: &'static str,
    tip: String,
    done: &'static str,
    act: QuickAct,
}

#[derive(Clone)]
enum QuickAct {
    /// Approve a job's changes, as its Approve button does.
    Approve(u64),
    /// Run a failed or stopped job again.
    Retry(u64),
    /// Start a plan's jobs.
    Start(u64),
    /// Say this in the thread, as if typed.
    Say(String),
    /// Mark a finished job seen, so it leaves Needs you.
    Seen(u64),
    /// Nothing but the mark itself.
    Mark,
}

const OK: &str = "\u{2705}";
const AGAIN: &str = "\u{1F501}";
const LOOK: &str = "\u{1F440}";
const THUMB: &str = "\u{1F44D}";

/// The quick reactions a message offers, each with a use: approve or start
/// what waits on you, try again what failed, have another bot look, or
/// acknowledge it.
fn quick_for(m: &Msg) -> Vec<Quick> {
    let q = |emoji, tip: String, done, act| Quick {
        emoji,
        tip,
        done,
        act,
    };
    match m {
        Msg::Mine(text) => vec![q(
            AGAIN,
            "Ask again".into(),
            "Asked again",
            QuickAct::Say(text.clone()),
        )],
        Msg::Job(id, status, bot, label) => {
            let mut out = Vec::new();
            match status {
                Status::Review => {
                    out.push(q(
                        OK,
                        "Approve the changes".into(),
                        "Approved",
                        QuickAct::Approve(*id),
                    ));
                    out.push(q(
                        LOOK,
                        "Ask Checker to review the changes".into(),
                        "Checker asked",
                        QuickAct::Say(format!("@Checker review the changes in job #{id}: {label}")),
                    ));
                }
                Status::Failed | Status::Stopped | Status::Interrupted => {
                    out.push(q(
                        AGAIN,
                        "Try again".into(),
                        "Tried again",
                        QuickAct::Retry(*id),
                    ));
                    out.push(q(
                        LOOK,
                        "Ask Fixer why it stopped".into(),
                        "Fixer asked",
                        QuickAct::Say(format!("@Fixer find out why job #{id} stopped: {label}")),
                    ));
                    out.push(q(
                        THUMB,
                        "Got it, clear it from Needs you".into(),
                        "Seen",
                        QuickAct::Seen(*id),
                    ));
                }
                Status::Done | Status::Shipped => {
                    out.push(q(
                        THUMB,
                        "Got it, clear it from Needs you".into(),
                        "Seen",
                        QuickAct::Seen(*id),
                    ));
                    if *bot != Member::Checker {
                        out.push(q(
                            LOOK,
                            "Ask Checker to look it over".into(),
                            "Checker asked",
                            QuickAct::Say(format!("@Checker look over job #{id}: {label}")),
                        ));
                    }
                }
                _ => out.push(q(THUMB, "Thanks".into(), "Thanks", QuickAct::Mark)),
            }
            out
        }
        Msg::Plan(id, true) => vec![q(
            OK,
            "Start the crew on this plan".into(),
            "Started",
            QuickAct::Start(*id),
        )],
        Msg::Plan(..) => vec![q(THUMB, "Looks good".into(), "Looks good", QuickAct::Mark)],
        Msg::Answer(bot, question) => vec![
            q(THUMB, "Helpful".into(), "Helpful", QuickAct::Mark),
            q(
                LOOK,
                format!("Ask {} to dig deeper", bot.name()),
                "Asked for more",
                QuickAct::Say(format!("@{} go deeper on that: {question}", bot.name())),
            ),
        ],
    }
}

/// A small square button on a message's hover bar.
fn bar_btn(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_size(px(14.))
        .text_color(c(ui::muted()))
        .cursor_pointer()
        .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
}

fn tip_of(text: String) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text.clone()).build(window, cx)
}

impl Shell {
    /// Does a quick reaction's action and leaves its mark on the message.
    fn quick(&mut self, key: &str, q: &Quick, window: &mut Window, cx: &mut Context<Self>) {
        match &q.act {
            QuickAct::Approve(id) => self.approve(*id, cx),
            QuickAct::Retry(id) => self.retry_job(*id, cx),
            QuickAct::Start(id) => self.start_goal(*id, cx),
            QuickAct::Say(text) => match self.dm() {
                // In a DM, words for this bot stay in the DM; asking another
                // bot to look makes a job, as in a thread.
                Some(who) if text.starts_with(&format!("@{} ", who.name())) => {
                    let rest = text[who.name().len() + 2..].to_string();
                    self.ask(who, rest, cx);
                }
                Some(_) => self.hand_over(text.clone(), cx),
                None => {
                    self.composer
                        .update(cx, |i, cx| ui::put_text(i, text.clone(), window, cx));
                    self.send_from_composer(window, cx);
                }
            },
            QuickAct::Seen(id) => {
                if let Some(j) = self.crew.store.job_mut(*id) {
                    j.seen = true;
                }
                self.crew.store.save();
            }
            QuickAct::Mark => {}
        }
        let mark = format!("{} {}", q.emoji, q.done);
        let on = self
            .crew
            .store
            .reactions
            .get(key)
            .is_some_and(|l| l.contains(&mark));
        if !on {
            self.crew.store.toggle_reaction(key, &mark);
        }
        cx.notify();
    }

    /// One message in a thread, yours or a bot's: the message, the marks
    /// quick reactions left on it, and while the pointer is over this message
    /// alone, a bar at its top right with what it offers, Reply and Copy.
    /// The bot whose DM is open, if one is.
    pub(super) fn dm(&self) -> Option<Member> {
        match self.place {
            Place::Chat(who) => Some(who),
            _ => None,
        }
    }

    /// Puts words in the box you type in here: the DM's, or the thread's.
    pub(super) fn put_in_box(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.dm().is_some() {
            self.chat_input.update(cx, |i, cx| {
                i.set_value(text, window, cx);
                i.focus(window, cx);
            });
        } else {
            self.composer.update(cx, |i, cx| {
                ui::put_text(i, text, window, cx);
                i.focus(window, cx);
            });
        }
        cx.notify();
    }

    /// `quiet` is the time of a line that carries on a run, without its own
    /// name and time: the time shows in the left margin while you point at it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn message(
        &mut self,
        key: String,
        copy: String,
        what: Msg,
        quiet: Option<u64>,
        body: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> Div {
        let group: SharedString = format!("msg-{key}").into();
        let n = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut h);
            h.finish() as usize
        };
        let marks: Vec<String> = self
            .crew
            .store
            .reactions
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let mut bar = div()
            .absolute()
            .top(px(-14.))
            .right(px(10.))
            .p(px(2.))
            .flex()
            .items_center()
            .gap(px(1.))
            .rounded(px(8.))
            .bg(c(ui::float()))
            .border_1()
            .border_color(c(ui::line()))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.08),
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
                inset: false,
            }])
            .invisible()
            .group_hover(group.clone(), |s| s.visible());
        for (k, q) in quick_for(&what).into_iter().enumerate() {
            let key = key.clone();
            let tip = q.tip.clone();
            bar = bar.child(
                bar_btn(("quick", n.wrapping_mul(8) + k))
                    .child(q.emoji)
                    .tooltip(tip_of(tip))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.quick(&key, &q, window, cx)),
                    ),
            );
        }
        let reply = match &what {
            Msg::Job(id, ..) => Some(Reply::Job(*id)),
            Msg::Answer(bot, _) => Some(Reply::Bot(*bot)),
            Msg::Plan(..) => Some(Reply::Bot(Member::Fin)),
            Msg::Mine(_) => None,
        };
        let edit = match &what {
            Msg::Mine(text) => Some(text.clone()),
            _ => None,
        };
        let right = self.on_menu(
            super::menu::Ctx::Message {
                copy: copy.clone(),
                reply,
                edit: edit.clone(),
                job: match &what {
                    Msg::Job(id, ..) => Some(*id),
                    _ => None,
                },
            },
            cx,
        );
        bar = bar
            .child(div().w(px(1.)).h(px(16.)).mx(px(2.)).bg(c(ui::line())))
            .when_some(reply, |d, reply| {
                d.child(
                    bar_btn(("reply", n))
                        .child(Icon::new(IconName::MessageSquareText).size(px(15.)))
                        .tooltip(tip_of("Reply".into()))
                        .on_click(cx.listener(move |this, _, window, cx| match reply {
                            Reply::Job(id) => {
                                this.reply_open = Some(id);
                                cx.notify();
                            }
                            // In its own DM a bot needs no @.
                            Reply::Bot(who) if this.dm() == Some(who) => {
                                this.put_in_box(String::new(), window, cx)
                            }
                            Reply::Bot(who) => {
                                this.put_in_box(format!("@{} ", who.name()), window, cx)
                            }
                        })),
                )
            })
            .when_some(edit, |d, text| {
                d.child(
                    bar_btn(("edit", n))
                        .child(Icon::new(IconName::Pencil).size(px(14.)))
                        .tooltip(tip_of("Edit and send again".into()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.put_in_box(text.clone(), window, cx)
                        })),
                )
            })
            .child(
                bar_btn(("copy", n))
                    .child(Icon::new(IconName::Copy).size(px(14.)))
                    .tooltip(tip_of("Copy text".into()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                        this.say("Copied.");
                        cx.notify();
                    })),
            );
        let marks = (!marks.is_empty()).then(|| {
            let mut row = div().pl(px(46.)).pb(px(4.)).flex().flex_wrap().gap(px(6.));
            for (k, mark) in marks.into_iter().enumerate() {
                let key = key.clone();
                let off = mark.clone();
                row = row.child(
                    div()
                        .id(("mark", n.wrapping_mul(8) + k))
                        .h(px(24.))
                        .px(px(9.))
                        .flex()
                        .items_center()
                        .rounded_full()
                        .bg(ca(ui::VIOLET, 0.08))
                        .border_1()
                        .border_color(ca(ui::VIOLET, 0.3))
                        .text_size(px(12.))
                        .text_color(c(ui::body()))
                        .cursor_pointer()
                        .hover(|s| s.bg(ca(ui::VIOLET, 0.16)))
                        .tooltip(tip_of("Click to clear the mark".into()))
                        .child(mark)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.crew.store.toggle_reaction(&key, &off);
                            cx.notify();
                        })),
                );
            }
            row
        });
        let stamp = quiet.map(|at| {
            div()
                .absolute()
                .left(px(10.))
                .top(px(5.))
                .w(px(34.))
                .flex()
                .justify_center()
                .text_size(px(10.5))
                .line_height(px(21.))
                .text_color(c(ui::faint()))
                .invisible()
                .group_hover(group.clone(), |s| s.visible())
                .child(clock(at))
        });
        // The hover ground reaches a little past the face and the words, so
        // they don't sit on its edge.
        div()
            .group(group)
            .relative()
            .mx(px(-10.))
            .px(px(10.))
            .rounded(px(8.))
            .hover(|s| s.bg(ca(ui::ink(), 0.03)))
            .on_mouse_down(MouseButton::Right, right)
            .child(body)
            .children(marks)
            .children(stamp)
            .child(bar)
    }

    /// Whether `who` speaking at `at` carries on the run of messages just
    /// above, so the face and name can be left off. Notes them as the last to
    /// speak either way.
    pub(super) fn continues(&mut self, who: &str, at: u64) -> bool {
        let goes_on = self
            .streak
            .as_ref()
            .is_some_and(|(w, t)| w == who && at >= *t && at - *t < RUN_SECS);
        self.streak = Some((who.to_string(), at));
        goes_on
    }
}

/// How long a pause still reads as one run of messages from the same author.
const RUN_SECS: u64 = 5 * 60;

/// A thread summary as it reads: each heading ("Decided:") in small bold
/// with its lines under it, and headings with nothing under them left out.
fn summary_lines(text: &str) -> Div {
    let mut out = div().flex().flex_col().gap(px(2.));
    if !text.lines().any(|l| l.trim().ends_with(':')) {
        return out.child(
            div()
                .text_size(px(13.))
                .text_color(c(ui::muted()))
                .child(text.to_string()),
        );
    }
    let mut heading: Option<String> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let item = line.trim_start_matches(['-', '\u{2022}']).trim();
        if line.ends_with(':') && !line.starts_with('-') {
            heading = Some(match line.trim_end_matches(':') {
                "For" => "What it's for".to_string(),
                h => h.to_string(),
            });
            continue;
        }
        if item.eq_ignore_ascii_case("none") {
            continue;
        }
        if let Some(h) = heading.take() {
            out = out.child(
                div()
                    .pt(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(h),
            );
        }
        out = out.child(
            div()
                .text_size(px(13.))
                .line_height(px(19.))
                .text_color(c(ui::body()))
                .child(format!("\u{2022} {item}")),
        );
    }
    out
}

/// "Today", "Yesterday", or the day's name or date, for the line between days.
pub(super) fn day_label(at: u64) -> String {
    use chrono::{Local, TimeZone};
    let Some(then) = Local.timestamp_opt(at as i64, 0).single() else {
        return String::new();
    };
    let days = (Local::now().date_naive() - then.date_naive()).num_days();
    match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => then.format("%A").to_string(),
        _ => then.format("%a %-d %b").to_string(),
    }
}

/// A line across the feed with a word in the middle: a day, or "New" in red
/// where what you haven't seen starts.
pub(super) fn divider(text: &str, tint: Option<u32>) -> Div {
    let line = tint.unwrap_or(ui::line());
    div()
        .w_full()
        .my(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().flex_1().h(px(1.)).bg(c(line)))
        .child(
            div()
                .px(px(10.))
                .py(px(2.))
                .rounded_full()
                .border_1()
                .border_color(c(line))
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(tint.unwrap_or(ui::muted())))
                .child(text.to_string()),
        )
        .child(div().flex_1().h(px(1.)).bg(c(line)))
}

fn when(at: u64) -> String {
    match store::ago(at).as_str() {
        "just now" => "now".to_string(),
        a => a.to_string(),
    }
}

/// The time of day a message was sent, like 08:12. The day is on the line
/// between days, and a clock time doesn't go stale the way "15m" does.
pub(super) fn clock(at: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(at as i64, 0)
        .single()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// A message's header: who, and when.
pub(super) fn byline(name: &str, at: u64) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(13.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::ink()))
                .child(name.to_string()),
        )
        .child(
            div()
                .font_family(ui::MONO)
                .text_size(px(11.))
                .text_color(c(ui::faint()))
                .child(clock(at)),
        )
}

/// Which engine answered and how long it took, quiet, after a bot's name.
pub(super) fn engine_note(via: String) -> Div {
    div()
        .text_size(px(11.))
        .text_color(c(ui::faint()))
        .child(via)
}

/// A bot's role beside its name: a small tag in its own colour.
pub(super) fn role_tag(text: impl Into<SharedString>, accent: u32) -> Div {
    div()
        .px(px(5.))
        .rounded(px(4.))
        .bg(ca(accent, 0.1))
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(super::theme::mix(accent, ui::ink(), 0.4)))
        .child(text.into())
}

/// A line in the chat, Slack style. The first of a run has the face in a
/// column at the left, and the name, tag and time over the words; the rest of
/// the run is the words alone, lined up under the name.
pub(super) fn line(head: Option<(AnyElement, Div)>, words: impl IntoElement) -> Div {
    let lead = head.is_some();
    let (face, by) = head.unzip();
    div()
        .flex()
        .gap(px(12.))
        .map(|d| {
            if lead {
                d.pt(px(8.)).pb(px(3.))
            } else {
                d.py(px(2.))
            }
        })
        .child(
            div()
                .w(px(32.))
                .flex_none()
                .flex()
                .justify_center()
                .children(face.map(|f| {
                    div()
                        .size(px(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(f)
                })),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(by)
                .child(words),
        )
}

/// The design's dashed progress bar: 6px dashes with 2px gaps in `col`,
/// `frac` of the way along a hairline track.
pub(super) fn dashed_bar(frac: f32, col: u32) -> Div {
    let mut dashes = div().h_full().flex().gap(px(2.)).overflow_hidden();
    for _ in 0..80 {
        dashes = dashes.child(div().w(px(6.)).h_full().flex_none().bg(c(col)));
    }
    div()
        .h(px(3.))
        .w_full()
        .bg(c(ui::line()))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(frac.clamp(0.04, 1.)))
                .overflow_hidden()
                .child(dashes),
        )
}

/// A card under a message: white with a hairline edge.
fn card() -> Div {
    div()
        .mt(px(8.))
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(6.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line()))
        .overflow_hidden()
}

/// The first letter of the person's name, for their square in the chat.
pub(super) fn you_letter() -> String {
    std::env::var("USER")
        .ok()
        .and_then(|u| u.chars().next())
        .filter(|ch| ch.is_alphabetic())
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_else(|| "Y".into())
}

/// The person's own message: with their face and name when it starts a run,
/// the words alone when it carries one on.
pub(super) fn mine(text: String, at: u64, lead: bool) -> Div {
    let face = div()
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .bg(c(ui::pick_fill()))
        .text_color(c(ui::pick_on()))
        .font_family(ui::MONO)
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .child(you_letter())
        .into_any_element();
    line(
        lead.then(|| (face, byline("You", at))),
        div()
            .text_size(px(14.))
            .line_height(px(21.))
            .text_color(c(ui::body()))
            .child(tagged(text)),
    )
}

/// Your words with @bots and #threads picked out in colour.
fn tagged(text: String) -> StyledText {
    let mut runs = Vec::new();
    let mut at = 0;
    for word in text.split_inclusive(char::is_whitespace) {
        let w = word.trim_end();
        if (w.starts_with('@') || w.starts_with('#')) && w.len() > 1 {
            let end = at
                + w.trim_end_matches(|ch: char| !ch.is_alphanumeric() && ch != '-')
                    .len();
            runs.push((
                at..end,
                HighlightStyle {
                    color: Some(c(ui::accent_text())),
                    font_weight: Some(FontWeight::SEMIBOLD),
                    background_color: Some(ca(ui::pick_fill(), 0.25)),
                    ..Default::default()
                },
            ));
        }
        at += word.len();
    }
    StyledText::new(text).with_highlights(runs)
}

impl Shell {
    /// The thread you're in: its header line, the chat, and the composer.
    pub(super) fn channel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(folder) = self.crew.store.folder().cloned() else {
            return self.today(cx);
        };
        let thread = self.crew.store.thread_here();
        let name = self.crew.store.thread_label(thread);
        // When you last looked, held while you stay in the thread, so the
        // New line stays put instead of moving with every redraw.
        let seen_key = format!("{}#{thread}", folder.path);
        if self.new_mark.0 != seen_key {
            let before = self.crew.store.seen_at.get(&seen_key).copied().unwrap_or(0);
            self.new_mark = (seen_key, before, store::now());
        }
        let since = self.new_mark.1;
        // When you came in: Peek's catch-up is posted then, and covers only
        // what happened while you were away.
        let arrived = self.new_mark.2;
        // Peek's catch-up: one line in a fixed place at the top of the thread.
        let catch_up = self.catch_up_bar(&folder.path, thread, since, arrived);
        self.crew.store.mark_seen(&folder.path, thread);
        // What's in the thread, by index and time only: this runs on every
        // frame, so only the entries that are drawn get copied.
        #[derive(Clone, Copy)]
        enum Slot {
            Job(usize),
            Plan(usize),
            Talk(usize),
        }
        let store = &self.crew.store;
        let mut slots: Vec<(u64, bool, Slot)> = Vec::new();
        for (i, j) in store.jobs.iter().enumerate() {
            if j.folder.path == folder.path
                && j.goal == 0
                && (j.race == 0 || j.race == j.id)
                && store.thread_for(&folder.path, j.thread) == thread
            {
                slots.push((j.started, true, Slot::Job(i)));
            }
        }
        for (i, g) in store.goals.iter().enumerate() {
            if g.folder.path == folder.path && store.thread_for(&folder.path, g.thread) == thread {
                slots.push((g.started, true, Slot::Plan(i)));
            }
        }
        for (i, t) in store.talks.iter().enumerate() {
            if t.folder == folder.path && store.thread_for(&folder.path, t.thread) == thread {
                slots.push((t.at, false, Slot::Talk(i)));
            }
        }
        // In the same second, a bot's message comes before the job it starts.
        slots.sort_by_key(|(at, not_talk, _)| (*at, *not_talk));
        let total = slots.len();
        // The newest page, newest at the bottom beside the composer; "Show
        // earlier" above it adds a page at a time.
        let thread_id = format!("{}#{thread}", folder.path);
        if self.thread_more.0 != thread_id {
            self.thread_more = (thread_id, 0);
        }
        let want = THREAD_PAGE + self.thread_more.1 * THREAD_PAGE;
        let first = total.saturating_sub(want);
        let earlier = first;
        // Jump to the newest when the thread, or what's in it, changes. Only
        // the newest page counts, so showing earlier entries keeps your place.
        // A plain top-down list with its own scroll: a bottom-up one let the
        // wheel run past the last message into empty space.
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            folder.path.hash(&mut h);
            name.hash(&mut h);
            total.hash(&mut h);
            // A run's card appearing, or opening up, makes the thread longer.
            for r in &self.runs {
                r.shape().hash(&mut h);
            }
            for (at, _, slot) in &slots[total.saturating_sub(THREAD_PAGE)..] {
                at.hash(&mut h);
                if let Slot::Job(i) = slot {
                    let j = &store.jobs[*i];
                    j.feed.len().hash(&mut h);
                    (j.status as u8).hash(&mut h);
                }
            }
            h.finish()
        };
        let shown: Vec<Entry> = slots[first..]
            .iter()
            .map(|(_, _, slot)| match *slot {
                Slot::Job(i) => Entry::Job(Box::new(store.jobs[i].clone())),
                Slot::Plan(i) => Entry::Plan(store.goals[i].clone()),
                Slot::Talk(i) => Entry::Talk(store.talks[i].clone()),
            })
            .collect();
        if key != self.thread_key {
            self.thread_key = key;
            // Again on the next few frames: the scroll only knows the last
            // frame's height, and what just arrived (a card) is taller.
            self.thread_settle = 3;
        }
        if self.thread_settle > 0 {
            self.thread_settle -= 1;
            self.thread_scroll.scroll_to_bottom();
        }
        // Apps whose card is at the bottom of the thread already don't need
        // their line over the composer too.
        let near: Vec<u64> = {
            let (talks, jobs): (Vec<u64>, Vec<u64>) = shown
                .iter()
                .rev()
                .take(3)
                .map(|e| match e {
                    Entry::Talk(t) => (t.id, 0),
                    Entry::Job(j) => (0, j.id),
                    Entry::Plan(_) => (0, 0),
                })
                .unzip();
            self.runs
                .iter()
                .filter(|r| {
                    r.spec.talk.is_some_and(|t| talks.contains(&t))
                        || (r.spec.talk.is_none() && r.spec.job.is_some_and(|j| jobs.contains(&j)))
                })
                .map(|r| r.id)
                .collect()
        };
        let empty = shown.is_empty();
        let mut feed = div()
            .w_full()
            .max_w(px(COLUMN))
            .flex()
            .flex_col()
            .child(self.thread_intro(thread, &name, empty, total));
        if earlier > 0 {
            let n = earlier.min(THREAD_PAGE);
            feed = feed.child(
                div().w_full().flex().justify_center().py(px(10.)).child(
                    div()
                        .id("thread-earlier")
                        .px(px(12.))
                        .py(px(5.))
                        .rounded_full()
                        .border_1()
                        .border_color(c(ui::line()))
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .cursor_pointer()
                        .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                        .child(if earlier > n {
                            format!("Show {n} earlier \u{b7} {earlier} more in this thread")
                        } else {
                            format!("Show {n} earlier")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.thread_more.1 += 1;
                            cx.notify();
                        })),
                ),
            );
        }
        let mut day = String::new();
        let mut marked = since == 0;
        self.streak = None;
        for e in shown {
            let at = e.at();
            let today = day_label(at);
            if today != day {
                feed = feed.child(divider(&today, None));
                day = today;
                self.streak = None;
            }
            if !marked && at > since {
                marked = true;
                feed = feed.child(divider("New", Some(ui::RED)));
                self.streak = None;
            }
            feed = feed.child(match e {
                Entry::Job(j) => self.job_entry(&j, cx).into_any_element(),
                Entry::Plan(g) => self.plan_entry(&g, cx).into_any_element(),
                Entry::Talk(t) => self.talk_entry(&t, cx).into_any_element(),
            });
        }

        // Short threads sit at the bottom, next to the composer.
        let chat = div()
            .id("thread-feed")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.thread_scroll)
            .child(
                div()
                    .min_h_full()
                    .w_full()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .items_center()
                    .px(px(28.))
                    .pt(px(20.))
                    .pb(px(12.))
                    .child(feed),
            );

        let offer = self.fin_offer(cx);
        let head = self.channel_head(thread, &name, cx);
        let page = div()
            .id("thread")
            .size_full()
            .flex()
            .flex_col()
            .child(head)
            .children(self.about_thread(&folder.path, thread, cx))
            .children(catch_up.map(|bar| self.catch_up_line(bar, cx)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .children(ui::feed_grid())
                    .child(chat),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px(px(28.))
                    .pb(px(18.))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(COLUMN))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(self.runs_strip(&folder.path, &near, cx))
                            .children(self.waiting_strip(&folder.path, thread, cx))
                            .children(offer)
                            .children(self.slash_strip(cx))
                            .children(self.hash_strip(cx))
                            .child(self.thread_composer(&name, cx)),
                    ),
            )
            .into_any_element();
        // A job's own conversation, open beside the thread like a reply pane.
        let pane = self
            .reply_open
            .and_then(|id| self.crew.store.job(id).cloned())
            .filter(|j| {
                j.folder.path == folder.path
                    && self.crew.store.thread_for(&folder.path, j.thread) == thread
            })
            .map(|j| self.peek_pane(&j, &name, cx));
        let page = div()
            .size_full()
            .flex()
            .child(div().flex_1().min_w_0().h_full().child(page))
            .children(pane)
            .into_any_element();
        let pick = None;
        self.split(pick, page, cx)
    }

    /// What happened in this thread while you were away, in one line: how
    /// many jobs finished, wait on you, and failed. Worked out each time.
    fn catch_up_bar(&self, path: &str, thread: u64, since: u64, arrived: u64) -> Option<String> {
        if since == 0 {
            return None;
        }
        let store = &self.crew.store;
        let ended: Vec<&Job> = store
            .listed()
            .into_iter()
            .filter(|j| j.folder.path == path && store.thread_for(path, j.thread) == thread)
            .filter(|j| j.ended.is_some_and(|e| e > since && e <= arrived))
            .collect();
        let done = ended
            .iter()
            .filter(|j| matches!(j.status, Status::Done | Status::Shipped))
            .count();
        let review = ended.iter().filter(|j| j.status == Status::Review).count();
        let failed = ended.iter().filter(|j| j.status == Status::Failed).count();
        catch_up_words(done, review, failed)
    }

    /// The catch-up line itself: Peek's face, the words and Jump to new.
    fn catch_up_line(&mut self, words: String, cx: &mut Context<Self>) -> Div {
        div()
            .mx(px(28.))
            .mt(px(10.))
            .h(px(34.))
            .px(px(12.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(9.))
            .rounded(px(5.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .text_size(px(12.5))
            .text_color(c(ui::muted()))
            .child(self.avatar(Member::Fin, 16.))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child("While you were away"),
            )
            .child(div().flex_1().min_w_0().truncate().child(words))
            .child(
                div()
                    .id("catch-up-jump")
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child("Jump to new")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.thread_scroll.scroll_to_bottom();
                        cx.notify();
                    })),
            )
    }

    /// A job's conversation beside its thread: what the bot did and said,
    /// with a box to answer it, like replies in a thread.
    /// The bar over a thread, like a channel's: its name, who's in it as a
    /// row of faces, and Add a bot, which starts an @ in the box.
    /// "About this thread": what the thread is for in one line, opening to
    /// the whole summary bots read, the person's pinned notes (each can go),
    /// and Rewrite now. Nothing until the thread has something in it.
    fn about_thread(&mut self, folder: &str, thread: u64, cx: &mut Context<Self>) -> Option<Div> {
        let sum = self.crew.store.summary(folder, thread).cloned();
        let said = self.crew.store.said_in(folder, thread).len();
        let writing = self.crew.summing(folder, thread);
        if said == 0 && sum.as_ref().is_none_or(|s| s.notes.is_empty()) {
            return None;
        }
        let sum = sum.unwrap_or_default();
        let open = self.about_open;
        let headline = super::brain::summary::headline(&sum.text);
        let line = if !headline.is_empty() {
            headline
        } else if writing {
            "Peek is writing a summary\u{2026}".to_string()
        } else if let Some(n) = sum.notes.first() {
            n.clone()
        } else {
            format!(
                "Peek sums this thread up after {} messages, so every bot knows what it's about.",
                super::brain::summary::EVERY
            )
        };
        if !open {
            return None;
        }
        let _ = line;
        let mut body = div()
            .w_full()
            .max_w(px(COLUMN))
            .mx_auto()
            .px(px(28.))
            .py(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.));
        let label = |t: &str| {
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::faint()))
                .child(t.to_string())
        };
        let mut notes = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(label("YOUR NOTES (THESE WIN)"));
        if sum.notes.is_empty() {
            notes = notes.child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::muted()))
                    .child("None yet. Type /note and what bots should always know here, like \u{201c}this thread is for discussion, not building.\u{201d}"),
            );
        }
        for (i, n) in sum.notes.iter().enumerate() {
            let folder = folder.to_string();
            notes = notes.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .text_color(c(ui::ink()))
                            .child(format!("\u{2022} {n}")),
                    )
                    .child(
                        ui::icon_btn(("about-note-off", i), IconName::X)
                            .size(px(24.))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Remove this note")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let s = this.crew.store.summary_mut(&folder, thread);
                                if i < s.notes.len() {
                                    s.notes.remove(i);
                                }
                                this.crew.store.save();
                                cx.notify();
                            })),
                    ),
            );
        }
        body = body.child(notes);
        let text = if sum.text.trim().is_empty() {
            if writing {
                "Peek is writing it now.".to_string()
            } else {
                format!(
                    "Not written yet. Peek writes it after {} messages, or press Rewrite now.",
                    super::brain::summary::EVERY
                )
            }
        } else {
            sum.text.clone()
        };
        let when = if sum.at > 0 {
            format!(
                "Written {} \u{b7} covers {} messages \u{b7} bots read it with the last {} word for word",
                store::ago(sum.at),
                sum.covers,
                super::brain::summary::RECENT
            )
        } else {
            String::new()
        };
        let path = folder.to_string();
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(label("SUMMARY BY PEEK"))
                .child(summary_lines(&text))
                .child(
                    div()
                        .pt(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.5))
                                .text_color(c(ui::faint()))
                                .child(when),
                        )
                        .when(said > 0 && !writing, |d| {
                            d.child(
                                ui::ghost("about-rewrite", "Rewrite now")
                                    .h(px(26.))
                                    .tooltip(|window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(
                                            "Peek writes it afresh from this thread. Notes stay as they are.",
                                        )
                                        .build(window, cx)
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.crew.summary_now = Some((path.clone(), thread));
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
        );
        Some(
            div().w_full().flex().flex_col().child(
                div()
                    .w_full()
                    .bg(c(ui::well()))
                    .border_b_1()
                    .border_color(c(ui::line()))
                    .child(body),
            ),
        )
    }

    fn channel_head(&mut self, thread: u64, name: &str, cx: &mut Context<Self>) -> Div {
        let here = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        let bots = self.crew.store.members(&here, thread);
        let busy = bots
            .iter()
            .filter(|b| self.crew.store.busy(**b).is_some())
            .count();
        let mut faces = div().flex().items_center();
        let shell = cx.entity().downgrade();
        for (i, b) in bots.iter().take(5).enumerate() {
            let f = self.avatar(*b, 20.);
            let card = self.bot_card(*b, &here, thread, shell.clone());
            faces = faces.child(
                div()
                    .id(("channel-face", i))
                    .hoverable_tooltip(move |_, cx| cx.new(|_| card.clone()).into())
                    .when(i > 0, |d| d.ml(px(2.)))
                    .flex()
                    .items_center()
                    .child(f),
            );
        }
        let _ = busy;
        // What the thread is about, in one quiet line: Peek's summary, else
        // the first pinned note. Clicking it opens the summary and notes.
        let sum = self
            .crew
            .store
            .summary(&here, thread)
            .cloned()
            .unwrap_or_default();
        let headline = super::brain::summary::headline(&sum.text);
        let about = if !headline.is_empty() {
            headline
        } else {
            sum.notes.first().cloned().unwrap_or_default()
        };
        let pinned = sum.notes.len();
        let toggle = |this: &mut Self, cx: &mut Context<Self>| {
            this.about_open = !this.about_open;
            cx.notify();
        };
        div()
            .h(px(50.))
            .flex_none()
            .w_full()
            .px(px(20.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .flex_none()
                    .font_family(ui::MONO)
                    .text_size(px(15.))
                    .text_color(c(ui::accent_text()))
                    .child("#"),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(name.to_string()),
            )
            .child(
                div()
                    .id("channel-about")
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .text_color(c(ui::faint()))
                    .cursor_pointer()
                    .hover(|s| s.text_color(c(ui::muted())))
                    .child(if about.is_empty() {
                        "What this thread is about".to_string()
                    } else {
                        about
                    })
                    .on_click(cx.listener(move |this, _, _, cx| toggle(this, cx))),
            )
            .child(
                div()
                    .id("channel-members")
                    .ml(px(4.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .cursor_pointer()
                    .child(faces)
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("The bots in this thread")
                            .build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Crew, cx))),
            )
            .child(
                ui::icon_btn("channel-add-bot", IconName::Plus)
                    .size(px(24.))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Add a bot").build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.composer.update(cx, |i, cx| {
                            let text = i.value().to_string();
                            let text = if text.is_empty() || text.ends_with(' ') {
                                format!("{text}@")
                            } else {
                                format!("{text} @")
                            };
                            ui::put_text(i, text, window, cx);
                            i.focus(window, cx);
                        });
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .when(pinned > 0, |d| {
                d.child(
                    ui::ghost_i(
                        "channel-pinned",
                        IconName::Bookmark,
                        format!("{pinned} pinned"),
                    )
                    .h(px(28.))
                    .flex_none()
                    .border_color(transparent_black())
                    .text_color(c(ui::muted()))
                    .on_click(cx.listener(move |this, _, _, cx| toggle(this, cx))),
                )
            })
    }

    /// What a face in the bar shows on hover: who the bot is, what it's on,
    /// and its last job in this thread, each a link.
    fn bot_card(
        &mut self,
        who: Member,
        here: &str,
        thread: u64,
        shell: WeakEntity<Shell>,
    ) -> BotCard {
        let face = self.avatar_image(who, 40.);
        let (state, tone) = self.status_of(who);
        let store = &self.crew.store;
        let mut jobs = Vec::new();
        let now = store.busy(who);
        if let Some(j) = now {
            jobs.push(CardJob::of("Working on", j));
        }
        let last = store
            .jobs_in(here, thread)
            .into_iter()
            .rev()
            .find(|j| j.bot == who && Some(j.id) != now.map(|n| n.id));
        if let Some(j) = last {
            jobs.push(CardJob::of("Last job here", j));
        }
        BotCard {
            shell,
            who,
            face,
            state,
            tone,
            jobs,
        }
    }

    /// A question asked in the thread, and the bot's answer under it.
    fn talk_entry(&mut self, t: &store::Talk, cx: &mut Context<Self>) -> Div {
        let face = match t.answer.as_ref().filter(|a| !a.note) {
            Some(a) => self.face(t.bot, super::text_mood(&a.text), 30.),
            None => self.avatar(t.bot, 30.),
        };
        // Peek reading a message with its engine shows as thinking too.
        let reading = self.reading_talk() == Some(t.id);
        let waiting = t.answer.is_none() && (self.crew.asking_talk == Some(t.id) || reading);
        let offer = t
            .answer
            .as_ref()
            .and_then(|a| self.job_offer(t.bot, a, super::chat::OfferAt::Talk(t.id), cx));
        let answer: Div = match &t.answer {
            Some(a) if a.note => super::live::note_line(&a.text),
            Some(a) => div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .text_color(c(ui::body()))
                        .child(self.rich(
                            format!("talk-{}", t.id),
                            &super::live::plain(&a.text),
                            &t.folder,
                            cx,
                        )),
                )
                .children(offer),
            None if waiting => {
                let secs = if reading {
                    self.reading_since.map(|at| at.elapsed().as_secs())
                } else {
                    self.crew
                        .asking
                        .as_ref()
                        .map(|a| a.started.elapsed().as_secs())
                }
                .unwrap_or(0);
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(super::live::typing(self.now()))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(ui::muted()))
                            .child(format!("thinking \u{b7} {secs}s")),
                    )
            }
            None => super::live::note_line("No answer came back. Ask again."),
        };
        let refs = self.ref_chips(t.id, &t.question, cx);
        let copy = t
            .answer
            .as_ref()
            .map(|a| super::live::plain(&a.text))
            .unwrap_or_default();
        // A bot-to-bot message has no question: only the bot's message shows.
        let asked = (!t.question.is_empty()).then(|| {
            let lead = !self.continues("You", t.at);
            self.message(
                format!("you-talk-{}", t.id),
                t.question.clone(),
                Msg::Mine(t.question.clone()),
                (!lead).then_some(t.at),
                mine(t.question.clone(), t.at, lead),
                cx,
            )
        });
        let choices = (!t.choices.is_empty()).then(|| {
            let mut row = div().pt(px(4.)).flex().flex_wrap().gap(px(6.));
            let talk = t.id;
            for (k, choice) in t.choices.iter().enumerate() {
                let said = choice.clone();
                row = row.child(
                    ui::ghost(("talk-choice", t.id as usize * 16 + k), choice.clone())
                        .h(px(28.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pick_choice(talk, said.clone(), cx)
                        })),
                );
            }
            row
        });
        let lead = !self.continues(t.bot.name(), t.at);
        let via = t
            .answer
            .as_ref()
            .filter(|a| !a.note)
            .and_then(|a| a.via.clone());
        let head = lead.then(|| {
            (
                face.into_any_element(),
                byline(t.bot.name(), t.at).children(via.map(engine_note)),
            )
        });
        let runs = self.talk_runs(t.id, cx);
        let said = div()
            .flex()
            .flex_col()
            .child(answer)
            .children(choices)
            .children(runs);
        div()
            .flex()
            .flex_col()
            .children(asked)
            .children(refs)
            .child(self.message(
                format!("talk-{}", t.id),
                copy,
                Msg::Answer(t.bot, t.question.clone()),
                (!lead).then_some(t.at),
                line(head, said),
                cx,
            ))
    }

    /// The top of a thread: what it's for and who's in it.
    fn thread_intro(&mut self, thread: u64, name: &str, empty: bool, total: usize) -> Div {
        let here = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        // Who's here shows once, as faces in the bar above; here it's words.
        let bots = self.crew.store.members(&here, thread);
        let names: Vec<&str> = bots.iter().map(|b| b.name()).collect();
        let who = if bots.len() == 1 {
            "Peek is here. Bring a bot in with @, like @Builder, or Peek brings one in when there's work."
                .to_string()
        } else {
            format!(
                "{} are here. Bring another in with @, like @Checker.",
                super::and_list(&names)
            )
        };
        div()
            .w_full()
            .pt(px(8.))
            .pb(px(16.))
            .mb(px(4.))
            .flex()
            .flex_col()
            .gap(px(8.))
            // No rule under the intro: the first day's line follows it.
            .child(
                div()
                    .text_size(px(24.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(ui::ink()))
                    .child(format!("#{name}")),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .text_color(c(ui::muted()))
                    .child(who),
            )
            .when(empty, |d| {
                d.child(
                    div()
                        .mt(px(6.))
                        .text_size(px(13.5))
                        .text_color(c(ui::faint()))
                        .child(
                            "Nothing here yet. Ask for something below, or type / for commands.",
                        ),
                )
            })
            .when(total > 30, |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(c(ui::faint()))
                        .child(format!(
                            "Showing the last 30 of {total}. All jobs has the rest."
                        )),
                )
            })
    }

    /// A job: your ask, then the bot's reply with a live card.
    pub(super) fn job_entry(&mut self, j: &Job, cx: &mut Context<Self>) -> Div {
        let running = j.status == Status::Running;
        let face = if running {
            self.bot(j.bot, Mood::Working, 30., Framing::Bust)
        } else {
            // The face matches the job's news: pleased at good news,
            // concerned only when something's wrong.
            self.face(j.bot, super::job_mood(j), 30.)
        };
        let (word, tone) = status_of(j);
        let frac = j.progress().map(|(d, n)| d as f32 / n.max(1) as f32);
        let said = match j.status {
            Status::Running => match j.progress() {
                Some((_, n)) if n > 1 => format!("On it. {n} steps, I'll post when it's done."),
                _ => "On it. I'll post when it's done.".to_string(),
            },
            Status::Review => "Done. The changes wait for your OK.".to_string(),
            Status::Done => j
                .last_words()
                .map(|w| w.lines().next().unwrap_or("").to_string())
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| "Done.".into()),
            Status::Failed => "This one failed. The reason is in the job.".to_string(),
            Status::Shipping => "Approved. Shipper is opening the pull request.".to_string(),
            Status::Shipped => j.shipped.clone(),
            _ => j.status.label().to_string(),
        };
        let said: String = said.chars().take(220).collect();

        // The last few steps it took, as a terminal tail while it runs.
        let tail: Vec<(u8, String, String)> = j
            .feed
            .iter()
            .filter_map(|i| match i {
                Item::Tool {
                    label, state, kind, ..
                } => Some((*state, label.clone(), kind.clone())),
                _ => None,
            })
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let added: usize = j.changes.iter().map(|c| c.added).sum();
        let removed: usize = j.changes.iter().map(|c| c.removed).sum();
        let review = j.status == Status::Review;
        // A race shows as one card: you pick the winner side by side, not here.
        let tries = if j.race != 0 {
            self.tries(j.race).len()
        } else {
            0
        };
        let racing = tries > 1;
        let asks = self.ask_card(j, cx);
        let branch = self.branch_line(j);
        let belt = self
            .belt_card(j, cx)
            .or_else(|| self.in_the_way_card(j, cx));
        let clash = self.clash_card(j, cx);
        // A job that's over and left nothing to look at (no changes, nothing
        // asked, no git news) is one slim line, not a two-part card.
        let quiet = !running
            && !review
            && !racing
            && j.status != Status::Shipping
            && j.changes.is_empty()
            && asks.is_none()
            && branch.is_none()
            && belt.is_none()
            && clash.is_none();
        let word_said = word.clone();
        let trouble = (!racing && !running && j.status != Status::Review)
            .then(|| self.trouble_card(j, cx))
            .flatten();
        let troubled = trouble.is_some();
        // The card says what went wrong; above it, only the bot's own words.
        let said = if troubled {
            j.last_words()
                .map(|w| w.lines().next().unwrap_or("").chars().take(220).collect())
                .unwrap_or_default()
        } else {
            said
        };
        let body = if racing {
            self.race_card(j.race, cx)
        } else if let Some(card) = trouble {
            card.ml(px(0.))
        } else if quiet {
            self.job_line(j, &word, tone.0, cx)
        } else {
            let bits = JobBits {
                word,
                tone: tone.0,
                frac,
                tail,
                asks,
                belt,
                racing,
                tries,
                added,
                removed,
                said: (!running && said != word_said).then(|| said.clone()),
            };
            self.live_card(j, bits, cx)
        };

        // Your ask, then Peek's hand-off when it picked the bot, then the
        // bot's reply; each leaves out the face and name when it carries on
        // a run from the same author.
        // A bot asked for this one: its message is in the thread as its own,
        // so nothing shows as yours.
        let asked_by_you = j.asked_by().is_none();
        let you_lead = asked_by_you && !self.continues("You", j.started);
        let handoff = (!j.handoff.is_empty()).then(|| {
            self.streak = Some(("Peek".into(), j.started));
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .py(px(4.))
                .pl(px(46.))
                .child(self.avatar(Member::Fin, 18.))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child("Peek"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .child(j.handoff.clone()),
                )
        });
        // A race is Peek's to run: it speaks for the tries.
        let speaker = if racing { Member::Fin } else { j.bot };
        let face = if racing {
            self.avatar(Member::Fin, 30.).into_any_element()
        } else {
            face.into_any_element()
        };
        let said = if racing {
            format!(
                "{tries} tries are on it, each on its own branch. I'll point out the one I'd pick when they finish."
            )
        } else {
            said
        };
        let lead = !self.continues(speaker.name(), j.started);
        let head = lead.then(|| (face, byline(speaker.name(), j.started)));
        let words = div()
            .flex()
            .flex_col()
            // A card carries the bot's words in its foot; a slim line has none.
            .when(
                (quiet || racing || troubled) && said != word_said && !said.is_empty(),
                |d| {
                    d.child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .text_color(c(ui::body()))
                            .child(said),
                    )
                },
            )
            .child(div().when(running, |d| d.mt(px(4.))).child(body))
            .children(self.job_runs(j.id, cx));
        let yours = asked_by_you.then(|| {
            self.message(
                format!("you-job-{}", j.id),
                j.title.clone(),
                Msg::Mine(j.title.clone()),
                (!you_lead).then_some(j.started),
                mine(j.title.clone(), j.started, you_lead),
                cx,
            )
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .children(yours)
            .children(handoff)
            .child(self.message(
                format!("job-{}", j.id),
                j.label().to_string(),
                Msg::Job(j.id, j.status, j.bot, j.label().to_string()),
                (!lead).then_some(j.started),
                line(head, words),
                cx,
            ))
    }

    /// A finished job with nothing to look at, as one line under the bot's
    /// words: how it ended, what it was, and where it ran. Click it to open it.
    fn job_line(&mut self, j: &Job, word: &str, tone: u32, cx: &mut Context<Self>) -> Div {
        let id = j.id;
        let again = matches!(
            j.status,
            Status::Failed | Status::Interrupted | Status::Stopped
        );
        div().mt(px(6.)).child(
            div()
                .id(("job-line", id as usize))
                .h(px(34.))
                .pl(px(12.))
                .pr(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(9.))
                .bg(c(ui::raised()))
                .border_1()
                .border_color(c(ui::line()))
                .cursor_pointer()
                .hover(|s| s.border_color(c(ui::line_hi())).bg(c(ui::hover())))
                .tooltip(tip_of("Peek at the job".into()))
                .on_click(cx.listener(move |this, _, _, cx| this.peek_job(id, cx)))
                .child(div().size(px(7.)).flex_none().rounded_full().bg(c(tone)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::ink()))
                        .child(card_title(j)),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.5))
                        .text_color(c(ui::faint()))
                        .child(format!("#{id} \u{b7} {}", j.engine)),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(tone))
                        .child(word.to_string()),
                )
                .when(again, |d| {
                    d.child(
                        ui::ghost(("thread-retry", id as usize), "Try again")
                            .h(px(24.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.retry_job(id, cx);
                            })),
                    )
                })
                .child(
                    Icon::new(IconName::ChevronRight)
                        .size(px(14.))
                        .text_color(c(ui::faint())),
                ),
        )
    }

    /// A job's card in the thread, as the design draws it: the title, its
    /// number and how it stands; a dashed bar while it runs; then one line
    /// with what it's doing or did, the lines it changed and a Seatbelt mark,
    /// with the buttons that matter now. The rest (the seatbelt's catch, a
    /// clash, checks, Approve) is in the peek beside it. A question the bot
    /// asks you stays right here.
    fn live_card(&mut self, j: &Job, b: JobBits, cx: &mut Context<Self>) -> Div {
        let id = j.id;
        let running = j.status == Status::Running;
        let JobBits {
            word,
            tone,
            frac,
            tail,
            racing,
            tries,
            added,
            removed,
            said,
            ..
        } = b;
        let held = b.belt.is_some();
        let (state, col) = if running {
            let what = match j.progress() {
                Some((d, n)) => format!("Working \u{b7} {}/{n}", (d + 1).min(n)),
                None => "Working".to_string(),
            };
            (what, ui::t_blue())
        } else {
            (word, tone)
        };
        let head = div()
            .px(px(14.))
            .pt(px(11.))
            .pb(px(4.))
            .flex()
            .items_center()
            .gap(px(9.))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(card_title(j)),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child(format!("#{id}")),
            )
            .child(div().flex_1())
            .child(ui::mono_status(col, &state));

        let peeked = self.reply_open == Some(id);
        let mut body = card()
            .id(("job-card", id as usize))
            .cursor_pointer()
            .rounded(px(6.))
            .when(peeked, |d| {
                d.border_color(c(ui::accent_bar())).shadow(vec![BoxShadow {
                    color: ca(ui::pick_fill(), 0.35),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(3.),
                    inset: false,
                }])
            })
            .on_click(cx.listener(move |this, _, _, cx| this.peek_job(id, cx)))
            .child(head);
        if running {
            body = body.child(
                div()
                    .mx(px(14.))
                    .mt(px(2.))
                    .child(dashed_bar(frac.unwrap_or(0.), ui::t_blue())),
            );
        }
        let line = if running {
            match tail.last() {
                Some((0, label, _)) => store::plain_step(label, true),
                _ if !j.plan.is_empty() => j.doing(),
                _ => "Thinking it through".to_string(),
            }
        } else if racing {
            format!("Race of {tries} engines")
        } else {
            said.map(|w| w.lines().next().unwrap_or("").to_string())
                .unwrap_or_else(|| format!("{} \u{b7} {}", j.bot.name(), j.engine))
        };
        let mut foot = div()
            .px(px(14.))
            .pt(px(if running { 8. } else { 4. }))
            .pb(px(12.))
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(12.5))
            .text_color(c(ui::muted()))
            .child(div().min_w_0().truncate().child(line))
            .when(added + removed > 0, |d| {
                d.child(
                    div()
                        .flex_none()
                        .font_family(ui::MONO)
                        .text_size(px(12.))
                        .text_color(c(ui::t_green()))
                        .child(format!("+{added}")),
                )
                .child(
                    div()
                        .flex_none()
                        .font_family(ui::MONO)
                        .text_size(px(12.))
                        .text_color(c(ui::t_red()))
                        .child(format!("\u{2212}{removed}")),
                )
            })
            .when(held, |d| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::t_red()))
                        .child(Icon::new(IconName::Shield).size(px(13.)))
                        .child("SEATBELT"),
                )
            })
            .child(div().flex_1());
        let btn = |id: (&'static str, usize), label: &'static str| {
            ui::ghost(id, label).h(px(28.)).text_size(px(12.5))
        };
        if racing {
            let race = j.race;
            foot = foot.child(
                ui::solid(
                    ("thread-compare", id as usize),
                    format!("Compare {tries} tries"),
                )
                .h(px(28.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.go(Place::Race(race), cx)
                })),
            );
        }
        if running && !j.remote_on.is_empty() {
            foot = foot.child(btn(("thread-browser", id as usize), "Browser").on_click(
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.go(Place::Browser(id), cx)
                }),
            ));
        }
        if running {
            foot = foot.child(
                btn(("thread-stop", id as usize), "Stop").on_click(cx.listener(
                    move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.stop_job(id, cx)
                    },
                )),
            );
        }
        if matches!(
            j.status,
            Status::Failed | Status::Interrupted | Status::Stopped
        ) {
            foot = foot.child(btn(("thread-retry", id as usize), "Try again").on_click(
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.retry_job(id, cx)
                }),
            ));
        }
        body = body.child(foot);
        // A question for you is answered here, in the thread.
        div().w_full().child(body.children(b.asks))
    }

    /// A plan: your goal, then Peek's plan card with a step per job.
    fn plan_entry(&mut self, g: &Goal, cx: &mut Context<Self>) -> Div {
        let id = g.id;
        let mood = match g.state {
            State::Planning => Mood::Thinking,
            // A plan ready to start is good news, not a worry.
            State::Ready | State::Done => Mood::Done,
            State::Paused | State::Failed => Mood::NeedsYou,
            State::Running => Mood::Working,
        };
        // Drawn still: an eased face here shared its slot with Peek on the
        // rail, which is the same size, so the two moods swapped every frame
        // and the face flickered.
        let face = self.face(Member::Fin, mood, 30.);
        let said = match g.state {
            State::Planning => "Reading the code to plan this.".to_string(),
            State::Ready => "Here's my plan. Start it when it looks right.".to_string(),
            State::Running => "Here's the plan. The crew is on it.".to_string(),
            State::Paused => "A step stopped. Look at it, then carry on or skip it.".to_string(),
            State::Done => "All done.".to_string(),
            State::Failed => g.note.clone(),
        };
        let jobs = self.crew.store.jobs.clone();
        let done = g
            .steps
            .iter()
            .filter(|s| {
                s.job != 0
                    && jobs.iter().any(|j| {
                        j.id == s.job && matches!(j.status, Status::Done | Status::Shipped)
                    })
            })
            .count();
        let n = g.steps.iter().filter(|s| !s.skipped).count();

        let mut steps = div().px(px(14.)).py(px(4.)).flex().flex_col();
        for (k, s) in g.steps.iter().enumerate() {
            let job = jobs.iter().find(|j| j.id == s.job && s.job != 0);
            let bot = job
                .map(|j| j.bot)
                .unwrap_or_else(|| Member::doer_for(s.mode).unwrap_or(Member::for_mode(s.mode)));
            let (mark, fill, edge) = match job.map(|j| j.status) {
                _ if s.skipped => ("", ui::raised(), ui::line_hi()),
                Some(Status::Done | Status::Shipped) => ("\u{2713}", ui::t_green(), ui::t_green()),
                Some(Status::Running | Status::Shipping) => ("", ui::BLUE, ui::BLUE),
                Some(Status::Review) => ("!", ui::AMBER, ui::AMBER),
                Some(Status::Failed) => ("\u{2717}", ui::t_red(), ui::t_red()),
                _ => ("", ui::raised(), ui::line_hi()),
            };
            let face = self.avatar(bot, 18.);
            let jid = s.job;
            steps = steps.child(
                div()
                    .id(("plan-step", (id * 100 + k as u64) as usize))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .py(px(7.))
                    .when(k > 0, |d| d.border_t_1().border_color(c(ui::line2())))
                    .when(jid != 0, |d| {
                        d.cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| this.peek_job(jid, cx)))
                    })
                    .child(
                        div()
                            .size(px(17.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .bg(c(fill))
                            .border_1()
                            .border_color(c(edge))
                            .text_size(px(11.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(0xFFFFFF))
                            .child(mark),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.5))
                            .text_color(c(if s.skipped { ui::faint() } else { ui::body() }))
                            .when(s.skipped, |d| d.line_through())
                            .child(s.title.clone()),
                    )
                    .child(face)
                    .child(
                        div()
                            .w(px(70.))
                            .flex_none()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child(bot.name()),
                    ),
            );
        }
        let mut card = card()
            .when(g.state == State::Ready, |d| {
                d.border_color(c(ui::ask_tone().2))
            })
            .child(
                div()
                    .px(px(14.))
                    .py(px(11.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .bg(c(super::theme::mix(ui::raised(), ui::VIOLET, 0.07)))
                    .child(
                        Icon::new(IconName::Sparkles)
                            .size(px(15.))
                            .text_color(c(ui::t_violet())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(format!(
                                "{} \u{b7} {} {}",
                                g.text,
                                n,
                                if n == 1 { "step" } else { "steps" }
                            )),
                    )
                    .when(
                        matches!(g.state, State::Running | State::Paused | State::Done),
                        |d| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(c(ui::muted()))
                                    .child(format!("{done} of {n} done")),
                            )
                        },
                    ),
            );
        if !g.steps.is_empty() {
            card = card.child(steps);
        }
        let footer = match g.state {
            State::Ready => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child("Nothing starts until you say so."),
                    )
                    .child(
                        ui::ghost(("plan-drop", id as usize), "Discard")
                            .h(px(28.))
                            .on_click(cx.listener(move |this, _, _, cx| this.drop_goal(id, cx))),
                    )
                    .child(
                        ui::ghost(("plan-edit", id as usize), "Edit plan")
                            .h(px(28.))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.go(Place::Goal(id), cx)),
                            ),
                    )
                    .child(
                        ui::solid(
                            ("plan-start", id as usize),
                            match g.steps.len() {
                                1 => "Start the job".to_string(),
                                n => format!("Start {n} jobs"),
                            },
                        )
                        .h(px(28.))
                        .on_click(cx.listener(move |this, _, _, cx| this.start_goal(id, cx))),
                    ),
            ),
            State::Planning => None,
            _ => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child(format!("Planned {}", when(g.started))),
                    )
                    .child(
                        ui::ghost(("plan-open", id as usize), "Open plan")
                            .h(px(28.))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.go(Place::Goal(id), cx)),
                            ),
                    ),
            ),
        };
        if let Some(f) = footer {
            card = card.child(
                f.px(px(14.))
                    .py(px(9.))
                    .bg(c(ui::well()))
                    .border_t_1()
                    .border_color(c(ui::line2())),
            );
        }
        let you_lead = !self.continues("You", g.started);
        let lead = !self.continues("Peek", g.started);
        let head = lead.then(|| (face.into_any_element(), byline("Peek", g.started)));
        let words = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .text_color(c(ui::body()))
                    .child(said),
            )
            .child(card);
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(self.message(
                format!("you-plan-{}", g.id),
                g.text.clone(),
                Msg::Mine(g.text.clone()),
                (!you_lead).then_some(g.started),
                mine(g.text.clone(), g.started, you_lead),
                cx,
            ))
            .child(self.message(
                format!("plan-{}", g.id),
                g.text.clone(),
                Msg::Plan(g.id, g.state == State::Ready),
                (!lead).then_some(g.started),
                line(head, words),
                cx,
            ))
    }

    /// While the composer starts with `/`: the commands that match, to click.
    /// Threads to point at while you type #: pick one and its name goes in.
    /// The bots an "@bu" being typed could be: the text before it, and the matches.
    pub(super) fn at_matches(&self, text: &str) -> Option<(String, Vec<Member>)> {
        if text.ends_with(char::is_whitespace) {
            return None;
        }
        let last = text.split_whitespace().last()?;
        let typed = last.strip_prefix('@')?.to_lowercase();
        let found: Vec<Member> = Member::all()
            .into_iter()
            .filter(|m| !self.crew.store.removed.contains(m))
            .filter(|m| m.name().to_lowercase().starts_with(&typed))
            .take(6)
            .collect();
        if found.is_empty() {
            return None;
        }
        Some((text[..text.len() - last.len()].to_string(), found))
    }

    /// "@" in the box: the crew to pick from, each with its face and job, and
    /// the bots already named, highlighted as they'll be brought in.
    fn at_strip(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let text = self.composer.read(cx).value().to_string();
        if let Some((head, found)) = self.at_matches(&text) {
            let mut list = div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pb(px(8.))
                .border_b_1()
                .border_color(c(ui::line()))
                .child(
                    div()
                        .px(px(4.))
                        .pb(px(4.))
                        .flex()
                        .items_center()
                        .text_size(px(11.5))
                        .text_color(c(ui::faint()))
                        .child(div().flex_1().child("Bring in a bot"))
                        .child("\u{21b5} picks the first"),
                );
            for (i, m) in found.into_iter().enumerate() {
                let head = head.clone();
                let face = self.avatar(m, 22.);
                let accent = m.accent();
                list = list.child(
                    div()
                        .id(("at", i))
                        .h(px(34.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .rounded(px(8.))
                        .when(i == 0, |d| {
                            d.bg(c(super::theme::mix(ui::raised(), accent, 0.12)))
                        })
                        .cursor_pointer()
                        .hover(|s| s.bg(c(super::theme::mix(ui::raised(), accent, 0.08))))
                        .child(face)
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(ui::ink()))
                                .child(m.name()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(c(ui::muted()))
                                .child(m.role().to_string()),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let text = format!("{head}@{} ", m.name());
                            this.composer.update(cx, |i, cx| {
                                ui::put_text(i, text, window, cx);
                                i.focus(window, cx);
                            });
                            cx.notify();
                        })),
                );
            }
            return Some(list);
        }
        // Bots named in full: shown as who's being brought in.
        let named: Vec<Member> = Member::all()
            .into_iter()
            .filter(|m| {
                text.split_whitespace().any(|w| {
                    w.strip_prefix('@').is_some_and(|n| {
                        n.trim_end_matches(|ch: char| !ch.is_alphanumeric())
                            .eq_ignore_ascii_case(m.name())
                    })
                })
            })
            .collect();
        if named.is_empty() {
            return None;
        }
        let mut row = div().flex().flex_wrap().items_center().gap(px(6.)).child(
            div()
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child("Brings in"),
        );
        for m in named {
            let accent = m.accent();
            row = row.child(
                div()
                    .h(px(24.))
                    .pl(px(4.))
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(7.))
                    .bg(c(super::theme::mix(ui::raised(), accent, 0.14)))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(super::theme::mix(accent, ui::ink(), 0.35)))
                    .child(self.avatar(m, 16.))
                    .child(format!("@{}", m.name())),
            );
        }
        Some(row)
    }

    fn hash_strip(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let text = self.composer.read(cx).value().to_string();
        if text.ends_with(char::is_whitespace) {
            return None;
        }
        let last = text.split_whitespace().last()?.to_string();
        let typed = last.strip_prefix('#')?.to_lowercase();
        let folder = self.crew.store.folder()?.path.clone();
        let mut names: Vec<String> = vec!["general".into()];
        names.extend(
            self.crew
                .store
                .threads_in(&folder)
                .into_iter()
                .map(|t| t.name.clone()),
        );
        let matches: Vec<String> = names
            .into_iter()
            .filter(|n| n.starts_with(&typed))
            .take(6)
            .collect();
        if matches.is_empty() {
            return None;
        }
        let head = text[..text.len() - last.len()].to_string();
        let mut row = div().flex().flex_wrap().gap(px(6.));
        for (i, n) in matches.into_iter().enumerate() {
            let head = head.clone();
            row = row.child(
                div()
                    .id(("hash", i))
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(c(ui::line()))
                    .bg(c(ui::raised()))
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(ui::line_hi())))
                    .child(div().text_color(c(ui::faint())).child("#"))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(ui::ink()))
                            .child(n.clone()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let text = format!("{head}#{n} ");
                        this.composer.update(cx, |i, cx| {
                            ui::put_text(i, text, window, cx);
                            i.focus(window, cx);
                        });
                        cx.notify();
                    })),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(c(ui::faint()))
                        .child("Point at a thread and Peek reads it. Send just #name to go there."),
                )
                .child(row),
        )
    }

    /// The threads a message points at, as chips that take you there.
    fn ref_chips(&mut self, key: u64, text: &str, cx: &mut Context<Self>) -> Option<Div> {
        let folder = self.crew.store.folder()?.path.clone();
        let here = self.crew.store.thread_here();
        let refs: Vec<u64> = self
            .crew
            .store
            .thread_refs(&folder, text)
            .into_iter()
            .filter(|t| *t != here)
            .collect();
        if refs.is_empty() {
            return None;
        }
        let mut row = div()
            .pl(px(46.))
            .pb(px(2.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child("Read from"),
            );
        for t in refs {
            let name = self.crew.store.thread_label(t);
            row = row.child(
                div()
                    .id(SharedString::from(format!("ref-{key}-{t}")))
                    .h(px(22.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .bg(c(super::theme::mix(ui::raised(), ui::VIOLET, 0.08)))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(ui::t_violet()))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(super::theme::mix(ui::raised(), ui::VIOLET, 0.16))))
                    .child(format!("#{name}"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.crew.store.thread_now = t;
                        this.crew.store.save();
                        this.go(Place::Today, cx);
                    })),
            );
        }
        Some(row)
    }

    fn slash_strip(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let text = self.composer.read(cx).value().to_string();
        let typed = text.trim_start();
        if !typed.starts_with('/') || typed.contains(' ') {
            return None;
        }
        let matches: Vec<(&str, &str)> = COMMANDS
            .iter()
            .copied()
            .filter(|(cmd, _)| cmd.starts_with(typed))
            .collect();
        if matches.is_empty() {
            return None;
        }
        let mut row = div().flex().flex_wrap().gap(px(6.));
        for (i, (cmd, what)) in matches.into_iter().enumerate() {
            row = row.child(
                div()
                    .id(("slash", i))
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .map(|d| {
                        if i == 0 {
                            d.bg(c(super::theme::mix(ui::raised(), ui::VIOLET, 0.08)))
                                .border_color(c(super::theme::mix(ui::raised(), ui::VIOLET, 0.3)))
                        } else {
                            d.bg(c(ui::raised())).border_color(c(ui::line()))
                        }
                    })
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(ui::line_hi())))
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(12.5))
                            .text_color(c(if i == 0 { ui::t_violet() } else { ui::ink() }))
                            .child(cmd),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(ui::muted()))
                            .child(what),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let text = format!("{cmd} ");
                        this.composer.update(cx, |i, cx| {
                            ui::put_text(i, text, window, cx);
                            i.focus(window, cx);
                        });
                        cx.notify();
                    })),
            );
        }
        Some(row)
    }

    /// The box at the bottom of a thread.
    fn thread_composer(&mut self, _thread: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.composer.read(cx).value().to_string();
        if !self.mode_pinned && !value.starts_with('/') {
            self.mode = Mode::guess(&value);
        }
        let named = super::mentioned(&value).map(|(m, _)| m);
        // Nobody named: Peek reads it and picks the bot, so no bot is
        // guessed here ahead of it.
        let peek_picks = named.is_none()
            && self.target.is_none()
            && !self.mode_pinned
            && !value.starts_with('/');
        let target = if let Some(m) = named {
            m
        } else if peek_picks || self.mode == Mode::Ask {
            self.ask_who()
        } else {
            self.doer()
        };
        let ready = !value.trim().is_empty();
        let face = self.avatar(target, 15.);
        let plan_first = cx.global::<crate::settings::AppSettings>().v2_plan_first;
        let goes = if let Some(m) = named.filter(|_| !value.starts_with('/')) {
            match self.open_here(m) {
                Some(id) => format!("{} adds it to #{id}", m.name()),
                None => format!("{} takes it", m.name()),
            }
        } else if peek_picks {
            "Peek picks the bot".to_string()
        } else if value.starts_with("/thread") {
            "Makes a new thread".to_string()
        } else if value.starts_with("/rename") {
            "Renames this thread".to_string()
        } else if value.starts_with("/note") {
            "Pins a note to this thread".to_string()
        } else if value.starts_with("/archive") {
            "Archives this thread".to_string()
        } else if value.starts_with("/delete") {
            "Deletes this thread".to_string()
        } else if value.starts_with("/ask") || (self.mode == Mode::Ask && !value.starts_with('/')) {
            format!("{} answers here", self.ask_who().name())
        } else if value.starts_with("/later")
            || (self.later_pick.is_some() && !value.starts_with('/'))
        {
            let typed = match &self.later_pick {
                Some(when) if !value.starts_with('/') => format!("{when} {value}"),
                _ => value.trim_start_matches("/later").trim().to_string(),
            };
            match super::shift::parse_when(&typed, chrono::Local::now()) {
                Some((at, rest)) => {
                    // The bot /later hands it to, as hand_over picks it.
                    let mode = match Mode::guess(&rest) {
                        m if m.writes() => m,
                        _ => Mode::Build,
                    };
                    let who = Member::doer_for(mode).unwrap_or(Member::for_mode(mode));
                    format!(
                        "{} starts it {}",
                        who.name(),
                        super::shift::when_words(at, store::now())
                    )
                }
                None => "Say when: tonight, 2am, in 2h, tomorrow".to_string(),
            }
        } else if value.starts_with("/race") {
            // A race runs as asked, plan-first or not, as hand_over does.
            format!("{} races it", target.name())
        } else if (value.starts_with("/plan") || plan_first || self.splitting) && self.mode.writes()
        {
            "Peek plans it first".to_string()
        } else {
            format!("{} \u{b7} {}", target.name(), self.mode.label())
        };
        // How it's sent, as the design's chips: Ask answers, Do starts a job,
        // Plan has Peek plan it first, Race tries it on two engines.
        let how = if value.starts_with("/race") {
            "Race"
        } else if value.starts_with("/plan") {
            "Plan"
        } else if value.starts_with("/ask")
            || (self.mode == Mode::Ask && (self.mode_pinned || !value.trim().is_empty()))
        {
            "Ask"
        } else {
            "Do"
        };
        let mut hows = div().flex().items_center().gap(px(2.));
        for (i, name) in ["Ask", "Do", "Plan", "Race"].into_iter().enumerate() {
            let on = how == name;
            hows = hows.child(
                div()
                    .id(("compose-how", i))
                    .h(px(26.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .rounded(px(3.))
                    .text_size(px(12.))
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
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_how(name, window, cx);
                    })),
            );
        }
        let sep = || div().w(px(1.)).h(px(16.)).mx(px(2.)).bg(c(ui::line()));
        // The chip says who gets it; what that means in full is its tip.
        let chip = if value.starts_with('/') {
            goes.clone()
        } else {
            format!("To {}", target.name())
        };
        let goes_tip =
            format!("{goes}\n/ for commands \u{b7} @ to bring in a bot \u{b7} # for a thread");
        // Where it runs: the server switch once there is one, else this Mac.
        let run_on = self
            .run_on_pill(cx)
            .map(|p| p.into_any_element())
            .unwrap_or_else(|| {
                div()
                    .h(px(26.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(12.))
                    .text_color(c(ui::muted()))
                    .child(Icon::new(IconName::Server).size(px(13.)))
                    .child(super::machines::here_name())
                    .into_any_element()
            });
        div()
            .w_full()
            .flex()
            .flex_col()
            .rounded(px(7.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line_hi()))
            .children(self.at_strip(cx))
            .child(
                div()
                    .px(px(16.))
                    .pt(px(13.))
                    .pb(px(6.))
                    .min_h(px(44.))
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .child(
                        Textarea::new(&self.composer)
                            .appearance(false)
                            .bordered(false),
                    ),
            )
            .child(
                div()
                    .pl(px(10.))
                    .pr(px(8.))
                    .pt(px(6.))
                    .pb(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("thread-goes-to")
                            .h(px(26.))
                            .px(px(9.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(5.))
                            .rounded(px(3.))
                            .bg(c(ui::hover()))
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(ui::ink()))
                            .child(face)
                            .child(chip)
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(goes_tip.clone())
                                    .build(window, cx)
                            }),
                    )
                    .child(sep())
                    .child(hows)
                    // Where a job from here runs.
                    .when(how != "Ask" && !value.starts_with('/'), |d| {
                        d.child(sep()).child(run_on)
                    })
                    .child(div().flex_1())
                    .child(self.later_button(cx))
                    // Send, split like the design: the arrow sends now, the
                    // chevron runs it later.
                    .child(
                        div()
                            .h(px(30.))
                            .flex()
                            .flex_none()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(c(ui::pick_fill()))
                            .child(
                                div()
                                    .id("thread-send")
                                    .w(px(32.))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .map(|d| {
                                        if ready {
                                            d.cursor_pointer().hover(|s| s.bg(c(ui::fill_hover())))
                                        } else {
                                            d.opacity(0.5)
                                        }
                                    })
                                    .child(
                                        Icon::new(IconName::ArrowUp)
                                            .size(px(15.))
                                            .text_color(c(ui::pick_on())),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.send_from_composer(window, cx);
                                    })),
                            )
                            .child(div().w(px(1.)).h_full().bg(ca(ui::pick_on(), 0.2)))
                            .child(
                                div()
                                    .id("thread-later")
                                    .w(px(20.))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(c(ui::fill_hover())))
                                    .child(
                                        Icon::new(IconName::ChevronDown)
                                            .size(px(11.))
                                            .text_color(c(ui::pick_on())),
                                    )
                                    .tooltip(|window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new("Run it later")
                                            .build(window, cx)
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.later_menu = !this.later_menu;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }

    /// One of the composer's Ask / Do / Plan / Race chips: Ask and Do set
    /// how it goes, Plan and Race put their command at the front.
    fn pick_how(&mut self, how: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().to_string();
        let mut body = text.trim_start();
        for cmd in ["/plan", "/race", "/ask"] {
            if let Some(rest) = body.strip_prefix(cmd) {
                body = rest.trim_start();
            }
        }
        let body = body.to_string();
        let next = match how {
            "Plan" => format!("/plan {body}"),
            "Race" => format!("/race {body}"),
            _ => body,
        };
        if next != text {
            self.composer.update(cx, |i, cx| {
                ui::put_text(i, next, window, cx);
            });
        }
        self.set_mode(
            if how == "Ask" { Mode::Ask } else { Mode::Build },
            window,
            cx,
        );
        self.composer.update(cx, |i, cx| i.focus(window, cx));
    }

    /// Run it later: a clock by the send button with the times a job can wait
    /// for. Picking one sends the next job to the night shift.
    fn later_button(&mut self, cx: &mut Context<Self>) -> Div {
        let mut times: Vec<(String, &str, &str)> = vec![
            ("tonight".into(), "Tonight", "11 PM, while you sleep"),
            (
                "tomorrow".into(),
                "Tomorrow morning",
                "9 AM, before you start",
            ),
            ("in 2h".into(), "In 2 hours", "When you're back"),
        ];
        // When the plan the crew runs on starts over, if the engine said.
        let now_s = store::now() as i64;
        if let Some(reset) = crate::usage::get("claude")
            .as_ref()
            .and_then(|u| u.headline().and_then(|w| w.resets_at))
            .filter(|r| *r > now_s + 60)
        {
            let mins = ((reset - now_s) / 60 + 1).min(72 * 60);
            times.push((
                format!("in {mins}m"),
                "When the plan resets",
                "Its hours start over, so it runs in full",
            ));
        }
        let picked = self.later_pick.clone();
        let open = self.later_menu;
        let mut menu = div()
            .absolute()
            .bottom(px(38.))
            .right(px(0.))
            .w(px(330.))
            .p(px(6.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(c(ui::float()))
            .border_1()
            .border_color(c(ui::line_hi()))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.12),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .px(px(8.))
                    .pt(px(4.))
                    .pb(px(6.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::faint()))
                    .child("RUN IT LATER"),
            );
        for (i, (when, name, line)) in times.into_iter().enumerate() {
            let on = picked.as_deref() == Some(when.as_str());
            let at = super::shift::parse_when(&format!("{when} x"), chrono::Local::now())
                .map(|(at, _)| super::shift::when_words(at, store::now()))
                .unwrap_or_default();
            menu = menu.child(
                div()
                    .id(("later-pick", i))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .when(on, |d| d.bg(c(ui::hover())))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::ink()))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(c(ui::muted()))
                                    .child(line),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .font_family(ui::MONO)
                            .text_color(c(ui::faint()))
                            .child(at),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.later_pick = if on { None } else { Some(when.clone()) };
                        this.later_menu = false;
                        cx.notify();
                    })),
            );
        }
        menu = menu.child(
            div()
                .px(px(8.))
                .pt(px(6.))
                .mt(px(4.))
                .border_t_1()
                .border_color(c(ui::line2()))
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child("Or type /later 2am, /later in 3h …"),
        );
        let label = picked.map(|w| match w.as_str() {
            "tonight" => "Tonight".to_string(),
            "tomorrow" => "Tomorrow".to_string(),
            "in 2h" => "In 2h".to_string(),
            _ => "When it resets".to_string(),
        });
        div()
            .relative()
            .flex()
            .items_center()
            .h_full()
            .when_some(label, |d, label| {
                d.child(
                    div()
                        .id("thread-later-pick")
                        .h(px(26.))
                        .mr(px(6.))
                        .px(px(9.))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .rounded(px(3.))
                        .bg(c(ui::hover()))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::ink()))
                        .cursor_pointer()
                        .child(Icon::new(IconName::Moon).size(px(13.)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.later_menu = !open;
                            cx.notify();
                        })),
                )
            })
            .when(open, |d| d.child(deferred(menu).with_priority(2)))
    }

    /// Sends what's in the composer to the thread you're in.
    pub(super) fn send_from_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.composer
            .update(cx, |i, cx| i.set_value("", window, cx));
        // Run it later: the job goes to the night shift for the time picked.
        self.later_menu = false;
        if let Some(when) = self.later_pick.take() {
            if !text.starts_with('/') {
                text = format!("/later {when} {text}");
            }
        }
        self.hand_over(text, cx);
        self.unpin_mode(window, cx);
    }
}

/// A card's title: Peek's short one, else what you typed, unless that reads as
/// keyboard mash, when it's named for the kind of job instead.
pub(super) fn card_title(j: &Job) -> String {
    let t = j.label().trim().to_string();
    if !mashed(&t) {
        return t;
    }
    format!("{} job #{}", j.mode.label(), j.id)
}

/// Whether text looks like random keys: one long word, or almost no vowels.
fn mashed(t: &str) -> bool {
    let letters: Vec<char> = t.chars().filter(|ch| ch.is_alphabetic()).collect();
    if letters.len() < 6 {
        return false;
    }
    let vowels = letters
        .iter()
        .filter(|ch| "aeiouyAEIOUY".contains(**ch))
        .count();
    let longest = t
        .split_whitespace()
        .map(|w| w.chars().count())
        .max()
        .unwrap_or(0);
    longest > 18 || vowels * 6 < letters.len()
}

/// What a job's card is drawn from, worked out once in `job_entry`.
struct JobBits {
    word: String,
    tone: u32,
    frac: Option<f32>,
    tail: Vec<(u8, String, String)>,
    asks: Option<Div>,
    belt: Option<Div>,
    racing: bool,
    tries: usize,
    added: usize,
    removed: usize,
    /// What the bot said about it, carried in the card's foot: the card is
    /// the message, not a message and then a card saying the same.
    said: Option<String>,
}

/// The catch-up's words: "2 finished, 1 ready for you, nothing failed".
/// Nothing when nothing ended.
fn catch_up_words(done: usize, review: usize, failed: usize) -> Option<String> {
    if done + review + failed == 0 {
        return None;
    }
    let mut parts = vec![format!("{done} finished")];
    if review > 0 {
        parts.push(format!("{review} ready for you"));
    }
    parts.push(if failed == 0 {
        "nothing failed".to_string()
    } else {
        format!("{failed} failed")
    });
    Some(parts.join(", "))
}

#[cfg(test)]
mod catch_up_tests {
    use super::catch_up_words;

    #[test]
    fn catch_up_reads_as_one_line() {
        assert_eq!(catch_up_words(0, 0, 0), None);
        assert_eq!(
            catch_up_words(2, 1, 0).as_deref(),
            Some("2 finished, 1 ready for you, nothing failed")
        );
        assert_eq!(
            catch_up_words(0, 0, 1).as_deref(),
            Some("0 finished, 1 failed")
        );
    }
}

#[cfg(test)]
mod card_tests {
    use super::mashed;

    #[test]
    fn keyboard_mash_gets_a_plain_title() {
        assert!(mashed("afadfasdgasdgasdgasd"));
        assert!(mashed("sdfgh jklqw"));
        assert!(!mashed("Add gift cards"));
        assert!(!mashed("can you build a simple node js app and run it"));
        assert!(!mashed("hi"));
    }
}

/// One job on a bot's hover card.
#[derive(Clone)]
struct CardJob {
    id: u64,
    heading: &'static str,
    title: String,
    word: String,
    tone: (u32, u32, u32),
}

impl CardJob {
    fn of(heading: &'static str, j: &Job) -> Self {
        let raw = if j.short.trim().is_empty() {
            &j.title
        } else {
            &j.short
        };
        let first = raw.lines().next().unwrap_or("").trim();
        let title = if first.chars().count() > 64 {
            format!("{}\u{2026}", first.chars().take(63).collect::<String>())
        } else {
            first.to_string()
        };
        let (word, tone) = status_of(j);
        Self {
            id: j.id,
            heading,
            title,
            word,
            tone,
        }
    }
}

/// The small card a bot's face in a thread's bar opens on hover, like a
/// profile card in Slack: its face, name and job, then links to what it's on
/// and to its chat.
#[derive(Clone)]
pub(super) struct BotCard {
    shell: WeakEntity<Shell>,
    who: Member,
    face: std::sync::Arc<RenderImage>,
    state: String,
    tone: u32,
    jobs: Vec<CardJob>,
}

impl Render for BotCard {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let who = self.who;
        let accent = who.accent();
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .size(px(44.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .bg(ca(accent, 0.1))
                    .child(img(self.face.clone()).size(px(40.))),
            )
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(ui::ink()))
                                    .child(who.name()),
                            )
                            .child(role_tag(
                                if who == Member::Fin {
                                    "Lead".to_string()
                                } else {
                                    who.role().to_string()
                                },
                                accent,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child(div().size(px(6.)).rounded_full().bg(c(self.tone)))
                            .child(self.state.clone()),
                    ),
            );
        let mut card = div()
            .id("bot-card")
            .w(px(272.))
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(px(12.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .font_family("Geist")
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.12),
                offset: point(px(0.), px(6.)),
                blur_radius: px(18.),
                spread_radius: px(0.),
                inset: false,
            }])
            // Clicks on the card stay on the card, not the bar under it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(head)
            .child(
                div()
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .text_color(c(ui::muted()))
                    .child(who.pitch()),
            );
        for (k, j) in self.jobs.iter().enumerate() {
            let shell = self.shell.clone();
            let id = j.id;
            card = card.child(
                div()
                    .id(("bot-card-job", k))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(c(ui::line()))
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(ui::line_hi())).bg(c(ui::hover())))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::faint()))
                                    .child(j.heading),
                            )
                            .child(super::home::pill(&j.word, j.tone)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.))
                                    .text_color(c(ui::ink()))
                                    .child(j.title.clone()),
                            )
                            .child(
                                Icon::new(IconName::ArrowRight)
                                    .size(px(13.))
                                    .text_color(c(ui::faint())),
                            ),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = shell.update(cx, |s, cx| s.open_job(id, cx));
                    }),
            );
        }
        let shell = self.shell.clone();
        card.child(
            ui::ghost("bot-card-chat", format!("Message {}", who.name()))
                .h(px(30.))
                .on_click(move |_, _, cx| {
                    let _ = shell.update(cx, |s, cx| s.go(Place::Chat(who), cx));
                }),
        )
    }
}
