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

/// The plan window closest to its limit, as a percent used, and what today
/// cost: the status bar's figures, for the menu bar panel.
pub(super) fn plan_and_spend(
    st: &crate::settings::AppSettings,
    store: &Store,
) -> (Option<f32>, f64) {
    let days = engines_today(st, store);
    let spend = days.iter().map(|d| d.spend).sum::<f64>();
    let plan = days
        .iter()
        .take(BAR_ENGINES)
        .filter_map(|d| d.usage.as_ref().and_then(|u| u.headline().map(|w| w.used)))
        .fold(None, |t: Option<f32>, u| Some(t.map_or(u, |t| t.max(u))));
    (plan, spend)
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
                ui::t_red()
            } else if head.as_ref().is_some_and(|w| w.used >= 80.) {
                ui::t_amber()
            } else {
                ui::t_green()
            };
            // The design's way: a square in the engine's state, then its name.
            dots = dots
                .child(div().size(px(6.)).flex_none().rounded(px(1.)).bg(c(dot)))
                .child(
                    div()
                        .mr(px(6.))
                        .child(crate::agent::PROVIDERS[d.i].name.to_string()),
                );
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
        let tip = format!(
            "{}\n{} tokens today\nClick for usage details",
            names.join(" \u{b7} "),
            crate::health::token_count(tokens)
        );
        let single = (days.len() == 1).then(|| crate::agent::PROVIDERS[days[0].i].name);
        let engines = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(dots)
            .when(days.len() > BAR_ENGINES, |d| {
                d.child(
                    div()
                        .text_color(c(ui::faint()))
                        .child(format!("+{}", days.len() - BAR_ENGINES)),
                )
            })
            .when(missing && single.is_some() && tight.is_none(), |el| {
                el.child(div().text_color(c(ui::t_red())).child("not found"))
            });
        // The plan window closest to its limit: "Plan ▬▬▬ 62%".
        let plan = tight.map(|used| {
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .child("Plan")
                .child(
                    div()
                        .w(px(56.))
                        .h(px(5.))
                        .flex_none()
                        .bg(c(ui::line_hi()))
                        .child(
                            div()
                                .h_full()
                                .bg(c(if used >= 100. {
                                    ui::t_red()
                                } else {
                                    ui::t_amber()
                                }))
                                .w(relative((used / 100.).clamp(0.02, 1.))),
                        ),
                )
                .child(format!("{used:.0}%"))
        });
        let right = div()
            .id("status-today")
            .h(px(22.))
            .px(px(9.))
            .flex()
            .items_center()
            .gap(px(16.))
            .rounded(px(4.))
            .cursor_pointer()
            .map(|d| {
                // Lit while its popover is open, so it reads as the anchor.
                if open {
                    d.bg(c(ui::hover())).text_color(c(ui::ink()))
                } else {
                    super::motion::hover_fill(d, None, ui::hover())
                        .hover(|s| s.text_color(c(ui::ink())))
                }
            })
            // Not while the popover is open: it would cover the popover's links.
            .when(!open, |d| {
                d.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
            })
            .child(engines)
            .children(plan)
            .child(
                div()
                    .flex()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_color(c(ui::ink()))
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("${spend:.2}")),
                    )
                    .child("today"),
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_usage(cx)));

        // Where the crew runs: this computer, and your server once there is
        // one. A click opens Machines.
        let here = if cfg!(target_os = "macos") {
            "This Mac"
        } else {
            "This computer"
        };
        let server = self.crew.link.as_ref().map(|l| l.up());
        let machine = div()
            .id("status-machine")
            .h(px(22.))
            .px(px(8.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
            .child(Icon::new(IconName::Server).size(px(13.)))
            .child("Runs on")
            .child(
                div()
                    .text_color(c(ui::ink()))
                    .font_weight(FontWeight::MEDIUM)
                    .child(here),
            )
            .when_some(server, |d, up| {
                d.child(div().size(px(6.)).rounded_full().bg(c(if up {
                    ui::t_green()
                } else {
                    ui::faint()
                })))
                .child("server")
            })
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Machines: where the crew runs")
                    .build(window, cx)
            })
            .on_click(cx.listener(|this, _, _, cx| this.go(super::Place::Server, cx)));

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
            .child("Terminal")
            .child(div().text_color(c(ui::faint())).child(super::term::KEY))
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
            .border_color(c(ui::line()))
            .bg(c(ui::ground()))
            .font_family(ui::MONO)
            .text_size(px(11.5))
            .text_color(c(ui::muted()))
            .child(machine)
            .child(term)
            .child(div().flex_1())
            .child(right)
    }

    /// The engines popover, over the status bar: today across the crew, then
    /// each engine's plan windows, how much of them is used and when they
    /// reset, and its day.
    ///
    /// Everything sits on one grid: a 16 px edge on both sides, and under
    /// each engine a column that starts where its name does.
    pub(super) fn usage_popover(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        /// The popover's side padding.
        const EDGE: f32 = 16.;
        /// An engine's icon tile, and the gap after it: its name, windows
        /// and notes all start this far in.
        const TILE: f32 = 28.;
        const INSET: f32 = TILE + 10.;

        let st = cx.global::<crate::settings::AppSettings>();
        let days = engines_today(st, &self.crew.store);
        let now = crate::usage::now_secs();
        let spend: f64 = days.iter().map(|d| d.spend).sum::<f64>() + 0.0;
        let tokens: u64 = days.iter().map(|d| d.tokens).sum();
        let jobs: usize = days.iter().map(|d| d.jobs).sum();
        let reading = crate::usage::busy();
        // When the freshest plan reading was taken, for the header.
        let read_at = days
            .iter()
            .filter_map(|d| d.usage.as_ref())
            .filter(|u| matches!(u.reading, crate::usage::Reading::Limits(_)))
            .map(|u| u.at)
            .max();

        // The header: a tile, the title over when the plans were last read,
        // and a refresh.
        let refresh = div()
            .id("usage-refresh")
            .size(px(28.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .border_1()
            .border_color(c(ui::line_hi()))
            .cursor_pointer()
            .map(|d| {
                super::motion::hover_paint(d, None, ui::hover(), Some((ui::muted(), ui::ink())))
            })
            .child(
                Icon::new(if reading {
                    IconName::LoaderCircle
                } else {
                    IconName::RefreshCw
                })
                .size(px(13.)),
            )
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(if reading {
                    "Reading plan limits\u{2026}"
                } else {
                    "Read plan limits again"
                })
                .build(window, cx)
            })
            .on_click(cx.listener(|_, _, _, cx| {
                crate::usage::refresh(true);
                cx.notify();
            }));
        let updated = match (reading, read_at) {
            (true, _) => "Reading plan limits\u{2026}".to_string(),
            (false, Some(at)) => format!("Updated {}", crate::usage::ago(at, now)),
            (false, None) => "Plan limits and today's spend".to_string(),
        };
        let head = div()
            .px(px(EDGE))
            .pt(px(14.))
            .pb(px(12.))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(tile(IconName::Gauge, ui::BLUE, TILE))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("Usage today"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child(updated),
                    ),
            )
            .child(refresh);

        // Today across every engine, as three figures on one tile.
        let figure = |icon: IconName, label: &'static str, value: String, first: bool| {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(12.))
                .py(px(10.))
                .when(!first, |el| el.border_l_1().border_color(c(ui::line2())))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(11.))
                        .text_color(c(ui::faint()))
                        .child(Icon::new(icon).size(px(12.)))
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(17.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(value),
                )
        };
        let totals = div()
            .mx(px(EDGE))
            .flex()
            .rounded(px(10.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line2()))
            .child(figure(
                IconName::CircleDollarSign,
                "Spent",
                format!("${spend:.2}"),
                true,
            ))
            .child(figure(
                IconName::Zap,
                "Tokens",
                crate::health::token_count(tokens),
                false,
            ))
            .child(figure(
                IconName::ListChecks,
                "Jobs",
                jobs.to_string(),
                false,
            ));

        let caption = div()
            .px(px(EDGE))
            .pt(px(16.))
            .pb(px(4.))
            .text_size(px(10.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(c(ui::faint()))
            .child("ENGINES");

        let mut list = div().flex().flex_col();
        for (k, d) in days.iter().enumerate() {
            let p = &crate::agent::PROVIDERS[d.i];
            let default = k == 0;
            let state = if d.ready { ui::GREEN } else { ui::RED };
            // The tile, with the engine's state as a dot on its corner.
            let mark = div()
                .relative()
                .flex_none()
                .child(tile(IconName::Cpu, ui::VIOLET, TILE))
                .child(
                    div()
                        .absolute()
                        .right(px(-2.))
                        .bottom(px(-2.))
                        .size(px(10.))
                        .rounded_full()
                        .border_2()
                        .border_color(c(ui::float()))
                        .bg(c(state)),
                );
            let title = div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(1.))
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
                                .child(p.name),
                        )
                        .when(default, |el| {
                            el.child(
                                div()
                                    .h(px(17.))
                                    .px(px(6.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(5.))
                                    .bg(ca(ui::RUNO, 0.14))
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::t_violet()))
                                    .child("Default"),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(c(ui::faint()))
                        .child(format!(
                            "{} tokens \u{b7} {} job{}",
                            crate::health::token_count(d.tokens),
                            d.jobs,
                            if d.jobs == 1 { "" } else { "s" }
                        )),
                );
            let row = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(mark)
                .child(title)
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(format!("${:.2}", d.spend)),
                );

            // Under the name: its plan windows, or a note.
            let mut body = div().pl(px(INSET)).flex().flex_col().gap(px(10.));
            let mut any = false;
            match d.usage.as_ref().map(|u| &u.reading) {
                Some(crate::usage::Reading::Limits(windows)) if !windows.is_empty() => {
                    for w in windows {
                        let resets = w
                            .resets_at
                            .map(|at| format!("resets in {}", crate::usage::left(at, now)))
                            .unwrap_or_default();
                        let icon = if w.label == "Weekly" {
                            IconName::CalendarDays
                        } else {
                            IconName::Clock
                        };
                        body = body.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .text_size(px(11.5))
                                        .child(
                                            Icon::new(icon)
                                                .size(px(12.))
                                                .text_color(c(ui::faint())),
                                        )
                                        .child(
                                            div()
                                                .text_color(c(ui::body()))
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(w.label),
                                        )
                                        .child(
                                            div().flex_1().text_color(c(ui::faint())).child(resets),
                                        )
                                        .child(
                                            div()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(c(if w.used >= 100. {
                                                    ui::t_red()
                                                } else if w.used >= 80. {
                                                    ui::t_amber()
                                                } else {
                                                    ui::body()
                                                }))
                                                .child(format!("{:.0}% used", w.used)),
                                        ),
                                )
                                .child(meter(w.used, 280., 6.).w_full()),
                        );
                        any = true;
                    }
                }
                Some(crate::usage::Reading::Note(text)) => {
                    body = body.child(note(text));
                    any = true;
                }
                Some(crate::usage::Reading::Problem { long, .. }) => {
                    body = body.child(note(long).text_color(c(ui::t_amber())));
                    any = true;
                }
                _ if !d.ready => {}
                _ => {
                    body = body.child(note(if reading {
                        "Reading its plan\u{2026}"
                    } else {
                        "This engine doesn't share its plan limits."
                    }));
                    any = true;
                }
            }
            if !d.ready {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(px(12.))
                                .text_color(c(ui::t_red())),
                        )
                        .child(
                            note("Not installed here. Set it up under Engines.")
                                .text_color(c(ui::t_red())),
                        ),
                );
                any = true;
            }
            list = list.child(
                div()
                    .mx(px(EDGE))
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .when(k > 0, |el| el.border_t_1().border_color(c(ui::line2())))
                    .child(row)
                    .when(any, |el| el.child(body)),
            );
        }

        // Two equal buttons at the foot: where engines are set up, and the
        // full usage page.
        let link = |id: &'static str, icon: IconName, label: &'static str| {
            div()
                .id(id)
                .flex_1()
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .rounded(px(8.))
                .border_1()
                .border_color(c(ui::line_hi()))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(ui::body()))
                .cursor_pointer()
                .map(|d| super::motion::hover_fill(d, None, ui::hover()))
                .hover(|s| s.text_color(c(ui::ink())))
                .active(|s| s.bg(c(ui::press())))
                .child(Icon::new(icon).size(px(13.)).text_color(c(ui::muted())))
                .child(label)
        };
        let foot = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(EDGE))
            .py(px(12.))
            .border_t_1()
            .border_color(c(ui::line2()))
            .child(
                link("usage-engines", IconName::Settings, "Engines").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.usage_open = false;
                        this.go(Place::Settings(Pane::Engines), cx)
                    },
                )),
            )
            .child(
                link("usage-page", IconName::ChartColumn, "Usage details").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.usage_open = false;
                        this.go(Place::Settings(super::Pane::Usage), cx)
                    },
                )),
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
                    .max_h(px(620.))
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
                    .child(head)
                    // The header and the buttons stay put; the middle scrolls
                    // when there are many engines.
                    .child(
                        div()
                            .id("usage-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .pb(px(4.))
                            .child(totals)
                            .child(caption)
                            .child(list),
                    )
                    .child(foot),
            )
    }
}

/// A square tile holding an icon, tinted with `tone`: the popover's header
/// and each engine lead with one.
fn tile(icon: IconName, tone: u32, size: f32) -> Div {
    div()
        .size(px(size))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .bg(ca(tone, 0.12))
        .child(Icon::new(icon).size(px(size * 0.55)).text_color(c(tone)))
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
