//! Usage: what the crew spent across every project, over the last 7, 14 or
//! 30 days, from the jobs kept on this computer.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::data::Member;
use super::home::{cost, status_of};
use super::store::{self, Status};
use super::ui::{self, c};
use super::{Pane, Place, Shell};

/// One bot's share of the window.
struct Share {
    who: Member,
    spend: f64,
    jobs: usize,
}

/// A mono section label, with an optional note on the right.
fn label(left: impl Into<SharedString>, right: Option<AnyElement>) -> Div {
    let left: SharedString = left.into();
    div()
        .flex()
        .items_center()
        .justify_between()
        .font_family(ui::MONO)
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(ui::faint()))
        .child(left.to_uppercase())
        .children(right)
}

/// A card on the raised ground with a hairline edge.
fn group() -> Div {
    div()
        .rounded(px(6.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line()))
        .overflow_hidden()
}

impl Shell {
    pub(super) fn usage(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let now = store::now();
        let days = self.usage_days.clamp(1, 30);
        let recent: Vec<_> = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| now.saturating_sub(j.started) < days * 86_400)
            .cloned()
            .collect();
        let spend: f64 = recent.iter().filter_map(|j| cost(&j.summary)).sum::<f64>() + 0.0;
        let approved = recent.iter().filter(|j| finished_well(j.status)).count();
        let st = cx.global::<crate::settings::AppSettings>().clone();
        let (plan, today) = super::statusbar::plan_and_spend(&st, &self.crew.store);

        // Per bot, by spend, then by jobs when nothing cost money.
        let mut shares: Vec<Share> = Vec::new();
        for j in &recent {
            let v = cost(&j.summary).unwrap_or(0.);
            match shares.iter_mut().find(|s| s.who == j.bot) {
                Some(s) => {
                    s.spend += v;
                    s.jobs += 1;
                }
                None => shares.push(Share {
                    who: j.bot,
                    spend: v,
                    jobs: 1,
                }),
            }
        }
        let by_money = spend > 0.;
        let worth = |s: &Share| if by_money { s.spend } else { s.jobs as f64 };
        shares.sort_by(|a, b| worth(b).total_cmp(&worth(a)));

        // Each day's total, oldest first, today last.
        let mut per_day = vec![0f64; days as usize];
        for j in &recent {
            let back = (now / 86_400).saturating_sub(j.started / 86_400) as usize;
            if back < per_day.len() {
                let i = per_day.len() - 1 - back;
                per_day[i] += if by_money {
                    cost(&j.summary).unwrap_or(0.)
                } else {
                    1.
                };
            }
        }
        let top = per_day.iter().copied().fold(0., f64::max).max(f64::EPSILON);

