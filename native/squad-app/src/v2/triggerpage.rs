//! Settings, Triggers: make a job that starts by itself, and see each trigger's
//! runs. The triggers themselves are in [`super::triggers`]; what a run started
//! shows in the Inbox like any job, with Run again.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::data::Member;
use super::store::{self, Status};
use super::triggers::{Trigger, When};
use super::ui::{self, c, space};
use super::{Pane, Place, Shell};

/// The kinds a new trigger can be, in the order the picker shows them.
const KINDS: [(&str, IconName); 4] = [
    ("Schedule", IconName::Clock),
    ("Issue label", IconName::Tag),
    ("CI fails", IconName::CircleX),
    ("Link", IconName::Link),
];

/// What each kind's box takes, as its placeholder.
pub(super) const ARG_EXAMPLES: [&str; 4] = ["weekdays at 9am", "squad", "main", ""];

/// Runs shown under each trigger.
const SHOWN_RUNS: usize = 5;

fn field(input: &Entity<InputState>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line_hi()))
        .text_size(px(12.5))
        .child(Input::new(input).appearance(false).bordered(false))
}

fn area(input: &Entity<TextareaState>) -> Div {
    div()
        .p(px(10.))
        .rounded(px(5.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line_hi()))
        .text_size(px(12.5))
        .child(Textarea::new(input).appearance(false).bordered(false))
}

fn label(text: &'static str) -> Div {
    div()
        .font_family(ui::mono())
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(ui::faint()))
        .child(text)
}

fn card(title: &'static str) -> Div {
    div()
        .rounded(px(6.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line()))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(div().px(px(14.)).pt(px(11.)).pb(px(8.)).child(label(title)))
}

/// A small picker button, like a segment.
fn pill(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(4.))
        .text_size(px(12.5))
        .cursor_pointer()
        .border_1()
        .map(|d| {
            if on {
                d.bg(c(ui::pick_fill()))
                    .border_color(c(ui::pick_fill()))
                    .text_color(c(ui::pick_on()))
                    .font_weight(FontWeight::SEMIBOLD)
            } else {
                d.border_color(c(ui::line()))
                    .text_color(c(ui::muted()))
                    .hover(|s| s.text_color(c(ui::ink())))
            }
        })
}

/// How a run went, as a word and its colour.
pub(super) fn run_state(rt: &super::runtime::Runtime, r: &super::triggers::Run) -> (String, u32) {
    if !r.skipped.is_empty() {
        return (format!("skipped: {}", r.skipped), ui::faint());
    }
    if r.job != 0 {
        return match rt.store.job(r.job) {
            Some(j) => (
                format!("#{} {}", j.id, j.status.label().to_lowercase()),
                super::live::status_color(j.status),
            ),
            None => (format!("#{} removed", r.job), ui::faint()),
        };
    }
    if rt.store.later.iter().any(|l| l.id == r.later) {
        ("waiting to start".into(), ui::AMBER)
    } else {
        ("didn't start".into(), ui::faint())
    }
}

impl Shell {
    /// The arg box's hint and what it reads as now, for the kind picked.
    fn trigger_arg_line(&self, cx: &App) -> (String, bool) {
        let arg = self.trig_arg.read(cx).value().trim().to_string();
        match self.trig_kind {
            0 if arg.is_empty() => (
                "Like \u{201c}weekdays at 9am\u{201d}, \u{201c}every 2 hours\u{201d} or a cron expression, in local time.".into(),
                false,
            ),
            0 => match crate::schedule::from_words(&arg) {
                Ok(cron) => (crate::schedule::describe(&cron), false),
                Err(why) => (why, true),
            },
            1 => (
                "Open issues that get this label start a job. Only people who can triage the repository can label, so a label is their go-ahead.".into(),
                false,
            ),
            2 => (
                "Failed GitHub Actions runs on this branch start a fix, one at a time.".into(),
                false,
            ),
            _ => (
                "A secret link to call from a script, a git hook or another service. What it sends reaches the bot quoted, as information.".into(),
                false,
            ),
        }
    }

