//! Race it: one job tried on two or three engines at once, each on its own
//! branch, then shown side by side. You keep the try you like best; Shipper
//! takes it from there and the others are thrown away.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::home;
use super::live::status_color;
use super::store::{Job, Status};
use super::ui::{self, c};
use super::work::{self, Verdict};
use super::{Place, Shell};

/// What sets one try apart from the others, for the small tags on its column.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    pub fastest: bool,
    pub cheapest: bool,
    pub smallest: bool,
    pub passes: bool,
}

/// The tags for each try, once at least two have finished.
pub fn marks(tries: &[Job]) -> Vec<Marks> {
    let done: Vec<&Job> = tries
        .iter()
        .filter(|j| matches!(j.status, Status::Review | Status::Done))
        .filter(|j| !j.changes.is_empty())
        .collect();
    let mut out = vec![Marks::default(); tries.len()];
    if done.len() < 2 {
        for (i, j) in tries.iter().enumerate() {
            out[i].passes = passes(j);
        }
        return out;
    }
    // A try whose checks failed isn't in the running for best: its speed or
    // price shouldn't sell it over one that works.
    let fails = |j: &Job| !j.checking && work::verdict(&j.checks, false).1 == Verdict::Fail;
    let done: Vec<&Job> = if done.iter().any(|j| !fails(j)) {
        done.into_iter().filter(|j| !fails(j)).collect()
    } else {
        done
    };
    let fastest = done.iter().map(|j| j.seconds()).min();
    let cheapest = done
        .iter()
        .filter_map(|j| home::cost(&j.summary))
        .fold(None, |m: Option<f64>, c| Some(m.map_or(c, |m| m.min(c))));
    let smallest = done.iter().map(|j| j.lines().0 + j.lines().1).min();
    for (i, j) in tries.iter().enumerate() {
        if !done.iter().any(|d| d.id == j.id) {
            continue;
        }
        out[i] = Marks {
            fastest: Some(j.seconds()) == fastest,
            cheapest: cheapest.is_some() && home::cost(&j.summary) == cheapest,
            smallest: Some(j.lines().0 + j.lines().1) == smallest,
            passes: passes(j),
        };
    }
    out
}

fn passes(j: &Job) -> bool {
    !j.checking && work::verdict(&j.checks, false).1 == Verdict::Pass
}

/// "4m 10s" for a number of seconds.
fn took(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
    }
}