        // The window: 7, 14 or 30 days.
        let mut seg = div()
            .flex()
            .flex_none()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(5.))
            .bg(c(ui::hover()));
        for n in [7u64, 14, 30] {
            let on = days == n;
            seg = seg.child(
                div()
                    .id(("usage-days", n as usize))
                    .px(px(9.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.bg(c(ui::pick_fill())).text_color(c(ui::pick_on()))
                        } else {
                            d.text_color(c(ui::body()))
                                .hover(|s| s.text_color(c(ui::ink())))
                        }
                    })
                    .child(format!("{n} days"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.usage_days = n;
                        cx.notify();
                    })),
            );
        }
        let head = div()
            .flex()
            .items_end()
            .child(
                div()
                    .flex_1()
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("Usage"),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(px(12.5))
                            .text_color(c(ui::body()))
                            .child("What the crew spent, across every project."),
                    ),
            )
            .child(seg);

        // Four numbers in one strip.
        let stat = |n: String, k: &'static str, first: bool| {
            div()
                .flex_1()
                .px(px(14.))
                .py(px(10.))
                .when(!first, |d| d.border_l_1().border_color(c(ui::line())))
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(17.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::ink()))
                        .child(n),
                )
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::faint()))
                        .child(k.to_uppercase()),
                )
        };
        let strip = group()
            .flex()
            .child(stat(format!("${spend:.2}"), "Spent", true))
            .child(stat(recent.len().to_string(), "Jobs", false))
            .child(stat(approved.to_string(), "Approved", false))
            .child(stat(
                plan.map_or("\u{2014}".into(), |p| format!("{p:.0}%")),
                "Plan used",
                false,
            ));

        // Per day: a bar a day, today in lime.
        let short = days <= 7;
        let mut bars = div().flex_1().min_h_0().flex().gap(px(if short {
            10.
        } else if days <= 14 {
            6.
        } else {
            3.
        }));
        let n = per_day.len();
        for (i, v) in per_day.iter().enumerate() {
            let is_today = i + 1 == n;
            let back = (n - 1 - i) as u64;
            let name = if is_today {
                "Today".to_string()
            } else if short {
                weekday(now / 86_400 - back).to_string()
            } else {
                String::new()
            };
            let frac = (*v / top) as f32;
            bars = bars.child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_end()
                    .gap(px(6.))
                    .child(
                        div()
                            .w_full()
                            .h(px((frac * 104.).max(2.)))
                            .rounded(px(2.))
                            .map(|d| {
                                if is_today {
                                    d.bg(c(ui::pick_fill()))
                                        .border_1()
                                        .border_color(c(ui::line_hi()))
                                } else {
                                    d.bg(c(ui::line()))
                                }
                            }),
                    )
                    .when(short || is_today, |d| {
                        d.child(
                            div()
                                .font_family(ui::MONO)
                                .text_size(px(10.5))
                                .text_color(c(if is_today { ui::ink() } else { ui::faint() }))
                                .child(name),
                        )
                    }),
            );
        }
        let today_note = div()
            .flex()
            .gap(px(4.))
            .child("today")
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(ui::ink()))
                    .child(format!("${today:.2}")),
            )
            .into_any_element();
        let chart = group()
            .flex_1()
            .min_w_0()
            .h(px(186.))
            .px(px(14.))
            .py(px(12.))
            .flex()
            .flex_col()
            .gap(px(7.))
            .child(label("Per day", Some(today_note)))
            .child(bars);

        // By bot: a bar each in the bot's colour.
        let most = shares.first().map(worth).unwrap_or(0.).max(f64::EPSILON);
        let mut by_bot = group()
            .flex_1()
            .min_w_0()
            .px(px(14.))
            .py(px(12.))
            .flex()
            .flex_col()
            .child(div().pb(px(7.)).child(label("By bot", None)));
        if shares.is_empty() {
            by_bot = by_bot.child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child("No jobs yet."),
            );
        }
        for s in shares.iter().take(6) {
            let face = self.avatar(s.who, 16.);
            let frac = (worth(s) / most) as f32;
            by_bot = by_bot.child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .text_size(px(12.5))
                    .text_color(c(ui::ink()))
                    .child(face)
                    .child(div().w(px(64.)).flex_none().child(s.who.name()))
                    .child(
                        div()
                            .flex_1()
                            .h(px(5.))
                            .bg(c(ui::hover()))
                            .child(div().h_full().w(relative(frac)).bg(c(s.who.accent()))),
                    )
                    .child(
                        div()
                            .w(px(44.))
                            .flex()
                            .justify_end()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .child(if by_money {
                                format!("${:.2}", s.spend)
                            } else {
                                format!("{} jobs", s.jobs)
                            }),
                    ),
            );
        }
        let pair = div()
            .flex()
            .gap(px(16.))
            .child(
                div()
                    .min_w_0()
                    .flex_grow(1.25)
                    .flex_basis(px(0.))
                    .child(chart),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_grow(1.)
                    .flex_basis(px(0.))
                    .child(by_bot),
            );

        // The costliest jobs in the window, so a surprise has a name.
        let mut dear: Vec<_> = recent
            .iter()
            .filter_map(|j| cost(&j.summary).map(|v| (v, j)))
            .filter(|(v, _)| *v > 0.)
            .collect();
        dear.sort_by(|a, b| b.0.total_cmp(&a.0));
        let all = div()
            .id("usage-all-jobs")
            .cursor_pointer()
            .hover(|s| s.text_color(c(ui::ink())))
            .child(format!("all {} in Jobs \u{2197}", recent.len()))
            .on_click(cx.listener(|this, _, _, cx| this.go(Place::History, cx)))
            .into_any_element();
        let when = if days == 7 {
            "this week".to_string()
        } else {
            format!("in {days} days")
        };
        let mut rows = group().px(px(14.)).flex().flex_col();
        if dear.is_empty() {
            rows = rows.child(
                div()
                    .py(px(12.))
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .child("Nothing has cost anything yet."),
            );
        }
        for (i, (v, j)) in dear.into_iter().take(4).enumerate() {
            let face = self.avatar(j.bot, 18.);
            let id = j.id;
            let (word, tone) = status_of(j);
            rows = rows.child(
                div()
                    .id(("usage-job", id))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .when(i > 0, |d| {
                        d.border_t_1().border_color(c(ui::line())).border_dashed()
                    })
                    .cursor_pointer()
                    .child(face)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::ink()))
                                    .child(div().truncate().child(j.title.clone()))
                                    .child(
                                        div()
                                            .font_family(ui::MONO)
                                            .text_size(px(11.))
                                            .text_color(c(ui::faint()))
                                            .child(format!("#{id}")),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(1.))
                                    .text_size(px(12.))
                                    .text_color(c(ui::faint()))
                                    .child(format!(
                                        "{} \u{b7} {} \u{b7} {}",
                                        j.bot.name(),
                                        j.folder.name,
                                        took(j.seconds())
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .w(px(96.))
                            .flex_none()
                            .child(ui::mono_status(tone.0, &word)),
                    )
                    .child(
                        div()
                            .w(px(48.))
                            .flex()
                            .justify_end()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(ui::ink()))
                            .child(format!("${v:.2}")),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
            );
        }
        let costly = div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .child(label(format!("Costliest jobs {when}"), Some(all)))
            .child(rows);

        // The default engine's plan, when it shares how much is used.
        // The engine closest to its limit, the one "Plan used" counts.
        let fullest = crate::agent::PROVIDERS
            .iter()
            .filter_map(|p| {
                crate::usage::get(p.id)
                    .and_then(|u| u.headline().cloned())
                    .map(|w| (p, w))
            })
            .max_by(|a, b| a.1.used.total_cmp(&b.1.used));
        let plan_note = fullest.map(|(p, w)| {
            let resets = w
                .resets_at
                .map(|at| {
                    let left = (at - now as i64).max(0) as u64;
                    format!("Resets in {}. ", took_long(left))
                })
                .unwrap_or_default();
            group()
                .px(px(14.))
                .py(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(12.5))
                .child(div().size(px(6.)).flex_none().bg(c(ui::t_amber())))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_wrap()
                        .gap(px(4.))
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(ui::ink()))
                                .child(format!("{} plan at {:.0}%.", p.name, w.used)),
                        )
                        .child(div().text_color(c(ui::body())).child(format!(
                            "{resets}When it runs out, jobs wait or switch engine, as you set in Engines."
                        ))),
                )
                .child(
                    div()
                        .id("usage-engines")
                        .cursor_pointer()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::accent_text()))
                        .child("Engines \u{2197}")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.go(Place::Settings(Pane::Engines), cx)
                        })),
                )
        });

        div()
            .id("usage")
            .size_full()
            .overflow_y_scroll()
            .px(px(36.))
            .py(px(24.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(head)
            .child(strip)
            .child(pair)
            .child(costly)
            .children(plan_note)
            .into_any_element()
    }
}

