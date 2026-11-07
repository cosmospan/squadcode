//! A # thread, Crew Chat style: your asks and the crew's replies as a chat.
//! Peek's plans arrive as plan cards you start or drop; each job arrives as a
//! card that shows its progress live, with the buttons it needs right there.
//! The composer sits at the bottom, with `/` commands.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use runo_holo::{Framing, Mood};

use super::data::{Member, Mode};
use super::goal::{Goal, State};
use super::home::status_of;
use super::store::{self, Item, Job, Status};
use super::ui::{self, c, ca};
use super::{Place, Shell};

/// The commands the composer knows, with what each does.
pub(super) const COMMANDS: [(&str, &str); 7] = [
    ("/plan", "Peek plans it first, you press Start"),
    ("/race", "Try it on two engines, keep the best"),
    ("/ask", "A question, no code changes"),
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
enum Reply {
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
    fn put_in_box(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn message(
        &mut self,
        key: String,
        copy: String,
        what: Msg,
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
            .top(px(-10.))
            .right(px(4.))
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
        div()
            .group(group)
            .relative()
            .rounded(px(10.))
            .hover(|s| s.bg(ca(ui::ink(), 0.025)))
            .child(body)
            .children(marks)
            .child(bar)
    }
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
        .my(px(6.))
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

/// A message's header: who, and when.
pub(super) fn byline(name: &str, at: u64) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::ink()))
                .child(name.to_string()),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child(when(at)),
        )
}

/// A card under a message: white, a hairline edge, a soft shadow.
fn card() -> Div {
    div()
        .mt(px(8.))
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(12.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line()))
        .overflow_hidden()
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.04),
            offset: point(px(0.), px(1.)),
            blur_radius: px(2.),
            spread_radius: px(0.),
            inset: false,
        }])
}