    fn add_trigger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.crew.store.folder().cloned() else {
            self.trig_note = ("Open a project first: triggers run in one.".into(), true);
            cx.notify();
            return;
        };
        let arg = self.trig_arg.read(cx).value().trim().to_string();
        let when = match self.trig_kind {
            0 => When::Schedule { cron: arg },
            1 => When::Label {
                label: arg,
                author: String::new(),
            },
            2 => When::CiFails {
                branch: arg,
                workflow: String::new(),
            },
            _ => When::Link {
                token: String::new(),
            },
        };
        let t = Trigger {
            name: self.trig_name.read(cx).value().trim().to_string(),
            when,
            text: self.trig_text.read(cx).value().trim().to_string(),
            bot: self.trig_bot,
            mode: self.trig_bot.mode(),
            folder,
            thread: self.crew.store.thread_here(),
            ..Default::default()
        };
        match self.crew.add_trigger(t) {
            Ok(id) => {
                let name = self
                    .crew
                    .store
                    .triggers
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.name.clone())
                    .unwrap_or_default();
                self.trig_note = (format!("Added \u{201c}{name}\u{201d}."), false);
                for i in [&self.trig_name, &self.trig_arg] {
                    i.update(cx, |i, cx| i.set_value("", window, cx));
                }
                self.trig_text
                    .update(cx, |i, cx| i.set_value("", window, cx));
            }
            Err(why) => self.trig_note = (why, true),
        }
        cx.notify();
    }

    pub(super) fn settings_triggers(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let body = div()
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(self.new_trigger_card(cx))
            .when(!self.crew.store.triggers.is_empty(), |d| {
                d.child(self.triggers_card(cx))
            });
        Self::page(
            "settings-triggers",
            "Triggers",
            "Jobs that start by themselves: on a schedule, when an issue gets a label, when CI fails, or when a link is called. What they do lands in your Inbox.",
            body,
        )
    }

    fn new_trigger_card(&mut self, cx: &mut Context<Self>) -> Div {
        let mut kinds = div().flex().flex_wrap().gap(px(6.));
        for (k, (name, icon)) in KINDS.iter().enumerate() {
            let on = self.trig_kind as usize == k;
            kinds = kinds.child(
                pill(("trig-kind", k), on)
                    .child(Icon::new(*icon).size(px(13.)))
                    .child(*name)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.trig_kind != k as u8 {
                            // A fresh box, with an example of what this kind takes.
                            this.trig_arg = cx
                                .new(|cx| InputState::new(window, cx).placeholder(ARG_EXAMPLES[k]));
                        }
                        this.trig_kind = k as u8;
                        this.trig_note = (String::new(), false);
                        cx.notify();
                    })),
            );
        }
        let removed = self.crew.store.removed.clone();
        let mut bots = div().flex().flex_wrap().gap(px(6.));
        for (k, who) in Member::CREW
            .into_iter()
            .filter(|m| !removed.contains(m))
            .enumerate()
        {
            let on = self.trig_bot == who;
            bots = bots.child(
                pill(("trig-bot", k), on)
                    .child(div().size(px(7.)).rounded_full().bg(c(who.accent())))
                    .child(who.name())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.trig_bot = who;
                        cx.notify();
                    })),
            );
        }
        let (hint, bad) = self.trigger_arg_line(cx);
        let arg_title = match self.trig_kind {
            0 => "WHEN",
            1 => "LABEL",
            2 => "BRANCH",
            _ => "",
        };
        let project = self
            .crew
            .store
            .folder()
            .map(|f| format!("Runs in {}", f.name))
            .unwrap_or_else(|| "Open a project first".into());
        let (note, failed) = self.trig_note.clone();
        let inner = div()
            .px(px(14.))
            .pb(px(14.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(kinds)
            .child(
                div()
                    .flex()
                    .gap(px(space::M))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(label("NAME"))
                            .child(field(&self.trig_name)),
                    )
                    .when(self.trig_kind < 3, |d| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(label(arg_title))
                                .child(field(&self.trig_arg)),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c(if bad { ui::RED } else { ui::body() }))
                    .child(hint),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(label("WHO DOES IT"))
                    .child(bots),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(label("WHAT TO DO EACH TIME"))
                    .child(area(&self.trig_text)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::M))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(c(if failed {
                                ui::RED
                            } else if note.is_empty() {
                                ui::faint()
                            } else {
                                ui::body()
                            }))
                            .child(if note.is_empty() { project } else { note }),
                    )
                    .child(
                        ui::dbtn_i("trig-add", IconName::Plus, "Add trigger", ui::Btn::Primary)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_trigger(window, cx)),
                            ),
                    ),
            );
        card("NEW TRIGGER").child(inner)
    }

    fn triggers_card(&mut self, cx: &mut Context<Self>) -> Div {
        let now = store::now();
        let mut list = div().flex().flex_col();
        let triggers = self.crew.store.triggers.clone();
        for (k, t) in triggers.iter().enumerate() {
            let id = t.id;
            let on = t.on;
            let mut facts = vec![t.summary(), t.bot.name().to_string(), t.folder.name.clone()];
            if let (When::Schedule { .. }, true, true) = (&t.when, t.on, t.next_at > 0) {
                facts.push(format!("next {}", super::triggers::clock(t.next_at)));
            }
            if t.when.reads_github() && t.on {
                facts.push(if t.looked == 0 {
                    "looks at GitHub soon".to_string()
                } else {
                    format!("looked {}", store::ago(t.looked))
                });
            }
            let armed = self.trig_arm == Some(id);
            let head = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .flex_none()
                        .size(px(28.))
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(c(ui::well()))
                        .text_color(c(if on { ui::ink() } else { ui::faint() }))
                        .child(Icon::new(IconName::Zap).size(px(14.))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(ui::ink()))
                                .child(t.name.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(c(ui::muted()))
                                .child(facts.join(" \u{b7} ")),
                        ),
                )
                .child(
                    ui::dbtn(("trig-run", k), "Run now", ui::Btn::Plain).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if let Err(why) = this.crew.run_trigger_now(id) {
                                this.trig_note = (why, true);
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(
                    ui::dbtn(
                        ("trig-rm", k),
                        if armed { "Remove it" } else { "Remove" },
                        ui::Btn::Ghost,
                    )
                    .when(armed, |d| d.text_color(c(ui::RED)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.trig_arm == Some(id) {
                            this.crew.remove_trigger(id);
                            this.trig_arm = None;
                        } else {
                            this.trig_arm = Some(id);
                        }
                        cx.notify();
                    })),
                )
                .child(
                    div()
                        .id(("trig-on", k))
                        .cursor_pointer()
                        .child(super::prefs::switch(("trig", id), on))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.crew.set_trigger_on(id, !on);
                            cx.notify();
                        })),
                );
            let mut block = div()
                .mx(px(14.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .when(k > 0, |d| d.border_t_1().border_color(c(ui::line())))
                .child(head);
            if let Some(token) = t.token() {
                let port = self
                    .crew
                    .hooks
                    .as_ref()
                    .map(|h| h.port)
                    .unwrap_or(super::triggers::HOOK_PORT);
                let url = format!(
                    "curl -X POST http://127.0.0.1:{port}/v1/hooks/{token} -d 'what happened'"
                );
                let copy = url.clone();
                block = block.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .px(px(8.))
                                .py(px(5.))
                                .rounded(px(4.))
                                .bg(c(ui::well()))
                                .font_family(ui::mono())
                                .text_size(px(11.5))
                                .text_color(c(ui::body()))
                                .child(url),
                        )
                        .child(
                            ui::dbtn_i(("trig-copy", k), IconName::Copy, "Copy", ui::Btn::Plain)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                    this.trig_note = (
                                        "Copied. On your own server, the same path works on squad serve's port.".into(),
                                        false,
                                    );
                                    cx.notify();
                                })),
                        ),
                );
            }
            if !t.trouble.is_empty() {
                block = block.child(
                    div()
                        .text_size(px(12.))
                        .text_color(c(ui::RED))
                        .child(t.trouble.clone()),
                );
            }
            if t.runs.is_empty() {
                block = block.child(div().text_size(px(12.)).text_color(c(ui::faint())).child(
                    match &t.when {
                        When::Label { .. } | When::CiFails { .. } => {
                            "No runs yet. What's on GitHub now doesn't count; what comes next does."
                        }
                        _ => "No runs yet.",
                    },
                ));
            }
            for (n, r) in t.runs.iter().rev().take(SHOWN_RUNS).enumerate() {
                let (state, col) = run_state(&self.crew, r);
                let job = r.job;
                let ago = if now.saturating_sub(r.at) < 60 {
                    "just now".to_string()
                } else {
                    super::triggers::clock(r.at)
                };
                block = block.child(
                    div()
                        .id(("trig-run-row", k * 100 + n))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .when(job != 0 && self.crew.store.job(job).is_some(), |d| {
                            d.cursor_pointer()
                                .hover(|s| s.bg(c(ui::well())))
                                .on_click(cx.listener(move |this, _, _, cx| this.open_job(job, cx)))
                        })
                        .child(div().size(px(6.)).flex_none().rounded_full().bg(c(col)))
                        .child(
                            div()
                                .w(px(120.))
                                .flex_none()
                                .font_family(ui::mono())
                                .text_size(px(11.))
                                .text_color(c(ui::faint()))
                                .child(ago),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(c(ui::body()))
                                .child(r.why.clone()),
                        )
                        .child(div().flex_none().text_color(c(col)).child(state)),
                );
            }
            list = list.child(block);
        }
        card("YOUR TRIGGERS").child(list)
    }

    /// In the Inbox, under a job a trigger started: which trigger, its other runs,
    /// and Run again.
    pub(super) fn trigger_box(&mut self, j: &store::Job, cx: &mut Context<Self>) -> Option<Div> {
        if j.trigger == 0 {
            return None;
        }
        let id = j.id;
        let t = self
            .crew
            .store
            .triggers
            .iter()
            .find(|t| t.id == j.trigger)
            .cloned();
        let name = t
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or_else(|| "A trigger that was removed".into());
        let mut runs = div().flex().flex_col().gap(px(3.));
        if let Some(t) = &t {
            for (n, r) in t
                .runs
                .iter()
                .rev()
                .filter(|r| r.job != id)
                .take(SHOWN_RUNS - 1)
                .enumerate()
            {
                let (state, col) = run_state(&self.crew, r);
                let job = r.job;
                runs = runs.child(
                    div()
                        .id(("inbox-trig-run", n))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .when(job != 0 && self.crew.store.job(job).is_some(), |d| {
                            d.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.inbox_pick = Some(job);
                                    if !this.inbox_ids().contains(&job) {
                                        this.open_job(job, cx);
                                    }
                                    cx.notify();
                                }))
                        })
                        .child(div().size(px(6.)).flex_none().rounded_full().bg(c(col)))
                        .child(
                            div()
                                .w(px(120.))
                                .flex_none()
                                .font_family(ui::mono())
                                .text_size(px(11.))
                                .text_color(c(ui::faint()))
                                .child(super::triggers::clock(r.at)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(c(ui::body()))
                                .child(r.why.clone()),
                        )
                        .child(div().flex_none().text_color(c(col)).child(state)),
                );
            }
        }
        let running = j.status == Status::Running;
        Some(
            div()
                .p(px(12.))
                .rounded(px(6.))
                .bg(c(ui::well()))
                .border_1()
                .border_color(c(ui::line2()))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            Icon::new(IconName::Zap)
                                .size(px(14.))
                                .text_color(c(ui::ink())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.5))
                                .text_color(c(ui::body()))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(c(ui::ink()))
                                                .child(name),
                                        )
                                        .child(format!(
                                            "\u{b7} {}",
                                            j.trigger_why.trim_end_matches('.')
                                        )),
                                ),
                        )
                        .when(t.is_some(), |d| {
                            d.child(
                                ui::dbtn("inbox-trig-settings", "Trigger", ui::Btn::Ghost)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.go(Place::Settings(Pane::Triggers), cx)
                                    })),
                            )
                            .child(
                                ui::dbtn_i(
                                    "inbox-run-again",
                                    IconName::RotateCcw,
                                    "Run again",
                                    ui::Btn::Plain,
                                )
                                .when(running, |d| d.opacity(0.4))
                                .when(!running, |d| {
                                    d.on_click(cx.listener(move |this, _, _, cx| {
                                        match this.crew.run_again(id) {
                                            Ok(_) => {
                                                if let Some(j) = this.crew.store.job_mut(id) {
                                                    j.seen = true;
                                                }
                                                this.crew.store.save();
                                            }
                                            Err(why) => this.crew.notes.push(why),
                                        }
                                        cx.notify();
                                    }))
                                }),
                            )
                        }),
                )
                .child(runs),
        )
    }
}