impl Shell {
    pub(super) fn race(&mut self, race: u64, cx: &mut Context<Self>) -> AnyElement {
        let tries = self.tries(race);
        let Some(first) = tries.first().cloned() else {
            return self.empty_place("This race is gone", "Its tries were cleared.", cx);
        };
        let kept = tries
            .iter()
            .find(|j| matches!(j.status, Status::Shipping | Status::Shipped));
        let running = tries.iter().filter(|j| j.status == Status::Running).count();
        let line = match (kept, running) {
            (Some(k), _) => format!("You kept {}'s try. The others were thrown away.", k.engine),
            (None, 0) => "Every try has finished. Keep the one you like best.".to_string(),
            (None, n) => format!(
                "{} of {} still working. Each try is on its own branch.",
                n,
                tries.len()
            ),
        };
        let mut head = div().flex().items_center().gap(px(12.)).child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::caption(format!(
                            "RACE \u{b7} {} ENGINES \u{b7} {}",
                            tries.len(),
                            first.folder.name.to_uppercase()
                        ))),
                )
                .child(
                    div()
                        .text_size(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .truncate()
                        .child(first.label().to_string()),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(c(ui::muted()))
                        .child(line),
                ),
        );
        if running > 0 {
            let ids: Vec<u64> = tries
                .iter()
                .filter(|j| j.status == Status::Running)
                .map(|j| j.id)
                .collect();
            head = head.child(ui::ghost("race-stop", "Stop all").on_click(cx.listener(
                move |this, _, _, cx| {
                    for id in &ids {
                        this.stop_job(*id, cx);
                    }
                },
            )));
        }
        if tries.len() < 3 && kept.is_none() && first.mode.writes() {
            let id = first.id;
            head = head.child(
                ui::ghost("race-more", "Add an engine")
                    .on_click(cx.listener(move |this, _, _, cx| this.race_it(id, cx))),
            );
        }

        let marks = marks(&tries);
        let mut cols = div().flex().gap(px(14.)).items_start();
        for (k, j) in tries.iter().enumerate() {
            cols = cols.child(self.try_column(j, &marks[k], kept.is_some(), cx));
        }
        div()
            .id("race")
            .size_full()
            .overflow_y_scroll()
            .bg(c(ui::bg()))
            .child(
                div()
                    .px(px(32.))
                    .py(px(28.))
                    .flex()
                    .flex_col()
                    .gap(px(22.))
                    .child(head)
                    .child(cols)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child("When you keep one, Shipper ships it the usual way and the other branches are deleted. Your checkout is never touched."),
                    ),
            )
            .into_any_element()
    }

    fn try_column(&mut self, j: &Job, m: &Marks, decided: bool, cx: &mut Context<Self>) -> Div {
        let id = j.id;
        let face = self.avatar(j.bot, 22.);
        let (word, tint) = home::status_of(j);
        let (added, removed) = j.lines();
        let mut tags = div().flex().flex_wrap().gap(px(6.));
        for (on, label, color) in [
            (m.passes, "Checks pass", ui::t_green()),
            (m.fastest, "Fastest", ui::t_blue()),
            (m.cheapest, "Cheapest", ui::t_amber()),
            (m.smallest, "Smallest change", ui::body()),
        ] {
            if on {
                tags = tags.child(
                    div()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded_full()
                        .border_1()
                        .border_color(c(ui::line_hi()))
                        .text_size(px(11.))
                        .text_color(c(color))
                        .child(label),
                );
            }
        }
        let stat = |label: &str, value: String| {
            div()
                .flex()
                .justify_between()
                .text_size(px(12.5))
                .child(div().text_color(c(ui::muted())).child(label.to_string()))
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_color(c(ui::ink()))
                        .child(value),
                )
        };
        let (verdict, v) = work::verdict(&j.checks, j.checking);
        let verdict_color = match v {
            Verdict::Pass => ui::t_green(),
            Verdict::Fail => ui::t_red(),
            Verdict::Couldnt => ui::t_amber(),
            Verdict::Running => ui::t_blue(),
            Verdict::None => ui::muted(),
        };
        let mut stats = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(stat("Time", took(j.seconds())));
        if let Some(cost) = home::cost(&j.summary) {
            stats = stats.child(stat("Cost", format!("${cost:.2}")));
        }
        if !j.changes.is_empty() {
            stats = stats.child(stat(
                "Change",
                format!("{} files +{added} \u{2212}{removed}", j.changes.len()),
            ));
        }
        let body: Div = match j.status {
            Status::Running => {
                let frac = j
                    .progress()
                    .map(|(d, n)| d as f32 / n.max(1) as f32)
                    .unwrap_or(0.08);
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(c(ui::body()))
                            .truncate()
                            .child(j.doing()),
                    )
                    .child(ui::bar(frac, ui::ORANGE))
            }
            Status::Review | Status::Done | Status::Shipping | Status::Shipped => {
                let mut files = div().flex().flex_col().gap(px(3.));
                for f in j.changes.iter().take(6) {
                    files = files.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .font_family(ui::MONO)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(c(ui::body()))
                                    .child(f.path.clone()),
                            )
                            .child(
                                div()
                                    .text_color(c(ui::t_green()))
                                    .child(format!("+{}", f.added)),
                            )
                            .child(
                                div()
                                    .text_color(c(ui::t_red()))
                                    .child(format!("\u{2212}{}", f.removed)),
                            ),
                    );
                }
                if j.changes.len() > 6 {
                    files = files.child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child(format!("and {} more", j.changes.len() - 6)),
                    );
                }
                let words = j.last_words().map(super::live::plain).unwrap_or_default();
                let words: String = words.chars().take(220).collect();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(ui::status(verdict_color, verdict))
                    .when(!words.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(12.5))
                                .line_height(px(18.))
                                .text_color(c(ui::body()))
                                .child(words),
                        )
                    })
                    .child(files)
            }
            _ => {
                let why = j
                    .feed
                    .iter()
                    .rev()
                    .find_map(|i| match i {
                        super::store::Item::Error(e) | super::store::Item::Note(e) => {
                            Some(e.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| j.status.label().to_string());
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::muted()))
                    .child(why)
            }
        };
        let can_keep = !decided && j.status == Status::Review;
        let won = matches!(j.status, Status::Shipping | Status::Shipped);
        let mut foot = div().flex().gap(px(8.)).child(
            ui::ghost(("try-open", id as usize), "Open")
                .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Live(id), cx))),
        );
        if can_keep {
            let failing = v == Verdict::Fail;
            let keep = if failing {
                ui::ghost(("try-keep", id as usize), "Keep anyway").text_color(c(ui::t_red()))
            } else {
                ui::solid(("try-keep", id as usize), "Keep this one")
            };
            foot = foot.child(
                keep.flex_1()
                    .on_click(cx.listener(move |this, _, _, cx| this.keep_try(id, cx))),
            );
        }
        ui::card()
            .flex_1()
            .min_w(px(220.))
            .when(won, |d| d.border_color(c(ui::GREEN)))
            .when(j.status == Status::Discarded, |d| d.opacity(0.55))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(face)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .truncate()
                            .child(if j.engine.is_empty() {
                                j.pick.clone()
                            } else {
                                j.engine.clone()
                            }),
                    )
                    .child(div().size(px(7.)).rounded_full().bg(c(
                        if j.status == Status::Running {
                            ui::BLUE
                        } else {
                            status_color(j.status)
                        },
                    ))),
            )
            .child(home::pill(&word, tint))
            .child(tags)
            .child(stats)
            .child(div().h(px(1.)).bg(c(ui::line2())))
            .child(body)
            .child(foot)
    }
}