/// The person's own message.
pub(super) fn mine(text: String, at: u64) -> Div {
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
                .rounded(px(10.))
                .bg(c(super::theme::mix(ui::raised(), ui::VIOLET, 0.14)))
                .text_color(c(ui::t_violet()))
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .child("You".chars().next().unwrap_or('Y').to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(byline("You", at))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .text_color(c(ui::body()))
                        .child(tagged(text)),
                ),
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
                    color: Some(c(ui::t_violet())),
                    font_weight: Some(FontWeight::SEMIBOLD),
                    background_color: Some(ca(ui::VIOLET, 0.1)),
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
            self.new_mark = (seen_key, before);
        }
        let since = self.new_mark.1;
        self.crew.store.mark_seen(&folder.path, thread);
        let mut entries: Vec<Entry> = self
            .crew
            .store
            .jobs_in(&folder.path, thread)
            .into_iter()
            .filter(|j| j.goal == 0 && (j.race == 0 || j.race == j.id))
            .cloned()
            .map(|j| Entry::Job(Box::new(j)))
            .collect();
        entries.extend(
            self.crew
                .store
                .goals
                .iter()
                .filter(|g| {
                    g.folder.path == folder.path
                        && self.crew.store.thread_for(&folder.path, g.thread) == thread
                })
                .cloned()
                .map(Entry::Plan),
        );
        entries.extend(
            self.crew
                .store
                .talks
                .iter()
                .filter(|t| {
                    t.folder == folder.path
                        && self.crew.store.thread_for(&folder.path, t.thread) == thread
                })
                .cloned()
                .map(Entry::Talk),
        );
        entries.sort_by_key(|e| e.at());
        let total = entries.len();
        // The last 30 things, newest at the bottom beside the composer.
        let shown: Vec<Entry> = entries.into_iter().skip(total.saturating_sub(30)).collect();

        // Jump to the newest when the thread, or what's in it, changes. A plain
        // top-down list with its own scroll: a bottom-up one let the wheel run
        // past the last message into empty space.
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            folder.path.hash(&mut h);
            name.hash(&mut h);
            total.hash(&mut h);
            for e in &shown {
                e.at().hash(&mut h);
                if let Entry::Job(j) = e {
                    j.feed.len().hash(&mut h);
                    (j.status as u8).hash(&mut h);
                }
            }
            h.finish()
        };
        if key != self.thread_key {
            self.thread_key = key;
            self.thread_scroll.scroll_to_bottom();
        }
        let empty = shown.is_empty();
        let mut feed = div()
            .w_full()
            .max_w(px(COLUMN))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(self.thread_intro(thread, &name, empty, total));
        let mut day = String::new();
        let mut marked = since == 0;
        for e in shown {
            let at = e.at();
            let today = day_label(at);
            if today != day {
                feed = feed.child(divider(&today, None));
                day = today;
            }
            if !marked && at > since {
                marked = true;
                feed = feed.child(divider("New", Some(ui::RED)));
            }
            feed = feed.child(match e {
                Entry::Job(j) => self.job_entry(&j, cx).into_any_element(),
                Entry::Plan(g) => self.plan_entry(&g, cx).into_any_element(),
                Entry::Talk(t) => self.talk_entry(&t, cx).into_any_element(),
            });
        }

        if thread == 0 {
            if let Some(post) = self.catch_up(&folder.path, since, cx) {
                if !marked {
                    feed = feed.child(divider("New", Some(ui::RED)));
                }
                feed = feed.child(post);
            }
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
            .child(chat)
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
            .map(|j| self.reply_pane(&j, &name, cx));
        let page = div()
            .size_full()
            .flex()
            .child(div().flex_1().min_w_0().h_full().child(page))
            .children(pane)
            .into_any_element();
        let pick = None;
        self.split(pick, page, cx)
    }

    /// Peek's catch-up at the bottom of #general when you come back: what
    /// finished, failed or waits on you across the project since you last
    /// looked, and what's still running. Worked out each time, not stored.
    fn catch_up(&mut self, path: &str, since: u64, cx: &mut Context<Self>) -> Option<Div> {
        if since == 0 {
            return None;
        }
        let store = &self.crew.store;
        let mine: Vec<&Job> = store
            .listed()
            .into_iter()
            .filter(|j| j.folder.path == path)
            .collect();
        let ended_since = |j: &&Job| j.ended.is_some_and(|e| e > since);
        let done: Vec<String> = mine
            .iter()
            .filter(|j| ended_since(j) && matches!(j.status, Status::Done | Status::Shipped))
            .map(|j| format!("{} finished {}", j.bot.name(), j.label()))
            .collect();
        let failed = mine
            .iter()
            .filter(|j| ended_since(j) && j.status == Status::Failed)
            .count();
        let review = mine.iter().filter(|j| j.status == Status::Review).count();
        let running = mine
            .iter()
            .filter(|j| matches!(j.status, Status::Running | Status::Shipping))
            .count();
        if done.is_empty() && failed == 0 && review == 0 {
            return None;
        }
        let plural = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        let mut lines: Vec<String> = done.into_iter().take(3).collect();
        if review > 0 {
            lines.push(format!(
                "{} waiting for your OK",
                plural(review, "change is", "changes are")
            ));
        }
        if failed > 0 {
            lines.push(format!("{} failed", plural(failed, "job", "jobs")));
        }
        if running > 0 {
            lines.push(format!(
                "{} still running",
                plural(running, "job is", "jobs are")
            ));
        }
        let face = self.avatar(Member::Fin, 34.);
        let mut list = div().flex().flex_col().gap(px(3.));
        for l in lines {
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        div()
                            .mt(px(8.))
                            .size(px(5.))
                            .rounded_full()
                            .bg(c(ui::faint())),
                    )
                    .child(l),
            );
        }
        let needs = review + failed;
        Some(
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
                        .gap(px(6.))
                        .child(
                            byline("Peek", store::now()).child(
                                div()
                                    .px(px(6.))
                                    .rounded(px(5.))
                                    .bg(ca(ui::VIOLET, 0.12))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::t_violet()))
                                    .child("Catch-up"),
                            ),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(c(ui::body()))
                                .child("While you were away:"),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(21.))
                                .text_color(c(ui::body()))
                                .child(list),
                        )
                        .when(needs > 0, |d| {
                            d.child(
                                div().pt(px(4.)).flex().gap(px(6.)).child(
                                    ui::ghost("catch-up-needs", "Open Needs you")
                                        .h(px(28.))
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.go(Place::Inbox, cx)),
                                        ),
                                ),
                            )
                        }),
                ),
        )
    }

    /// A job's conversation beside its thread: what the bot did and said,
    /// with a box to answer it, like replies in a thread.
    fn reply_pane(&mut self, job: &Job, thread: &str, cx: &mut Context<Self>) -> Div {
        div()
            .w(px(460.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(c(ui::line2()))
            .bg(c(ui::bg()))
            .child(
                div()
                    .h(px(46.))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(c(ui::line2()))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(ui::ink()))
                            .child("Thread"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(c(ui::faint()))
                            .child(format!("#{thread} \u{b7} {}", job.label())),
                    )
                    .child(
                        ui::icon_btn("reply-full", IconName::Maximize)
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Open the whole job")
                                    .build(window, cx)
                            })
                            .on_click({
                                let id = job.id;
                                cx.listener(move |this, _, _, cx| {
                                    this.reply_open = None;
                                    this.open_job(id, cx);
                                })
                            }),
                    )
                    .child(
                        ui::icon_btn("reply-close", IconName::X).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.reply_open = None;
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.live_main(job, cx)),
            )
    }

    /// The bar over a thread, like a channel's: its name, who's in it as a
    /// row of faces, and Add a bot, which starts an @ in the box.
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
        for (i, b) in bots.iter().take(5).enumerate() {
            let f = self.avatar(*b, 20.);
            faces = faces.child(
                div()
                    .when(i > 0, |d| d.ml(px(-5.)))
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(c(ui::bg()))
                    .child(f),
            );
        }
        let names: Vec<&str> = bots.iter().map(|b| b.name()).collect();
        let about = if busy > 0 {
            format!("{} \u{b7} {busy} working", bots.len())
        } else {
            bots.len().to_string()
        };
        div()
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
                div()
                    .flex_none()
                    .text_size(px(15.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(ui::ink()))
                    .child(format!("#{name}")),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child(super::and_list(&names)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("channel-members")
                    .h(px(30.))
                    .pl(px(4.))
                    .pr(px(10.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_size(px(12.))
                    .text_color(c(ui::muted()))
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(ui::line_hi())).text_color(c(ui::ink())))
                    .child(faces)
                    .child(about)
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Crew, cx))),
            )
            .child(
                ui::ghost_i("channel-add-bot", IconName::Plus, "Add a bot")
                    .h(px(30.))
                    .flex_none()
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
    }

    /// A question asked in the thread, and the bot's answer under it.
    fn talk_entry(&mut self, t: &store::Talk, cx: &mut Context<Self>) -> Div {
        let face = self.avatar(t.bot, 34.);
        let waiting = t.answer.is_none() && self.crew.asking_talk == Some(t.id);
        let accent = t.bot.accent();
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
                .when_some(a.via.clone(), |d, via| {
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child(via),
                    )
                }),
            None if waiting => {
                let secs = self
                    .crew
                    .asking
                    .as_ref()
                    .map(|a| a.started.elapsed().as_secs())
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
            self.message(
                format!("you-talk-{}", t.id),
                t.question.clone(),
                Msg::Mine(t.question.clone()),
                mine(t.question.clone(), t.at),
                cx,
            )
        });
        div()
            .flex()
            .flex_col()
            .children(asked)
            .children(refs)
            .child(
                self.message(
                    format!("talk-{}", t.id),
                    copy,
                    Msg::Answer(t.bot, t.question.clone()),
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
                                .gap(px(4.))
                                .child(
                                    byline(t.bot.name(), t.at).child(
                                        div()
                                            .px(px(6.))
                                            .rounded(px(5.))
                                            .bg(ca(accent, 0.12))
                                            .text_size(px(11.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(c(super::theme::mix(
                                                accent,
                                                ui::ink(),
                                                0.35,
                                            )))
                                            .child(if t.bot == Member::Fin {
                                                "Lead"
                                            } else {
                                                "Answers"
                                            }),
                                    ),
                                )
                                .child(answer)
                                .when(!t.choices.is_empty(), |d| {
                                    let mut row = div().pt(px(4.)).flex().flex_wrap().gap(px(6.));
                                    let talk = t.id;
                                    for (k, choice) in t.choices.iter().enumerate() {
                                        let said = choice.clone();
                                        row =
                                            row.child(
                                                ui::ghost(
                                                    ("talk-choice", t.id as usize * 16 + k),
                                                    choice.clone(),
                                                )
                                                .h(px(28.))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.pick_choice(talk, said.clone(), cx)
                                                })),
                                            );
                                    }
                                    d.child(row)
                                }),
                        ),
                    cx,
                ),
            )
    }

    /// The top of a thread: what it's for and who's in it.
    fn thread_intro(&mut self, thread: u64, name: &str, empty: bool, total: usize) -> Div {
        let here = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        let bots = self.crew.store.members(&here, thread);
        let mut faces = div().flex().items_center();
        for (i, b) in bots.iter().take(8).enumerate() {
            let f = self.avatar(*b, 26.);
            faces = faces.child(div().when(i > 0, |d| d.ml(px(-6.))).child(f));
        }
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
            .border_b_1()
            .border_color(c(ui::line2()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(ui::ink()))
                            .child(format!("#{name}")),
                    )
                    .child(div().flex_1())
                    .child(faces),
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
        let id = j.id;
        let running = j.status == Status::Running;
        let face = if running {
            self.bot(j.bot, Mood::Working, 34., Framing::Bust)
        } else if j.needs_you() {
            self.bot(j.bot, Mood::NeedsYou, 34., Framing::Bust)
        } else {
            self.avatar(j.bot, 34.)
        };
        let (word, tone) = status_of(j);
        let frac = j.progress().map(|(d, n)| d as f32 / n.max(1) as f32);
        let accent = j.bot.accent();
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
        let approve = (review && !racing).then(|| self.approve_label(j));
        let failed_checks = j.checks.iter().filter(|c| !c.ok).count();

        let head = div()
            .px(px(14.))
            .py(px(11.))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .size(px(7.))
                    .flex_none()
                    .rounded_full()
                    .bg(c(if running { accent } else { tone.0 })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(card_title(j)),
            )
            .child(if running {
                // Working: a pill in the bot's own colour, with how long so far.
                let k = 0.4
                    + 0.6
                        * runo_holo::anim::alternate(self.now(), 0.9, runo_holo::anim::SOFT) as f32;
                let what = match j.progress() {
                    Some((d, n)) => format!("Step {} of {n}", (d + 1).min(n)),
                    None => "Working".to_string(),
                };
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(22.))
                    .px(px(9.))
                    .rounded_full()
                    .bg(ca(accent, 0.12))
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(super::theme::mix(accent, ui::ink(), 0.3)))
                    .child(div().size(px(6.)).rounded_full().bg(c(accent)).opacity(k))
                    .child(format!("{what} \u{b7} {}", store::span(j.seconds())))
            } else {
                div()
                    .flex_none()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(tone.0))
                    .child(word.clone())
            });

        let mut body = card()
            .when(running, |d| {
                d.border_color(c(super::theme::mix(ui::line(), accent, 0.45)))
            })
            .when(review, |d| d.border_color(c(ui::ask_tone().2)))
            .child(head);
        if let Some(f) = frac.filter(|_| running) {
            body = body.child(div().px(px(14.)).child(ui::bar(f, accent).h(px(3.))));
        }
        if running {
            // What it's doing now, in plain words, then its last few steps.
            let now_doing = match tail.last() {
                Some((0, label, _)) => store::plain_step(label, true),
                _ if !j.plan.is_empty() => j.doing(),
                _ => "Thinking it through".to_string(),
            };
            let open = self.raw_open.contains(&id);
            let mut steps = div()
                .mx(px(14.))
                .mt(px(8.))
                .mb(px(10.))
                .flex()
                .flex_col()
                .gap(px(6.));
            steps = steps.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mx(px(-6.))
                    .px(px(6.))
                    .py(px(5.))
                    .rounded(px(8.))
                    .bg(ca(accent, 0.06))
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(ui::ink()))
                    .child({
                        let k = 0.35
                            + 0.65
                                * runo_holo::anim::alternate(self.now(), 0.9, runo_holo::anim::SOFT)
                                    as f32;
                        div()
                            .size(px(8.))
                            .mx(px(3.))
                            .flex_none()
                            .rounded_full()
                            .bg(c(accent))
                            .opacity(k)
                    })
                    .child(div().min_w_0().truncate().child(now_doing)),
            );
            let done: Vec<&(u8, String, String)> =
                tail.iter().filter(|(st, _, _)| *st != 0).collect();
            for (n, (state, label, kind)) in done.into_iter().enumerate() {
                let (icon, color, text) = match state {
                    1 => (
                        IconName::Check,
                        ui::t_green(),
                        store::plain_step(label, false),
                    ),
                    _ => (
                        IconName::RotateCcw,
                        ui::muted(),
                        format!(
                            "{} didn't work, trying another way",
                            store::plain_step(label, false)
                        ),
                    ),
                };
                let raw = if kind.is_empty() {
                    label.clone()
                } else {
                    format!("{kind}: {label}")
                };
                steps = steps.child(
                    div()
                        .id(("job-step", (id as usize) * 8 + n))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .child(Icon::new(icon).size(px(12.)).text_color(c(color)))
                        .child(div().min_w_0().truncate().child(text))
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(raw.clone())
                                .build(window, cx)
                        }),
                );
            }
            body = body.child(steps);
            if open && !tail.is_empty() {
                let mut raw = div()
                    .mx(px(14.))
                    .mb(px(10.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(c(ui::well()))
                    .border_1()
                    .border_color(c(ui::line2()))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .text_color(c(ui::muted()));
                for (_, label, _) in &tail {
                    raw = raw.child(div().min_w_0().truncate().child(label.clone()));
                }
                body = body.child(raw);
            }
        }
        // What the bot needs your yes for, right here in the chat.
        if let Some(asks) = self.ask_card(j, cx) {
            body = body.child(asks);
        }
        if review || matches!(j.status, Status::Done | Status::Shipped) && !j.changes.is_empty() {
            body = body.child(
                div()
                    .px(px(14.))
                    .pt(px(2.))
                    .pb(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(ui::badge(ui::ok_tone(), format!("+{added}")))
                    .child(ui::badge(ui::bad_tone(), format!("\u{2212}{removed}")))
                    .child(div().text_color(c(ui::muted())).child(format!(
                        "{} {}",
                        j.changes.len(),
                        if j.changes.len() == 1 {
                            "file"
                        } else {
                            "files"
                        }
                    )))
                    .child(div().flex_1())
                    .when(!j.checks.is_empty(), |d| {
                        d.child(
                            div()
                                .text_color(c(if failed_checks == 0 {
                                    ui::t_green()
                                } else {
                                    ui::t_red()
                                }))
                                .child(if failed_checks == 0 {
                                    format!("{} checks passed", j.checks.len())
                                } else {
                                    format!("{failed_checks} of {} checks failed", j.checks.len())
                                }),
                        )
                    }),
            );
        }
        // What you can do about it, right here.
        let mut actions = div()
            .px(px(14.))
            .py(px(9.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(c(ui::well()))
            .border_t_1()
            .border_color(c(ui::line2()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(c(ui::faint()))
                    .child(if racing {
                        format!("Race of {tries} engines")
                    } else {
                        format!("#{id} \u{b7} {}", j.engine)
                    }),
            )
            .map(|d| {
                if racing {
                    let race = j.race;
                    d.child(
                        ui::solid(
                            ("thread-compare", id as usize),
                            format!("Compare {tries} tries"),
                        )
                        .h(px(28.))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.go(Place::Race(race), cx)),
                        ),
                    )
                } else {
                    d.child(
                        ui::ghost(("thread-open", id as usize), "Open")
                            .h(px(28.))
                            .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
                    )
                }
            });
        if running && !tail.is_empty() {
            let open = self.raw_open.contains(&id);
            actions = actions.child(
                div()
                    .id(("job-raw", id as usize))
                    .px(px(6.))
                    .text_size(px(12.))
                    .text_color(c(ui::muted()))
                    .cursor_pointer()
                    .hover(|s| s.text_color(c(ui::ink())))
                    .child(if open { "Hide commands" } else { "Commands" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.raw_open.remove(&id) {
                            this.raw_open.insert(id);
                        }
                        cx.notify();
                    })),
            );
        }
        if running {
            actions = actions.child(
                ui::ghost(("thread-stop", id as usize), "Stop")
                    .h(px(28.))
                    .on_click(cx.listener(move |this, _, _, cx| this.stop_job(id, cx))),
            );
        }
        if let Some(label) = approve {
            actions = actions
                .child(
                    ui::ghost(("thread-discard", id as usize), "Discard")
                        .h(px(28.))
                        .on_click(cx.listener(move |this, _, _, cx| this.discard_job(id, cx))),
                )
                .child(
                    ui::solid(("thread-approve", id as usize), label)
                        .h(px(28.))
                        .on_click(cx.listener(move |this, _, _, cx| this.approve(id, cx))),
                );
        }
        if matches!(
            j.status,
            Status::Failed | Status::Interrupted | Status::Stopped
        ) {
            actions = actions.child(
                ui::ghost(("thread-retry", id as usize), "Try again")
                    .h(px(28.))
                    .on_click(cx.listener(move |this, _, _, cx| this.retry_job(id, cx))),
            );
        }
        body = body.child(actions);

        let reply = div()
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
                    .child(
                        byline(j.bot.name(), j.started).child(
                            div()
                                .px(px(6.))
                                .rounded(px(5.))
                                .bg(ca(accent, 0.12))
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(super::theme::mix(accent, ui::ink(), 0.35)))
                                .child(j.bot.role().to_string()),
                        ),
                    )
                    .when(!running, |d| {
                        d.child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(21.))
                                .text_color(c(ui::body()))
                                .child(said),
                        )
                    })
                    .child(div().when(running, |d| d.mt(px(4.))).child(body)),
            );
        // Peek's hand-off, when it picked the bot: a small line between your
        // ask and the bot's reply.
        let handoff = (!j.handoff.is_empty()).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .py(px(4.))
                .pl(px(46.))
                .child(self.avatar(Member::Fin, 20.))
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
                        .text_size(px(13.))
                        .text_color(c(ui::muted()))
                        .child(j.handoff.clone()),
                )
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(self.message(
                format!("you-job-{}", j.id),
                j.title.clone(),
                Msg::Mine(j.title.clone()),
                mine(j.title.clone(), j.started),
                cx,
            ))
            .children(handoff)
            .child(self.message(
                format!("job-{}", j.id),
                j.label().to_string(),
                Msg::Job(j.id, j.status, j.bot, j.label().to_string()),
                reply,
                cx,
            ))
    }

    /// A plan: your goal, then Peek's plan card with a step per job.
    fn plan_entry(&mut self, g: &Goal, cx: &mut Context<Self>) -> Div {
        let id = g.id;
        let mood = match g.state {
            State::Planning => Mood::Thinking,
            State::Ready | State::Paused => Mood::NeedsYou,
            State::Done => Mood::Idle,
            _ => Mood::Idle,
        };
        let face = self.bot(Member::Fin, mood, 34., Framing::Bust);
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
                            .on_click(cx.listener(move |this, _, _, cx| this.open_job(jid, cx)))
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
                        ui::ghost(("plan-drop", id as usize), "Drop")
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
                        ui::solid(("plan-start", id as usize), "Start the crew")
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
        let reply = div()
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
                    .child(byline("Peek", g.started))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .text_color(c(ui::body()))
                            .child(said),
                    )
                    .child(card),
            );
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(self.message(
                format!("you-plan-{}", g.id),
                g.text.clone(),
                Msg::Mine(g.text.clone()),
                mine(g.text.clone(), g.started),
                cx,
            ))
            .child(self.message(
                format!("plan-{}", g.id),
                g.text.clone(),
                Msg::Plan(g.id, g.state == State::Ready),
                reply,
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
    fn thread_composer(&mut self, name: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.composer.read(cx).value().to_string();
        if !self.mode_pinned && !value.starts_with('/') {
            self.mode = Mode::guess(&value);
        }
        let target = if self.mode == Mode::Ask {
            self.ask_who()
        } else {
            self.doer()
        };
        let ready = !value.trim().is_empty();
        let face = self.avatar(target, 20.);
        let plan_first = cx.global::<crate::settings::AppSettings>().v2_plan_first;
        let named = super::mentioned(&value).map(|(m, _)| m);
        let goes = if let Some(m) = named.filter(|_| !value.starts_with('/')) {
            format!("{} takes it", m.name())
        } else if value.starts_with("/thread") {
            "Makes a new thread".to_string()
        } else if value.starts_with("/rename") {
            "Renames this thread".to_string()
        } else if value.starts_with("/archive") {
            "Archives this thread".to_string()
        } else if value.starts_with("/delete") {
            "Deletes this thread".to_string()
        } else if value.starts_with("/ask") || (self.mode == Mode::Ask && !value.starts_with('/')) {
            format!("{} answers here", self.ask_who().name())
        } else if value.starts_with("/race") {
            // A race runs as asked, plan-first or not, as hand_over does.
            format!("{} races it", target.name())
        } else if (value.starts_with("/plan") || plan_first || self.splitting) && self.mode.writes()
        {
            "Peek plans it first".to_string()
        } else {
            format!("{} \u{b7} {}", target.name(), self.mode.label())
        };
        let placeholder_hint = format!("Message #{name}. Type / for commands.");
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
            .children(self.at_strip(cx))
            .child(
                div()
                    .min_h(px(44.))
                    .text_size(px(14.5))
                    .line_height(px(21.))
                    .child(
                        Textarea::new(&self.composer)
                            .appearance(false)
                            .bordered(false),
                    ),
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
                            .child(placeholder_hint),
                    )
                    .child(
                        div()
                            .id("thread-goes-to")
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
                            .child(goes),
                    )
                    .child(
                        div()
                            .id("thread-send")
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
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send_from_composer(window, cx);
                            })),
                    ),
            )
    }

    /// Sends what's in the composer to the thread you're in.
    pub(super) fn send_from_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.composer
            .update(cx, |i, cx| i.set_value("", window, cx));
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
