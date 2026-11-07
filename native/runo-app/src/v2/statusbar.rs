//! The status bar along the bottom of the page, and the engines popover it
//! opens.
//!
//! One item on the right sums it up: a dot for each engine in play today
//! (Claude Code, Codex, Gemini or any other) for whether it's ready, a small
//! meter for the plan window closest to its limit, and what the crew cost and
//! used today. Clicking anywhere on it opens the popover: for each engine, its
//! plan windows with what's left and when they reset, and today's cost, tokens
//! and jobs.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::home::cost;
use super::store::{self, Store};
use super::ui::{self, c, ca};
use super::{Pane, Place, Shell};

/// How many engines the bar names before the popover has to show the rest.
const BAR_ENGINES: usize = 4;

/// One engine's day.
struct Day {
    /// Index into `agent::PROVIDERS`.
    i: usize,
    ready: bool,
    spend: f64,
    tokens: u64,
    jobs: usize,
    usage: Option<crate::usage::Usage>,
}

/// The engines in play today: the default one first, then every engine a job
/// ran on today, in the order they first ran.
fn engines_today(st: &crate::settings::AppSettings, store: &Store) -> Vec<Day> {
    let today = store::now() / 86_400;
    let mut order = vec![super::engine::index_of(st, "")];
    let todays: Vec<_> = store
        .jobs
        .iter()
        .filter(|j| j.started / 86_400 == today)
        .collect();
    for j in &todays {
        if let Some(i) = crate::agent::PROVIDERS.iter().position(|p| p.id == j.agent) {
            if !order.contains(&i) {
                order.push(i);
            }
        }
    }
    order
        .into_iter()
        .map(|i| {
            let id = crate::agent::PROVIDERS[i].id;
            let mine = todays.iter().filter(|j| j.agent == id);
            let (mut spend, mut tokens, mut jobs) = (0.0, 0, 0);
            for j in mine {
                spend += cost(&j.summary).unwrap_or(0.);
                tokens += j.tokens;
                jobs += 1;
            }
            Day {
                i,
                ready: super::engine::ready(st, i),
                spend,
                tokens,
                jobs,
                usage: crate::usage::get(id),
            }
        })
        .collect()
}

/// The colour for a share of a window used: blue, then amber near the limit,
/// red when it's used up.
fn used_color(used: f32) -> u32 {
    if used >= 100. {
        ui::RED
    } else if used >= 80. {
        ui::AMBER
    } else {
        ui::BLUE
    }
}

/// A small meter for a share used, `w` wide.
fn meter(used: f32, w: f32, h: f32) -> Div {
    div()
        .w(px(w))
        .h(px(h))
        .flex_none()
        .rounded_full()
        .bg(c(ui::line()))
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(c(used_color(used)))
                .w(relative((used / 100.).clamp(0.02, 1.))),
        )
}

/// The engines popover's width.
const USAGE_W: f32 = 360.;

/// The engines popover's left edge in a window `view` wide: its right edge
/// lines up with the status item at the end of the bar, wherever you clicked
/// it, and it never leaves the window.
fn popover_left(view: f32, w: f32) -> f32 {
    const MARGIN: f32 = 8.;
    const BAR_PAD: f32 = 10.;
    (view - BAR_PAD - w).max(MARGIN)
}

impl Shell {
    /// Opens or closes the engines popover.
    fn toggle_usage(&mut self, cx: &mut Context<Self>) {
        self.usage_open = !self.usage_open;
        // A fresh read of the plans each time it opens, unless one ran lately.
        if self.usage_open && !self.crew.store.demo {
            crate::usage::refresh(false);
        }
        cx.notify();
    }

    /// Where the engines popover's left edge goes: always in the same place,
    /// right above the status item.
    fn usage_left(&self) -> f32 {
        popover_left(self.view_w, USAGE_W)
    }

    /// The status bar along the bottom of the page.
    pub(super) fn statusbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let st = cx.global::<crate::settings::AppSettings>();
        let days = engines_today(st, &self.crew.store);
        let spend: f64 = days.iter().map(|d| d.spend).sum::<f64>() + 0.0;
        let tokens: u64 = days.iter().map(|d| d.tokens).sum();
        let open = self.usage_open;