/// The try Peek would keep once at least two have finished: checks passing
/// first, then the smallest change, then the cheapest, then the fastest.
/// None while there's nothing to compare or two tie.
pub fn pick(m: &[Marks]) -> Option<usize> {
    let score = |m: &Marks| {
        u8::from(m.passes) * 8
            + u8::from(m.smallest) * 4
            + u8::from(m.cheapest) * 2
            + u8::from(m.fastest)
    };
    let best = m.iter().map(score).max()?;
    if best < 2 {
        return None;
    }
    let mut at = m.iter().enumerate().filter(|(_, m)| score(m) == best);
    let first = at.next()?.0;
    at.next().is_none().then_some(first)
}

/// Why Peek would keep a try, in a few words.
fn why(m: &Marks) -> String {
    let mut bits = Vec::new();
    if m.smallest {
        bits.push("smallest change");
    }
    if m.passes {
        bits.push("checks pass");
    }
    if m.cheapest {
        bits.push("cheapest");
    }
    if m.fastest {
        bits.push("fastest");
    }
    bits.join(", ")
}

impl Shell {
    /// A race in its thread: one column per try, side by side, with what each
    /// changed, its checks and its cost, and the one Peek would keep.
    pub(super) fn race_card(&mut self, race: u64, cx: &mut Context<Self>) -> Div {
        let tries = self.tries(race);
        let Some(first) = tries.first().cloned() else {
            return div();
        };
        let marks = marks(&tries);
        let decided = tries
            .iter()
            .any(|j| matches!(j.status, Status::Shipping | Status::Shipped));
        let pick = (!decided).then(|| pick(&marks)).flatten();
        let running: Vec<u64> = tries
            .iter()
            .filter(|j| j.status == Status::Running)
            .map(|j| j.id)
            .collect();
        let longest = tries.iter().map(|j| j.seconds()).max().unwrap_or(0);
        let head = div()
            .px(px(14.))
            .py(px(11.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                Icon::new(IconName::Flag)
                    .size(px(14.))
                    .text_color(c(ui::ink())),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(first.label().to_string()),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(ui::faint()))
                    .child(format!(
                        "race \u{b7} {} tries \u{b7} {}",
                        tries.len(),
                        short(longest)
                    )),
            )
            .child(div().flex_1())
            .map(|d| {
                if running.is_empty() {
                    d.child(
                        ui::dbtn(("race-side", race as usize), "Side by side", ui::Btn::Ghost)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.go(Place::Race(race), cx)),
                            ),
                    )
                } else {
                    let ids = running.clone();
                    d.child(
                        ui::dbtn(("race-stop-all", race as usize), "Stop all", ui::Btn::Ghost)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                for id in &ids {
                                    this.stop_job(*id, cx);
                                }
                            })),
                    )
                }
            });
        let mut cols = div().flex();
        for (i, j) in tries.iter().enumerate() {
            let col = self.race_column(
                j,
                &marks[i],
                pick == Some(i),
                pick.is_none() && !decided,
                i > 0,
                cx,
            );
            cols = cols.child(col);
        }
        div()
            .w_full()
            .rounded(px(6.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .overflow_hidden()
            .child(head)
            .child(cols)
    }

    /// One try in the thread's race card.
    fn race_column(
        &mut self,
        j: &Job,
        m: &Marks,
        picked: bool,
        open_pick: bool,
        divided: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = j.id;
        let face = self.avatar(j.bot, 18.);
        let (word, _) = home::status_of(j);
        let (state, col) = match j.status {
            Status::Running => (
                match j.progress() {
                    Some((d, n)) => format!("step {} of {n}", (d + 1).min(n)),
                    None => "working".into(),
                },
                ui::t_blue(),
            ),
            Status::Review | Status::Done => ("done".into(), ui::t_green()),
            Status::Shipping | Status::Shipped => ("kept".into(), ui::t_green()),
            Status::Failed => ("failed".into(), ui::t_red()),
            _ => (word.to_lowercase(), ui::faint()),
        };
        let (added, removed) = j.lines();
        let row = |label: &'static str, value: AnyElement| {
            div()
                .py(px(6.))
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(c(ui::line()))
                .border_dashed()
                .text_size(px(12.5))
                .child(div().text_color(c(ui::body())).child(label))
                .child(value)
        };
        let mono = |s: String, col: u32| {
            div()
                .font_family(ui::MONO)
                .text_size(px(12.))
                .text_color(c(col))
                .child(s)
        };
        let changes = if j.changes.is_empty() {
            mono("\u{2014}".into(), ui::faint()).into_any_element()
        } else {
            div()
                .flex()
                .gap(px(5.))
                .child(mono(format!("+{added}"), ui::t_green()))
                .child(mono(format!("\u{2212}{removed}"), ui::t_red()))
                .into_any_element()
        };
        let ok = j.checks.iter().filter(|c| c.ok).count();
        let n = j.checks.len();
        let checks = if j.status == Status::Running {
            mono("waiting".into(), ui::faint())
        } else if j.checking {
            mono("running".into(), ui::t_blue())
        } else if n == 0 {
            mono("none".into(), ui::faint())
        } else if ok == n {
            mono(format!("\u{2713} {ok} of {n}"), ui::t_green())
        } else {
            mono(format!("\u{2717} {ok} of {n}"), ui::t_red())
        };
        let cost = mono(
            home::cost(&j.summary).map_or("\u{2014}".into(), |v| format!("${v:.2}")),
            ui::ink(),
        );
        let finished = matches!(j.status, Status::Review | Status::Done);
        let mut out = div()
            .flex_1()
            .min_w_0()
            .px(px(14.))
            .py(px(12.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .when(divided && !picked, |d| {
                d.border_l_1().border_color(c(ui::line()))
            })
            .when(picked, |d| {
                d.bg(c(super::theme::mix(
                    ui::raised(),
                    ui::pick_fill(),
                    ui::pick_k(0.035),
                )))
                .border(px(1.5))
                .border_color(c(ui::accent_bar()))
            })
            .child(
                div()
                    .pb(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(face)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(j.engine.clone()),
                    )
                    .child(ui::mono_status(col, &state)),
            );
        if j.status == Status::Running {
            let frac = j
                .progress()
                .map(|(d, n)| d as f32 / n.max(1) as f32)
                .unwrap_or(0.1);
            out = out.child(
                div()
                    .h(px(4.))
                    .child(super::channel::dashed_bar(frac, ui::t_blue())),
            );
        }
        out = out
            .child(row("changes", changes))
            .child(row("checks", checks.into_any_element()))
            .child(row("cost", cost.into_any_element()));
        if picked {
            let peek = self.avatar(super::data::Member::Fin, 14.);
            out = out.child(
                div()
                    .pt(px(6.))
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(c(ui::accent_text()))
                    .child(div().flex_none().child(peek))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(format!("Peek picks this: {}.", why(m))),
                    ),
            );
        }
        if finished {
            out = out.child(
                div()
                    .pt(px(8.))
                    .flex()
                    .gap(px(6.))
                    .child(
                        ui::dbtn(("race-see", id as usize), "See changes", ui::Btn::Plain)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.go(Place::Live(id), cx)),
                            ),
                    )
                    .when(picked || open_pick, |d| {
                        d.child(
                            ui::dbtn(
                                ("race-pick", id as usize),
                                "Pick this one",
                                if picked {
                                    ui::Btn::Primary
                                } else {
                                    ui::Btn::Plain
                                },
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.keep_try(id, cx))),
                        )
                    }),
            );
        }
        out
    }
}