/// The weekday of a day number since 1970 ("Mon"); day 0 was a Thursday.
fn weekday(day: u64) -> &'static str {
    ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(day % 7) as usize]
}

/// How long a job took: "2h 40m", "9m", "2m 21s", "12s".
fn took(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m >= 5 || (m > 0 && s == 0) {
        format!("{m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// A longer wait in words: "3 days", "5 hours", "20 minutes".
fn took_long(secs: u64) -> String {
    let plural = |n: u64, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    if secs >= 86_400 {
        plural(secs / 86_400, "day")
    } else if secs >= 3600 {
        plural(secs / 3600, "hour")
    } else {
        plural((secs / 60).max(1), "minute")
    }
}

/// A job that did what it was asked: done, waiting for your OK, or on its way in.
fn finished_well(status: Status) -> bool {
    matches!(
        status,
        Status::Done | Status::Review | Status::Shipping | Status::Shipped
    )
}

#[cfg(test)]
mod tests {
    use super::{took, took_long, weekday};
    use std::prelude::v1::test;

    #[test]
    fn says_how_long_a_job_took() {
        assert_eq!(took(12), "12s");
        assert_eq!(took(141), "2m 21s");
        assert_eq!(took(540), "9m");
        assert_eq!(took(240), "4m");
        assert_eq!(took(9600), "2h 40m");
        assert_eq!(took_long(3 * 86_400 + 5), "3 days");
        assert_eq!(took_long(3600), "1 hour");
    }

    #[test]
    fn knows_the_weekday() {
        // 10 November 2026 is a Tuesday.
        assert_eq!(weekday(20_767), "Tue");
    }
}