        // One item on the right: a dot per engine (ready, near its limit, or
        // missing), the plan window closest to its limit, then today's cost.
        let mut dots = div().flex().items_center().gap(px(4.));
        let mut tight: Option<f32> = None;
        let mut missing = false;
        for d in days.iter().take(BAR_ENGINES) {
            let head = d.usage.as_ref().and_then(|u| u.headline().cloned());
            if let Some(w) = &head {
                tight = Some(tight.map_or(w.used, |t| t.max(w.used)));
            }
            missing |= !d.ready;
            let dot = if !d.ready {
                ui::RED
            } else if head.as_ref().is_some_and(|w| w.used >= 80.) {
                ui::AMBER
            } else {
                ui::GREEN
            };
            dots = dots.child(div().size(px(7.)).rounded_full().bg(c(dot)));
        }
        let names: Vec<String> = days
            .iter()
            .map(|d| {
                let p = &crate::agent::PROVIDERS[d.i];
                let head = d.usage.as_ref().and_then(|u| u.headline().cloned());
                match head {
                    Some(w) => format!("{} {:.0}% used", p.name, w.used),
                    None if !d.ready => format!("{} not found", p.name),
                    None => p.name.to_string(),
                }
            })
            .collect();
        let tip = format!("{}\nClick for usage details", names.join(" \u{b7} "));
        let single = (days.len() == 1).then(|| crate::agent::PROVIDERS[days[0].i].name);
        let engines = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .child(dots)
            .when_some(single, |d, name| d.child(name))
            .when(days.len() > BAR_ENGINES, |d| {
                d.child(
                    div()
                        .text_color(c(ui::faint()))
                        .child(format!("+{}", days.len() - BAR_ENGINES)),
                )
            })
            .map(|el| match tight {
                Some(used) => el.child(meter(used, 28., 4.)).child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(if used >= 80. {
                            ui::t_amber()
                        } else {
                            ui::faint()
                        }))
                        .child(format!("{used:.0}%")),
                ),
                None if missing && single.is_some() => {
                    el.child(div().text_color(c(ui::t_red())).child("not found"))
                }
                None => el,
            });

        let right = div()
            .id("status-today")
            .h(px(22.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded_full()
            .cursor_pointer()
            .map(|d| super::motion::hover_fill(d, None, ui::hover()))
            .hover(|s| s.text_color(c(ui::ink())))
            // Not while the popover is open: it would cover the popover's links.
            .when(!open, |d| {
                d.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
            })
            .child(engines)
            .child(div().size(px(3.)).rounded_full().bg(c(ui::faint())))
            .child(div().text_color(c(ui::faint())).child("Today"))
            .child(
                div()
                    .text_color(c(ui::body()))
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("${spend:.2}")),
            )
            .child(div().size(px(3.)).rounded_full().bg(c(ui::faint())))
            .child(format!("{} tokens", crate::health::token_count(tokens)))
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronUp
                })
                .size(px(12.))
                .text_color(c(ui::faint())),
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_usage(cx)));

        // The project's terminal: a toggle at the start of the bar, lit while
        // it's open, that opens it as a drawer just above.
        let term_on = self.term_open.is_some_and(super::term::is_project);
        let term = div()
            .id("status-term")
            .h(px(22.))
            .px(px(8.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .rounded(px(6.))
            .cursor_pointer()
            .map(|d| {
                if term_on {
                    d.bg(c(ui::ink())).text_color(c(ui::theme().bg & 0xff_ffff))
                } else {
                    d.hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                }
            })
            .child(Icon::new(IconName::SquareTerminal).size(px(13.)))
            .child("Terminal")
            .child(
                div()
                    .font_family(ui::MONO)
                    .text_size(px(10.))
                    .opacity(0.6)
                    .child(super::term::KEY),
            )
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("A shell in this project")
                    .build(window, cx)
            })
            .on_click(cx.listener(|this, _, window, cx| this.toggle_project_term(window, cx)));

        div()
            .h(px(30.))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(10.))
            .border_t_1()
            .border_color(c(ui::line2()))
            .text_size(px(11.5))
            .text_color(c(ui::muted()))
            .child(term)
            .child(div().flex_1())
            .child(right)
    }

    /// The engines popover, over the status bar: each engine's plan windows,
    /// what's left of them and when they reset, and its day.
    pub(super) fn usage_popover(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let st = cx.global::<crate::settings::AppSettings>();
        let days = engines_today(st, &self.crew.store);
        let now = crate::usage::now_secs();
        let spend: f64 = days.iter().map(|d| d.spend).sum::<f64>() + 0.0;
        let tokens: u64 = days.iter().map(|d| d.tokens).sum();
        let jobs: usize = days.iter().map(|d| d.jobs).sum();
        let reading = crate::usage::busy();

        // Today across every engine, as three figures.
        let figure = |label: &'static str, value: String| {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(c(ui::faint()))
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(value),
                )
        };
        let totals = div()
            .flex()
            .gap(px(12.))
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(c(ui::line2()))
            .child(figure("Spent today", format!("${spend:.2}")))
            .child(figure("Tokens", crate::health::token_count(tokens)))
            .child(figure("Jobs", jobs.to_string()));

        let mut list = div().flex().flex_col();
        for (k, d) in days.iter().enumerate() {
            let p = &crate::agent::PROVIDERS[d.i];
            let default = k == 0;
            let mut card = div()
                .px(px(16.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .when(k > 0, |el| el.border_t_1().border_color(c(ui::line2())))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().size(px(8.)).rounded_full().bg(c(if d.ready {
                            ui::GREEN
                        } else {
                            ui::RED
                        })))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(ui::ink()))
                                .child(p.name),
                        )
                        .when(default, |el| {
                            el.child(
                                div()
                                    .px(px(6.))
                                    .rounded(px(4.))
                                    .bg(ca(ui::RUNO, 0.18))
                                    .text_size(px(10.5))
                                    .text_color(c(ui::t_violet()))
                                    .child("Default"),
                            )
                        })
                        .child(div().flex_1())
                        .child(div().text_size(px(11.5)).text_color(c(ui::muted())).child(
                            format!(
                                "${:.2} · {} · {} job{}",
                                d.spend,
                                crate::health::token_count(d.tokens),
                                d.jobs,
                                if d.jobs == 1 { "" } else { "s" }
                            ),
                        )),
                );
            match d.usage.as_ref().map(|u| &u.reading) {
                Some(crate::usage::Reading::Limits(windows)) if !windows.is_empty() => {
                    for w in windows {
                        let left = (100. - w.used).max(0.);
                        let resets = w
                            .resets_at
                            .map(|at| format!(" · resets in {}", crate::usage::left(at, now)))
                            .unwrap_or_default();
                        card = card.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .text_size(px(11.5))
                                        .child(
                                            div()
                                                .flex_1()
                                                .text_color(c(ui::body()))
                                                .child(format!("{} window", w.label)),
                                        )
                                        .child(
                                            div()
                                                .text_color(c(if w.used >= 80. {
                                                    ui::t_amber()
                                                } else {
                                                    ui::muted()
                                                }))
                                                .child(format!("{left:.0}% left{resets}")),
                                        ),
                                )
                                .child(meter(w.used, 296., 5.).w_full()),
                        );
                    }
                }
                Some(crate::usage::Reading::Note(text)) => card = card.child(note(text)),
                Some(crate::usage::Reading::Problem { long, .. }) => {
                    card = card.child(note(long).text_color(c(ui::t_amber())))
                }
                _ if !d.ready => {}
                _ => {
                    card = card.child(note(if reading {
                        "Reading its plan…"
                    } else {
                        "This engine doesn't share its plan limits."
                    }))
                }
            }
            if !d.ready {
                card = card.child(
                    note("Not found on this computer. Set it up under Engines.")
                        .text_color(c(ui::t_red())),
                );
            }
            list = list.child(card);
        }

        let link = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(28.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(ui::body()))
                .cursor_pointer()
                .map(|d| super::motion::hover_fill(d, None, ui::hover()))
                .hover(|s| s.text_color(c(ui::ink())))
                .child(label)
        };
        let foot = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .py(px(6.))
            .border_t_1()
            .border_color(c(ui::line2()))
            .bg(c(ui::bg()))
            .child(
                link(
                    "usage-refresh",
                    if reading { "Reading…" } else { "Refresh" },
                )
                .on_click(cx.listener(|_, _, _, cx| {
                    crate::usage::refresh(true);
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(
                link("usage-engines", "Engines").on_click(cx.listener(|this, _, _, cx| {
                    this.usage_open = false;
                    this.go(Place::Settings(Pane::Engines), cx)
                })),
            )
            .child(
                link("usage-page", "Usage details").on_click(cx.listener(|this, _, _, cx| {
                    this.usage_open = false;
                    this.go(Place::Usage, cx)
                })),
            );

        div()
            .absolute()
            .inset_0()
            // A click anywhere else puts it away.
            .child(
                div()
                    .id("usage-backdrop")
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.usage_open = false;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("usage-popover")
                    .absolute()
                    .left(px(self.usage_left()))
                    .bottom(px(36.))
                    .w(px(USAGE_W))
                    .max_h(px(560.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .rounded(px(12.))
                    .bg(c(ui::float()))
                    .border_1()
                    .border_color(c(ui::line_hi()))
                    .shadow(vec![BoxShadow {
                        color: ca(0x000000, 0.5 * ui::float_shadow()),
                        offset: point(px(0.), px(12.)),
                        blur_radius: px(32.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px(px(16.))
                            .pt(px(14.))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child("Engines"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c(ui::faint()))
                                    .child("Plan limits and today"),
                            ),
                    )
                    .child(totals)
                    .child(list)
                    .child(foot),
            )
    }
}

/// A quiet line under an engine.
fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(11.5))
        .text_color(c(ui::faint()))
        .child(text.into())
}

#[cfg(test)]
mod popover_tests {
    use super::popover_left;

    #[test]
    fn popover_sits_over_the_status_item() {
        // Its right edge lines up with the end of the bar.
        assert_eq!(popover_left(1400., 360.), 1030.);
        // It never leaves the window.
        assert_eq!(popover_left(300., 360.), 8.);
    }
}