/// "6m", "1h 5m", "40s".
fn short(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::{Changed, Job, Status};
    use super::{marks, pick, took, Marks};

    #[test]
    fn peek_picks_one_clear_winner_or_none() {
        let best = Marks {
            passes: true,
            smallest: true,
            ..Default::default()
        };
        let other = Marks {
            fastest: true,
            ..Default::default()
        };
        assert_eq!(pick(&[other.clone(), best.clone()]), Some(1));
        assert_eq!(pick(&[best.clone(), best]), None);
        assert_eq!(pick(&[other.clone(), other]), None);
    }

    fn try_(id: u64, secs: u64, cost: &str, lines: usize) -> Job {
        Job {
            id,
            race: 1,
            status: Status::Review,
            started: 100,
            ended: Some(100 + secs),
            summary: format!("{secs}s \u{b7} 3 turns \u{b7} ${cost}"),
            changes: vec![Changed {
                path: "a.rs".into(),
                status: "M".into(),
                added: lines,
                removed: 0,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn the_best_try_gets_its_tags() {
        let tries = vec![try_(1, 90, "0.30", 40), try_(2, 200, "0.10", 12)];
        let m = marks(&tries);
        assert!(m[0].fastest && !m[0].cheapest && !m[0].smallest);
        assert!(!m[1].fastest && m[1].cheapest && m[1].smallest);
    }

    #[test]
    fn one_finished_try_has_nothing_to_beat() {
        let mut running = try_(2, 10, "0.01", 1);
        running.status = Status::Running;
        let m = marks(&[try_(1, 90, "0.30", 40), running]);
        assert!(!m[0].fastest && !m[1].fastest);
    }

    #[test]
    fn a_try_that_fails_its_checks_wins_nothing() {
        let mut quick = try_(1, 60, "0.05", 5);
        quick.checks = vec![crate::proof::CheckRun {
            command: "cargo test".into(),
            ok: false,
            ..Default::default()
        }];
        let m = marks(&[quick, try_(2, 200, "0.40", 50)]);
        assert!(!m[0].fastest && !m[0].cheapest && !m[0].smallest);
        assert!(m[1].fastest && m[1].cheapest && m[1].smallest);
    }

    #[test]
    fn time_reads_plainly() {
        assert_eq!(took(42), "42s");
        assert_eq!(took(250), "4m 10s");
    }
}
