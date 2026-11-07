//! Live: one job, as the bot works. What it says and does on the left, as it
//! happens; its plan, the files it touched and where things stand on the right.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use runo_holo::{Framing, Mood};

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
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.live_main(&job, cx))
                    .child(self.live_side(&job, cx)),
            )
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
        let hero = self.bot(who, mood, 64., Framing::Bust);
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
                        Some(format!("Part {} of {}", k + 1, g.steps.len()))
                    })
                    .unwrap_or_else(|| "Goal".into());
                d.child(
                    ui::ghost("to-goal", label)
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
                        ui::ghost_i("race-it", IconName::Swords, "Race it")
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
                    && !matches!(job.status, Status::Done | Status::Review | Status::Shipped),
                |d| {
                    let id = job.id;
                    d.child(
                        ui::ghost_i("retry", IconName::RotateCcw, "Try again")
                            .on_click(cx.listener(move |this, _, _, cx| this.retry_job(id, cx))),
                    )
                },
            )
            .when(running, |d| {
                let id = job.id;
                d.child(
                    ui::ghost_i("stop", IconName::CircleStop, "Stop")
                        .text_color(c(ui::RED))
                        .on_click(cx.listener(move |this, _, _, cx| this.stop_job(id, cx))),
                )
            })
            .map(|d| {
                let id = job.id;
                let open = self.term_open == Some(id);
                d.child(
                    ui::ghost_i(
                        "terminal",
                        IconName::SquareTerminal,
                        if open { "Hide terminal" } else { "Terminal" },
                    )
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "A shell in this job's own branch (⌃`)",
                        )
                        .build(window, cx)
                    })
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.toggle_term(id, window, cx)),
                    ),
                )
            })
            .when(!running, |d| {
                let path = job.folder.path.clone();
                d.child(
                    ui::ghost_i("open-folder", IconName::FolderOpen, "Open folder").on_click(
                        cx.listener(move |_, _, _, cx| cx.reveal_path(std::path::Path::new(&path))),
                    ),
                )
            });
        let fraction = match (running, progress) {
            (true, Some((d, n))) => {
                Some(self.glide_bar(job.id, (d as f32 + 0.5) / n.max(1) as f32))
            }
            _ => None,
        };
        let header = ui::chrome(div())
            .flex_none()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(c(ui::line()))
            .bg(linear_gradient(
                180.,
                linear_color_stop(ca(accent, 0.07), 0.0),
                linear_color_stop(ca(ui::panel(), 0.0), 1.0),
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(14.))
                    .px(px(28.))
                    .pt(px(if cfg!(target_os = "macos") { 16. } else { 12. }))
                    .pb(px(14.))
                    .child(div().flex_none().size(px(64.)).child(hero))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(320.))
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("job-title")
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(20.))
                                            .line_height(px(26.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(c(ui::ink()))
                                            .child(job.label().to_string())
                                            .tooltip({
                                                let ask = job.title.clone();
                                                move |window, cx| {
                                                    gpui_kit::component::tooltip::Tooltip::new(
                                                        ask.clone(),
                                                    )
                                                    .build(window, cx)
                                                }
                                            }),
                                    )
                                    .child(ui::badge(tone, word)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(12.5))
                                    .text_color(c(ui::muted()))
                                    .child(
                                        div()
                                            .id("job-bot-dm")
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(c(ui::ink()))
                                            .cursor_pointer()
                                            .hover(|s| s.underline())
                                            .child(who.name())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.go(Place::Chat(who), cx)
                                            })),
                                    )
                                    .child(div().text_color(c(ui::faint())).child("\u{b7}"))
                                    .child(facts.join(" \u{b7} ")),
                            ),
                    )
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
        let mut feed = div().flex().flex_col().gap(px(10.)).px(px(30.)).py(px(20.));
        let mut tools: Vec<(String, String, u8)> = Vec::new();
        // A step in progress shows a dot that breathes.
        let breathe =
            0.35 + 0.65 * runo_holo::anim::alternate(self.now(), 0.9, runo_holo::anim::SOFT) as f32;
        let flush = |feed: Div, tools: &mut Vec<(String, String, u8)>| -> Div {
            if tools.is_empty() {
                return feed;
            }
            let rows: Vec<_> = std::mem::take(tools);
            let mut list = div()
                .ml(px(40.))
                .px(px(14.))
                .py(px(8.))
                .rounded(px(14.))
                .bg(ca(ui::panel(), 0.6))
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
            feed = match item {
                Item::Text(t) if super::rich::tool_chatter(t) => feed,
                Item::Text(t) => {
                    let t = plain(t);
                    if t.is_empty() {
                        feed
                    } else {
                        let face = self.avatar(who, 32.);
                        let kind = (Some(at) == last_text && job.status != Status::Running)
                            .then(|| super::rich::Kind::of(&t, job.status));
                        let text = self.rich(format!("feed-{}-{at}", job.id), &t, &dir, cx);
                        feed.child(
                            div().flex().gap(px(12.)).child(face).child(
                                super::rich::bubble(kind, text)
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .line_height(px(21.))
                                    .text_color(c(ui::body())),
                            ),
                        )
                    }
                }
                Item::You(t) => feed.child(
                    div().flex().justify_end().child(
                        div()
                            .max_w(px(620.))
                            .px(px(16.))
                            .py(px(10.))
                            .rounded(px(16.))
                            .bg(c(ui::fill()))
                            .text_color(c(ui::on_fill()))
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .child(t.clone()),
                    ),
                ),
                Item::Note(t) => feed.child(note_line(t)),
                Item::By(by, t) => {
                    let face = self.avatar(*by, 18.);
                    feed.child(
                        div()
                            .ml(px(40.))
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .text_size(px(12.5))
                            .line_height(px(18.))
                            .child(face)
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(by.accent()))
                                    .child(by.name()),
                            )
                            .child(div().flex_1().text_color(c(ui::body())).child(t.clone())),
                    )
                }
                Item::Error(t) => feed.child(
                    div()
                        .ml(px(40.))
                        .px(px(16.))
                        .py(px(12.))
                        .rounded(px(14.))
                        .bg(ca(ui::RED, 0.07))
                        .border_1()
                        .border_color(ca(ui::RED, 0.25))
                        .text_size(px(13.5))
                        .line_height(px(20.))
                        .text_color(c(ui::t_red()))
                        .child(t.clone()),
                ),
                Item::Tool { .. } => feed,
            };
        }
        feed = flush(feed, &mut tools);
        if let Some(card) = self.trouble_card(job, cx) {
            feed = feed.child(card);
        }
        if let Some(card) = self.review_card(job, cx) {
            feed = feed.child(card);
        }
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
        let input_hint = if running {
            "It's working. You can add to the job when it finishes."
        } else if job.status == Status::Review {
            "Send it back: say what to change…"
        } else {
            "Ask a follow-up or ask for a change…"
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("feed")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.live_scroll)
                    .child(feed),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(30.))
                    .pb(px(super::DOCK_ROOM))
                    .pt(px(6.))
                    .child(
                        ui::card()
                            .rounded(px(18.))
                            .px(px(16.))
                            .py(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .when(running, |d| d.opacity(0.6))
                            .child(
                                div().flex_1().text_size(px(14.)).child(
                                    Input::new(&self.live_input)
                                        .appearance(false)
                                        .disabled(running),
                                ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c(ui::faint()))
                                    .child(input_hint),
                            ),
                    ),
            )
    }

    fn live_side(&mut self, job: &Job, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .hover(|s| s.bg(c(ui::hover())))
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
                    .hover(|s| s.bg(c(ui::hover())))
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
            vec![store::span(job.seconds())]
        } else {
            job.summary
                .split(" \u{b7} ")
                .map(|p| p.trim().to_string())
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
            .id("live-side")
            .w(px(320.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(c(ui::line2()))
            .px(px(24.))
            .pt(px(if cfg!(target_os = "macos") { 16. } else { 4. }))
            .pb(px(24. + super::DOCK_ROOM))
            .flex()
            .flex_col()
            .child(section(
                "Status",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child({
                        let (word, tone) = super::home::status_of(job);
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(12.))
                            .py(px(9.))
                            .rounded(px(10.))
                            .bg(c(tone.1))
                            .border_1()
                            .border_color(c(tone.2))
                            .child(div().size(px(8.)).rounded_full().bg(c(tone.0)))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(tone.0))
                                    .child(word),
                            )
                    })
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
            .child(section("Files changed", files))
            .when(has_more, |d| {
                d.child(section(&format!("{}'s other jobs", job.bot.name()), more))
            })
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
fn section(title: &str, body: Div) -> Div {
    div()
        .py(px(20.))
        .border_b_1()
        .border_color(c(ui::line2()))
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(ui::heading(title, None, None))
        .child(body)
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
