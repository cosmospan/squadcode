//! Live: one job, as the bot works. What it says and does on the left, as it
//! happens; its plan, the files it touched and where things stand on the right.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::{Framing, Mood};

use super::store::{self, Item, Job, Status};
use super::ui::{self, c, ca};
use super::{Place, Shell};

/// Agent text with the loudest Markdown taken out, so it reads as sentences.
pub(super) fn plain(text: &str) -> String {
    text.lines()
        .map(|l| {
            let l = l.trim_end();
            let l = l.trim_start_matches('#').trim_start();
            l.replace("**", "").replace("__", "")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn tool_icon(kind: &str) -> IconName {
    match kind {
        "read" | "list" => IconName::Eye,
        "edit" | "write" => IconName::Pencil,
        "delete" => IconName::Trash,
        "shell" => IconName::SquareTerminal,
        "search" | "websearch" => IconName::Search,
        "fetch" => IconName::Globe,
        _ => IconName::Sparkles,
    }
}

impl Shell {
    pub(super) fn live(&mut self, id: u64, cx: &mut Context<Self>) -> AnyElement {
        let Some(job) = self.crew.store.job(id).cloned() else {
            return self.empty_place(
                "That job is gone",
                "It may have been cleared. Everything else is in History.",
                cx,
            );
        };
        let drawer = self.term_drawer(&job, cx);
        // What Squad runs for this job (its app, a command from its words),
        // under it, whatever its feed scrolls to.
        let runs = self.job_runs(id, cx);
        let runs = (!runs.is_empty()).then(|| {
            div()
                .flex_none()
                .px(px(20.))
                .pb(px(10.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(runs)
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                // Its details live in the one right sidebar, beside the Jobs list.
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.live_main(&job, cx)),
            )
            .children(runs)
            .children(drawer)
            .into_any_element()
    }

    pub(super) fn live_main(&mut self, job: &Job, cx: &mut Context<Self>) -> impl IntoElement {
        let who = job.bot;
        let running = job.status == Status::Running;
        let mood = match job.status {
            Status::Running => Mood::Working,
            Status::Done => {
                if job
                    .ended
                    .is_some_and(|e| store::now().saturating_sub(e) < 6)
                {
                    Mood::Done
                } else {
                    Mood::Signature
                }
            }
            Status::Failed | Status::Review => Mood::NeedsYou,
            Status::Shipped | Status::Shipping => Mood::Signature,
            _ => Mood::Idle,
        };
        let hero = self.bot(who, mood, 26., Framing::Bust);
        let progress = job.progress();
        // The engine's own summary already says how long it took.
        let mut meta = if job.summary.is_empty() || running {
            vec![store::span(job.seconds())]
        } else {
            Vec::new()
        };
        if let Some((d, n)) = progress {
            meta.insert(
                0,
                format!(
                    "step {} of {n}",
                    (d + usize::from(job.plan.iter().any(|s| s.1 == 1))).min(n)
                ),
            );
        }
        if !job.summary.is_empty() {
            meta.push(job.summary.clone());
        }
        let accent = who.accent();
        let (word, tone) = super::home::status_of(job);
        let mut facts = vec![job.engine.clone(), job.folder.name.clone()];
        facts.extend(meta.iter().filter(|m| m.starts_with("step")).cloned());
        let actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .when(!self.trail.is_empty(), |d| {
                d.child(
                    ui::ghost_i("back", IconName::ArrowLeft, "Back")
                        .gap(px(6.))
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new("Back (⌘[)")
                                .build(window, cx)
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                )
            })
            .when(job.goal != 0, |d| {
                let gid = job.goal;
                let label = self
                    .crew
                    .store
                    .goals
                    .iter()
                    .find(|g| g.id == gid)
                    .and_then(|g| {
                        let k = g.steps.iter().position(|s| s.job == job.id)?;
                        Some(format!("Plan: part {} of {}", k + 1, g.steps.len()))
                    })
                    .unwrap_or_else(|| "Plan".into());
                d.child(
                    ui::ghost_i("to-goal", IconName::ListTodo, label)
                        .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Goal(gid), cx))),
                )
            })
            .map(|d| {
                let tries = if job.race == 0 {
                    0
                } else {
                    self.tries(job.race).len()
                };
                let id = job.id;
                if tries > 1 {
                    let race = job.race;
                    d.child(
                        ui::ghost_i(
                            "compare",
                            IconName::Columns2,
                            format!("Compare {tries} tries"),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.go(Place::Race(race), cx)),
                        ),
                    )
                } else if job.mode.writes()
                    && !job.worktree.is_empty()
                    && matches!(job.status, Status::Running | Status::Review)
                {
                    d.child(
                        ui::dbtn("race-it", "Race it", ui::Btn::Ghost)
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(
                                    "Try the same job on other engines and keep the best",
                                )
                                .build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.race_it(id, cx))),
                    )
                } else {
                    d
                }
            })
            // Approving lives on the review card below, not up here.
            .when(
                !running
                    && super::trouble::trouble(job).is_none()
                    && !matches!(
                        job.status,
                        Status::Done | Status::Review | Status::Shipping | Status::Shipped
                    ),
                |d| {
                    let id = job.id;
                    d.child(
                        ui::ghost_i("retry", IconName::RotateCcw, "Try again")
                            .on_click(cx.listener(move |this, _, _, cx| this.retry_job(id, cx))),
                    )
                },
            )
            // Its browser, terminal and files are the workspace's tabs.
            .when(running && !job.remote_on.is_empty(), |d| {
                d.child(self.screen_button("job-screen", "Watch its screen", cx))
            })
            .when(running, |d| {
                let id = job.id;
                d.child(
                    ui::ghost_i("stop", IconName::CircleStop, "Stop")
                        .text_color(c(ui::RED))
                        .on_click(cx.listener(move |this, _, _, cx| this.stop_job(id, cx))),
                )
            });
        let fraction = match (running, progress) {
            (true, Some((d, n))) => {
                Some(self.glide_bar(job.id, (d as f32 + 0.5) / n.max(1) as f32))
            }
            _ => None,
        };
        // One quiet line, like the design: the bot's face, the title, its
        // number and how it stands, with what you can do on the right. Who,
        // which engine and where are in the details beside it.
        let about = format!("{} \u{b7} {}", who.name(), facts.join(" \u{b7} "));
        let header = ui::chrome(div())
            .flex_none()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .min_h(px(50.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(20.))
                    .py(px(8.))
                    .child(
                        div()
                            .id("job-bot-dm")
                            .flex_none()
                            .size(px(26.))
                            .cursor_pointer()
                            .child(hero)
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(format!(
                                    "Message {}",
                                    who.name()
                                ))
                                .build(window, cx)
                            })
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.go(Place::Chat(who), cx)),
                            ),
                    )
                    .child(
                        div()
                            .id("job-title")
                            .flex_shrink(1.)
                            .min_w_0()
                            .truncate()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(job.label().to_string())
                            .tooltip({
                                let ask = format!("{}\n{about}", job.title);
                                move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(ask.clone())
                                        .build(window, cx)
                                }
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .text_color(c(ui::faint()))
                            .child(format!("#{}", job.id)),
                    )
                    .child(ui::mono_status(
                        if running { ui::t_blue() } else { tone.0 },
                        &word,
                    ))
                    .child(div().flex_1())
                    .child(div().flex_none().child(actions)),
            )
            .child(match fraction {
                Some(f) => div().h(px(2.)).w_full().child(ui::bar(f, accent).h(px(2.))),
                None if running => div()
                    .h(px(2.))
                    .w_full()
                    .child(ui::sweep(accent, 0.0).h(px(2.))),
                None => div(),
            });

        // The feed: what the bot said, the steps it took, and what you added.
        let mut feed = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(18.))
            .pt(px(14.))
            .pb(px(8.));
        // Who spoke last, so a run of one speaker's lines shows its name once.
        let mut last_by: Option<Option<super::Member>> = None;
        let mut tools: Vec<(String, String, u8)> = Vec::new();
        // A step in progress shows a dot that breathes.
        let breathe = 0.35 + 0.65 * super::motion::breath();
        let flush = |feed: Div, tools: &mut Vec<(String, String, u8)>| -> Div {
            if tools.is_empty() {
                return feed;
            }
            let rows: Vec<_> = std::mem::take(tools);
            let mut list = div()
                .ml(px(44.))
                .px(px(12.))
                .py(px(7.))
                .rounded(px(6.))
                .bg(c(ui::raised()))
                .border_1()
                .border_color(c(ui::line()))
                .flex()
                .flex_col()
                .gap(px(4.));
            // Long runs of steps fold, keeping the latest in view.
            let hidden = rows.len().saturating_sub(8);
            if hidden > 0 {
                list = list.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(c(ui::faint()))
                        .child(format!("{hidden} earlier steps")),
                );
            }
            for (n, (label, kind, state)) in rows.into_iter().skip(hidden).enumerate() {
                let (mark, col) = match state {
                    0 => ("…", ui::t_blue()),
                    2 => ("\u{21bb}", ui::muted()),
                    _ => ("\u{2713}", ui::t_green()),
                };
                let raw = label.clone();
                let label = super::store::plain_step(&label, state == 0);
                let tip = raw.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("step-{n}-{raw}")))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.5))
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                .build(window, cx)
                        })
                        .child(
                            Icon::new(tool_icon(&kind))
                                .size(px(13.))
                                .text_color(c(ui::muted())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.5))
                                .text_color(c(ui::body()))
                                .child(label),
                        )
                        .child(if state == 0 {
                            div()
                                .size(px(7.))
                                .mx(px(2.))
                                .rounded_full()
                                .bg(c(col))
                                .opacity(breathe)
                        } else {
                            div()
                                .text_color(c(col))
                                .font_weight(FontWeight::BOLD)
                                .child(mark)
                        }),
                );
            }
            feed.child(list)
        };
        if job.feed.is_empty() && running {
            feed = feed.child(note_line("Starting the engine and reading the folder…"));
        }
        let last_text = job
            .feed
            .iter()
            .rposition(|i| matches!(i, Item::Text(t) if !t.trim().is_empty()));
        let dir = job.dir().to_string();
        for (at, item) in job.feed.iter().enumerate() {
            if let Item::Tool {
                label, kind, state, ..
            } = item
            {
                tools.push((label.clone(), kind.clone(), *state));
                continue;
            }
            feed = flush(feed, &mut tools);
            // Every turn after the first can be undone from where it began.
            if let Some(k) = super::rewind::mark_at(job, at) {
                let id = job.id;
                feed = feed.child(
                    div().flex().justify_end().child(
                        ui::ghost_i(
                            SharedString::from(format!("rewind-{id}-{k}")),
                            IconName::Undo2,
                            "Rewind to before this",
                        )
                        .h(px(24.))
                        .px(px(8.))
                        .text_size(px(12.))
                        .text_color(c(ui::muted()))
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "Put the files back as they were before this message, and drop what came after",
                            )
                            .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.rewind_ask = Some((id, k));
                            this.live_scroll.scroll_to_bottom();
                            cx.notify();
                        })),
                    ),
                );
            }
            feed = match item {
                Item::Text(t) if super::rich::tool_chatter(t) => feed,
                Item::Text(t) => {
                    let t = plain(t);
                    if t.is_empty() {
                        feed
                    } else {
                        let lead = last_by != Some(Some(who));
                        last_by = Some(Some(who));
                        let face = self.avatar(who, 30.).into_any_element();
                        // Still on the server: its links and commands are there.
                        self.rich_there = (!job.remote_on.is_empty() && !job.remote_dir.is_empty())
                            .then(|| job.remote_dir.clone());
                        let text = self.rich(format!("feed-{}-{at}", job.id), &t, &dir, cx);
                        self.rich_there = None;
                        let when = if Some(at) == last_text {
                            job.ended.unwrap_or(job.started)
                        } else {
                            job.started
                        };
                        feed.child(super::channel::line(
                            lead.then(|| (face, super::channel::byline(who.name(), when))),
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(14.))
                                .line_height(px(21.))
                                .text_color(c(ui::ink()))
                                .child(text),
                        ))
                    }
                }
                Item::You(t) => {
                    let lead = last_by != Some(None);
                    last_by = Some(None);
                    feed.child(super::channel::mine(t.clone(), job.started, lead))
                }
                Item::Note(t) => feed.child(note_line(t)),
                Item::By(by, t) => {
                    last_by = Some(Some(*by));
                    let face = self.avatar(*by, 16.);
                    feed.child(
                        div()
                            .pl(px(44.))
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .text_size(px(12.5))
                            .line_height(px(19.))
                            .child(div().pt(px(1.5)).flex_none().child(face))
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(by.name()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_color(c(ui::body()))
                                    .child(t.clone()),
                            ),
                    )
                }
                Item::Error(t) => feed.child(
                    div()
                        .ml(px(44.))
                        .px(px(14.))
                        .py(px(12.))
                        .rounded(px(5.))
                        .bg(ca(ui::RED, 0.07))
                        .border_1()
                        .border_color(ca(ui::RED, 0.25))
                        .text_size(px(13.5))
                        .line_height(px(20.))
                        .text_color(c(ui::t_red()))
                        .child(t.clone()),
                ),
                Item::Tool { .. } => feed,
                Item::Picture {
                    file,
                    title,
                    caption,
                } => feed.child(
                    div()
                        .pl(px(44.))
                        .child(self.picture_card(job.id, at, file, title, caption, cx)),
                ),
                Item::Directions {
                    title,
                    options,
                    pick,
                    chosen,
                } => feed.child(div().pl(px(44.)).child(self.directions_card(
                    job.id,
                    at,
                    (title, options),
                    (*pick, *chosen),
                    cx,
                ))),
            };
        }
        feed = flush(feed, &mut tools);
        if let Some(card) = self.trouble_card(job, cx) {
            feed = feed.child(card);
        }
        if job.status == Status::Review && !job.held.is_empty() && !job.belt_ok {
            let card = self.belt_box(job, cx);
            feed = feed.child(div().pl(px(44.)).child(card));
        }
        if let Some(card) = self.in_the_way_card(job, cx) {
            feed = feed.child(card.ml(px(40.)).mr_0());
        }
        if let Some(card) = self.clash_box(job, cx) {
            feed = feed.child(div().pl(px(44.)).child(card));
        }
        if let Some(card) = self.lesson_card(job, cx) {
            feed = feed.child(card.ml(px(40.)).mr_0());
        }
        // A rewind waiting for its OK sits last, where the eye already is.
        if let Some((_, k)) = self.rewind_ask.filter(|(id, _)| *id == job.id) {
            feed = feed.child(self.rewind_confirm(job, k, cx));
        }
        // The review itself is the workspace's Changes tab, beside this.
        if running {
            let dots = typing(self.now());
            feed = feed.child(
                div()
                    .ml(px(40.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(dots)
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(ui::muted()))
                            .child(job.doing()),
                    ),
            );
        }
        // Keep the newest line in view while it runs.
        let len = job.feed.len()
            + job
                .feed
                .last()
                .map(|i| match i {
                    Item::Text(t) => t.len() / 80,
                    _ => 0,
                })
                .unwrap_or(0);
        if len != self.live_len {
            self.live_len = len;
            self.live_scroll.scroll_to_bottom();
        }
        let composer = self.live_composer(job, cx);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .children(ui::feed_grid())
                    .child(
                        div()
                            .id("feed")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.live_scroll)
                            .child(feed),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(18.))
                    .pb(px(16.))
                    .pt(px(6.))
                    .children(self.queued_strip(job, cx))
                    .child(composer),
            )
    }

    /// The job's own box, the design's composer: who it goes to, Ask or Do,
    /// where it runs, and the send button.
    fn live_composer(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let who = job.bot;
        let face = self.avatar(who, 15.);
        let ask = self.live_ask;
        let sep = || div().w(px(1.)).h(px(16.)).mx(px(2.)).bg(c(ui::line()));
        let mut hows = div().flex().items_center().gap(px(2.));
        for (i, (name, on)) in [("Ask", ask), ("Do", !ask)].into_iter().enumerate() {
            hows = hows.child(
                div()
                    .id(("live-how", i))
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
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(if i == 0 {
                            "Ask: it answers without changing files"
                        } else {
                            "Do: it carries on and changes what you say"
                        })
                        .build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.live_ask = i == 0;
                        cx.notify();
                    })),
            );
        }
        let place = if job.remote_on.is_empty() {
            super::machines::here_name().to_string()
        } else {
            job.remote_on.clone()
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .rounded(px(7.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line_hi()))
            .child(
                div()
                    .px(px(16.))
                    .pt(px(13.))
                    .pb(px(6.))
                    .text_size(px(14.))
                    .child(Input::new(&self.live_input).appearance(false)),
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
                            .child(format!("To {}", who.name())),
                    )
                    .child(sep())
                    .child(hows)
                    .child(sep())
                    .child(
                        div()
                            .h(px(26.))
                            .px(px(9.))
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child(Icon::new(IconName::Server).size(px(13.)))
                            .child(place),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("live-send")
                            .h(px(30.))
                            .w(px(32.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .bg(c(ui::pick_fill()))
                            .cursor_pointer()
                            .hover(|s| s.bg(c(ui::fill_hover())))
                            .child(
                                Icon::new(IconName::ArrowUp)
                                    .size(px(15.))
                                    .text_color(c(ui::pick_on())),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send_live(window, cx);
                            })),
                    ),
            )
    }

    /// What you've added while the job works, waiting for its next turn, each
    /// with a cross to take it back.
    fn queued_strip(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if job.queued.is_empty() {
            return None;
        }
        let id = job.id;
        let mut list = div().flex().flex_col().gap(px(4.)).mb(px(8.));
        list = list.child(
            div()
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child(format!("Goes to {} when this turn ends", job.bot.name())),
        );
        for (n, text) in job.queued.iter().enumerate() {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(10.))
                    .bg(c(ui::hover()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(c(ui::ink()))
                            .child(text.clone()),
                    )
                    .child(
                        div()
                            .id(("unqueue", n))
                            .cursor_pointer()
                            .text_color(c(ui::faint()))
                            .hover(|s| s.text_color(c(ui::ink())))
                            .child(Icon::new(IconName::Close).size(px(12.)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.crew.unqueue(id, n);
                                cx.notify();
                            })),
                    ),
            );
        }
        Some(list)
    }

    /// A job's details for the right sidebar: where it stands, its plan, the
    /// files it changed and the bot's other jobs.
    /// The job's conversation and what Squad runs for it, without its
    /// terminal: the workspace's left column.
    pub(super) fn live_talk(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let runs = self.job_runs(job.id, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.live_main(job, cx)),
            )
            .when(!runs.is_empty(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .px(px(16.))
                        .pb(px(10.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(runs),
                )
            })
    }

    pub(super) fn job_details(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let mut plan = div().flex().flex_col().gap(px(10.));
        if job.plan.is_empty() {
            plan = plan.child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .line_height(px(18.))
                    .child(if job.status == Status::Running {
                        "Its steps show here when it writes a plan."
                    } else {
                        "No plan for this one: it went straight at it."
                    }),
            );
        }
        for (text, mark) in &job.plan {
            let (dot, col, weight) = match mark {
                2 => ("✓", ui::GREEN, FontWeight::NORMAL),
                1 => ("●", job.bot.accent(), FontWeight::SEMIBOLD),
                _ => ("○", ui::faint(), FontWeight::NORMAL),
            };
            plan = plan.child(
                div()
                    .flex()
                    .gap(px(10.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .child(div().w(px(14.)).flex_none().text_color(c(col)).child(dot))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(weight)
                            .text_color(c(if *mark == 0 { ui::muted() } else { ui::ink() }))
                            .child(text.clone()),
                    ),
            );
        }
        // What it changed on its branch, once known; else the files it touched.
        let touched: Vec<String> = if job.changes.is_empty() {
            job.files.clone()
        } else {
            job.changes.iter().map(|c| c.path.clone()).collect()
        };
        let mut files = div().flex().flex_col().gap(px(6.));
        if touched.is_empty() {
            files = files.child(div().text_size(px(12.5)).text_color(c(ui::faint())).child(
                if job.mode.writes() {
                    "Nothing changed yet."
                } else {
                    "This kind of job only reads. Nothing changes."
                },
            ));
        }
        let dir = job.dir().to_string();
        for (n, f) in touched.iter().take(14).enumerate() {
            let counts = job
                .changes
                .iter()
                .find(|ch| &ch.path == f)
                .map(|ch| (ch.added, ch.removed));
            let name = f.rsplit('/').next().unwrap_or(f).to_string();
            let folder = f
                .strip_suffix(name.as_str())
                .unwrap_or("")
                .trim_end_matches('/')
                .to_string();
            let act = super::rich::Act::File(f.clone());
            let dir = dir.clone();
            files = files.child(
                div()
                    .id(("side-file", n))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mx(px(-8.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .map(|d| super::motion::hover_fill(d, None, ui::hover()))
                    .child(
                        Icon::new(IconName::FileText)
                            .size(px(13.))
                            .text_color(c(ui::muted())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::ink()))
                                    .child(name),
                            )
                            .when(!folder.is_empty(), |d| {
                                d.child(
                                    div()
                                        .truncate()
                                        .text_size(px(11.))
                                        .text_color(c(ui::faint()))
                                        .child(folder),
                                )
                            }),
                    )
                    .when_some(counts, |d, (a, r)| {
                        d.child(
                            div()
                                .flex()
                                .gap(px(4.))
                                .font_family(ui::MONO)
                                .text_size(px(11.))
                                .child(div().text_color(c(ui::add())).child(format!("+{a}")))
                                .child(
                                    div().text_color(c(ui::del())).child(format!("\u{2212}{r}")),
                                ),
                        )
                    })
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.act(&act, &dir, window, cx)),
                    ),
            );
        }
        if touched.len() > 14 {
            files = files.child(note_line(&format!("and {} more", touched.len() - 14)));
        }
        let others: Vec<_> = self
            .crew
            .store
            .listed()
            .into_iter()
            .filter(|j| j.id != job.id && j.bot == job.bot && (j.race == 0 || j.race != job.race))
            .take(3)
            .map(|j| (j.id, j.label().to_string(), j.status))
            .collect();
        let has_more = !others.is_empty();
        let mut more = div().flex().flex_col().gap(px(4.));
        for (id, title, status) in others {
            more = more.child(
                div()
                    .id(("other-job", id as usize))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mx(px(-8.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(9.))
                    .cursor_pointer()
                    .map(|d| super::motion::hover_fill(d, None, ui::hover()))
                    .child(
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(c(status_color(status))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .child(title),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Live(id), cx))),
            );
        }
        // "4s · 1 turn · $0.07 · 59% from cache" as small tiles.
        let stats: Vec<String> = if job.summary.is_empty() {
            vec![store::span(job.seconds()).replace(' ', "\u{a0}")]
        } else {
            job.summary
                .split(" \u{b7} ")
                .map(|p| p.trim().to_string())
                // An engine's "260s" reads as "4m 20s", like everywhere else.
                .map(
                    |p| match p.strip_suffix('s').and_then(|n| n.parse::<u64>().ok()) {
                        Some(secs) => store::span(secs).replace(' ', "\u{a0}"),
                        None => p,
                    },
                )
                .collect()
        };
        let mut stats_grid = div().flex().flex_wrap().gap(px(6.));
        for st in &stats {
            let (num, what) = match st.split_once(' ') {
                Some((n, w)) => (n.to_string(), w.to_string()),
                None if st.starts_with('$') => (st.clone(), "cost".to_string()),
                None => (st.clone(), "time".to_string()),
            };
            stats_grid = stats_grid.child(
                div()
                    .min_w(px(78.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(9.))
                    .bg(c(ui::well()))
                    .border_1()
                    .border_color(c(ui::line2()))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(num.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(c(ui::muted()))
                            .child(what),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            // The header already says the status; this says how it went.
            .child(section(
                "Summary",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(!stats.is_empty(), |d| d.child(stats_grid))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .text_color(c(ui::body()))
                            .child(match job.status {
                                Status::Running => format!(
                                    "Started {} in {}. You can leave this screen; it keeps going.",
                                    ago_phrase(job.started),
                                    job.folder.name
                                ),
                                Status::Done => format!(
                                    "Finished {} after {}. Read its last note, then try it yourself.",
                                    ago_phrase(job.ended.unwrap_or(job.started)),
                                    store::span(job.seconds())
                                ),
                                Status::Review => format!(
                                    "Finished {} on its own branch. Your checkout is untouched until you approve.",
                                    ago_phrase(job.ended.unwrap_or(job.started)),
                                ),
                                Status::Shipped => job.shipped.clone(),
                                Status::Shipping => if job.pr.is_empty() {
                                    "Shipper is pushing the branch and opening the pull request.".to_string()
                                } else {
                                    format!("Pull request open. {}", job.pr_line)
                                },
                                Status::Discarded => "Its changes were thrown away; your checkout never changed.".to_string(),
                                Status::Stopped => format!(
                                    "You stopped it after {}. It can carry on from where it was.",
                                    store::span(job.seconds())
                                ),
                                _ => format!(
                                    "Ran for {}. Its last note says what went wrong; ask a follow-up to carry on.",
                                    store::span(job.seconds())
                                ),
                            }),
                    ),
            ))
            .when(!job.plan.is_empty(), |d| d.child(section("Plan", plan)))
            // The review card lists them while there's a review; a job that
            // only reads has none to list.
            .when(
                job.status != Status::Review && !(touched.is_empty() && !job.mode.writes()),
                |d| d.child(section("Files changed", files)),
            )
            .when(has_more, |d| {
                d.child(section(&format!("{}'s other jobs", job.bot.name()), more))
            })
            .children(self.rewind_points(job, cx))
    }

    /// Asks before a rewind: what goes back and what leaves the feed, with
    /// Cancel, Rewind and edit, and Rewind.
    fn rewind_confirm(&mut self, job: &Job, k: usize, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let at = job.marks.get(k).map_or(job.feed.len(), |m| m.feed_at);
        let later = job.marks.len().saturating_sub(k);
        let gone = job.feed[at.min(job.feed.len())..]
            .iter()
            .filter(|i| matches!(i, Item::You(_) | Item::Text(_)))
            .count();
        let line = |text: String| {
            div()
                .flex()
                .gap(px(8.))
                .text_size(px(12.5))
                .line_height(px(18.))
                .text_color(c(ui::body()))
                .child(
                    div()
                        .flex_none()
                        .text_color(c(ui::faint()))
                        .child("\u{2022}"),
                )
                .child(div().flex_1().min_w_0().child(text))
        };
        div()
            .ml(px(40.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(12.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::pick()))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(Icon::new(IconName::Undo2).size(px(14.)))
                    .child("Rewind to before this message?"),
            )
            .child(line(format!(
                "The files on its branch go back to how they were then. {} later {} undone.",
                later,
                if later == 1 { "turn is" } else { "turns are" }
            )))
            .child(line(format!(
                "{gone} {} after it leave the feed, and {} forgets them.",
                if gone == 1 { "message" } else { "messages" },
                job.bot.name()
            )))
            .child(line(
                "Only the job's own branch changes. Your folder isn't touched.".to_string(),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_1())
                    .child(
                        ui::ghost(("rewind-cancel", id as usize), "Cancel")
                            .h(px(28.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.rewind_ask = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::ghost(("rewind-edit", id as usize), "Rewind and edit")
                            .h(px(28.))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.rewind_and_edit(id, k, window, cx)
                            })),
                    )
                    .child(
                        ui::solid(("rewind-go", id as usize), "Rewind")
                            .h(px(28.))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.rewind_job(id, k, cx)),
                            ),
                    ),
            )
    }

    /// Every point the job can go back to, newest first, each with Rewind.
    /// Nothing while it can't be rewound.
    fn rewind_points(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        let points = super::rewind::points(job);
        if points.is_empty() {
            return None;
        }
        let id = job.id;
        let mut list = div().flex().flex_col();
        for k in points.into_iter().rev() {
            let m = &job.marks[k];
            list = list.child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(c(ui::line2()))
                    .child(
                        div()
                            .w(px(64.))
                            .flex_none()
                            .font_family(ui::MONO)
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child(store::ago(m.at)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(c(ui::body()))
                            .child(format!("Before \u{201c}{}\u{201d}", m.said)),
                    )
                    .child(
                        ui::ghost(("rewind-point", id as usize * 1000 + k), "Rewind")
                            .h(px(24.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.rewind_ask = Some((id, k));
                                this.go(Place::Live(id), cx);
                                this.live_scroll.scroll_to_bottom();
                                cx.notify();
                            })),
                    ),
            );
        }
        Some(section("Rewind points", list))
    }

    /// A centred message for a place with nothing in it.
    pub(super) fn empty_place(
        &mut self,
        title: &'static str,
        line: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let keeper = self.bot(
            super::data::Member::Keeper,
            Mood::Asleep,
            160.,
            Framing::Full,
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(keeper)
            .child(
                div()
                    .text_size(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(c(ui::muted()))
                    .child(line),
            )
            // Nothing anywhere yet: point back to where a first job starts.
            .when(self.crew.store.jobs.is_empty(), |d| {
                d.child(
                    ui::solid("empty-start", "Give the crew a first job")
                        .mt(px(14.))
                        .on_click(cx.listener(|this, _, _, cx| this.go(super::Place::Today, cx))),
                )
            })
            .into_any_element()
    }
}

fn ago_phrase(at: u64) -> String {
    match store::ago(at).as_str() {
        "just now" => "just now".into(),
        a => format!("{a} ago"),
    }
}

pub(super) fn status_color(s: Status) -> u32 {
    match s {
        Status::Running => ui::ORANGE,
        Status::Done | Status::Shipped => ui::GREEN,
        Status::Failed => ui::RED,
        Status::Discarded => ui::faint(),
        _ => ui::AMBER,
    }
}

/// One block of the side column: its heading, then its body, with a hairline
/// under it to part it from the next.
pub(super) fn section(title: &str, body: Div) -> Div {
    div()
        .py(px(18.))
        .border_b_1()
        .border_color(c(ui::line2()))
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(ui::heading(title, None, None))
        .child(body)
}

impl Shell {
    /// A picture the bot showed: a screenshot, mockup or diagram, with its name and
    /// what to notice. Clicking it opens it full size.
    fn picture_card(
        &self,
        job: u64,
        at: usize,
        file: &str,
        title: &str,
        caption: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = std::path::PathBuf::from(file);
        let open = path.clone();
        ui::card()
            .max_w(px(640.))
            .overflow_hidden()
            .when(!title.is_empty(), |d| {
                d.child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .border_b_1()
                        .border_color(c(ui::line()))
                        .child(
                            Icon::new(IconName::Image)
                                .size(px(14.))
                                .text_color(c(ui::muted())),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(ui::ink()))
                                .child(title.to_string()),
                        ),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!("picture-{job}-{at}")))
                    .p(px(10.))
                    .bg(c(ui::ground()))
                    .cursor_pointer()
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Open it full size")
                            .build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_file(open.clone(), Some(job), window, cx);
                    }))
                    .child(
                        img(path)
                            .w_full()
                            .h(px(320.))
                            .object_fit(ObjectFit::Contain),
                    ),
            )
            .when(!caption.is_empty(), |d| {
                d.child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .border_t_1()
                        .border_color(c(ui::line()))
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(c(ui::body()))
                        .child(caption.to_string()),
                )
            })
            .into_any_element()
    }

    /// Two to four looks to pick from, side by side. The bot's pick is marked, and
    /// once the person picks, that one is marked instead and the rest are dimmed.
    fn directions_card(
        &self,
        job: u64,
        at: usize,
        (title, options): (&str, &[store::Direction]),
        (pick, chosen): (usize, Option<usize>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bot_pick = |k: usize| (k == pick).then_some("BOT'S PICK");
        let mut row = div().p(px(12.)).flex().gap(px(10.));
        for (k, o) in options.iter().enumerate() {
            let letter = (b'A' + k as u8) as char;
            let on = chosen.map_or(k == pick, |ch| ch == k);
            let dim = chosen.is_some_and(|ch| ch != k);
            let path = std::path::PathBuf::from(&o.file);
            let open = path.clone();
            let label = match chosen {
                Some(ch) if ch == k => Some("PICKED"),
                Some(_) => None,
                None => bot_pick(k),
            };
            let tile = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .rounded(px(4.))
                .border_1()
                .border_color(c(if on { ui::pick() } else { ui::line() }))
                .when(on, |d| d.border_l_4().border_color(c(ui::accent_bar())))
                .when(dim, |d| d.opacity(0.55))
                .overflow_hidden()
                .child(
                    div()
                        .id(SharedString::from(format!("direction-pic-{job}-{at}-{k}")))
                        .h(px(220.))
                        .p(px(8.))
                        .bg(c(ui::ground()))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_file(open.clone(), Some(job), window, cx);
                        }))
                        .child(img(path).size_full().object_fit(ObjectFit::Contain)),
                )
                .child(
                    div()
                        .p(px(10.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(c(ui::ink()))
                                        .child(format!("{letter} · {}", o.name)),
                                )
                                .when_some(label, |d, l| {
                                    d.child(
                                        div()
                                            .px(px(5.))
                                            .rounded(px(3.))
                                            .bg(c(ui::pick_fill()))
                                            .text_color(c(ui::pick_on()))
                                            .font_family(ui::MONO)
                                            .text_size(px(10.))
                                            .child(l),
                                    )
                                }),
                        )
                        .when(!o.note.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(12.5))
                                    .line_height(px(18.))
                                    .text_color(c(ui::body()))
                                    .child(o.note.clone()),
                            )
                        })
                        .when(chosen.is_none(), |d| {
                            let kind = if k == pick {
                                ui::Btn::Primary
                            } else {
                                ui::Btn::Plain
                            };
                            d.child(
                                ui::dbtn(
                                    SharedString::from(format!("direction-{job}-{at}-{k}")),
                                    format!("Use {}", o.name),
                                    kind,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.use_direction(job, at, k, cx);
                                    },
                                )),
                            )
                        }),
                );
            row = row.child(tile);
        }
        let head = if title.is_empty() {
            "Pick a direction".to_string()
        } else {
            format!("Pick a direction · {title}")
        };
        ui::card()
            .max_w(px(900.))
            .child(
                div()
                    .px(px(14.))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(c(ui::line()))
                    .child(Icon::new(IconName::Palette).size(px(14.)).text_color(c(ui::muted())))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(head),
                    ),
            )
            .child(row)
            .when(chosen.is_none(), |d| {
                d.child(
                    div()
                        .px(px(14.))
                        .pb(px(10.))
                        .text_size(px(12.))
                        .text_color(c(ui::muted()))
                        .child("Or say what to mix in the box below, like \u{201c}A, with B's rounder cards\u{201d}."),
                )
            })
            .into_any_element()
    }

    /// The person picked direction `k` on the card at `at` in a job's feed: mark it
    /// and tell the bot.
    fn use_direction(&mut self, job: u64, at: usize, k: usize, cx: &mut Context<Self>) {
        let Some(text) = self.crew.pick_direction(job, at, k) else {
            return;
        };
        self.follow_up(job, text, cx);
    }
}

pub(super) fn note_line(text: &str) -> Div {
    div()
        .ml(px(40.))
        .text_size(px(12.5))
        .text_color(c(ui::muted()))
        .italic()
        .child(text.to_string())
}

/// Three dots bouncing in turn, for a bot that is mid-thought.
pub(super) fn typing(now: f64) -> Div {
    let mut dots = div()
        .px(px(14.))
        .h(px(34.))
        .rounded(px(14.))
        .bg(ca(ui::panel(), 0.85))
        .border_1()
        .border_color(c(ui::line()))
        .flex()
        .items_center()
        .gap(px(5.));
    for i in 0..3 {
        let p = ((now * 1.6 - i as f64 * 0.18).rem_euclid(1.0) * std::f64::consts::TAU)
            .sin()
            .max(0.0);
        dots = dots.child(
            div()
                .size(px(7.))
                .rounded_full()
                .relative()
                .top(px(-3. * p as f32))
                .bg(ca(ui::BLUE, 0.45 + 0.55 * p as f32)),
        );
    }
    dots
}
