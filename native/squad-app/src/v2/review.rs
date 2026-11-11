//! The review: what a finished writing job changed, Checker's run of the
//! checks, and the decision (approve, send back or discard).

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::data::Member;
use super::ship;
use super::store::{Job, Status};
use super::ui::{self, c, ca};
use super::work::{self, Verdict};
use super::Shell;

/// Most diff lines shown for one file; the rest are a click away in the folder.
const DIFF_LINES: usize = 400;

impl Shell {
    /// Approve, as the main button; with failing checks it steps back to an
    /// outline button that says so.
    pub(super) fn approve_button(
        &mut self,
        key: impl Into<ElementId>,
        job: &Job,
        label: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = job.id;
        let failing = !job.checking && work::verdict(&job.checks, false).1 == Verdict::Fail;
        if !job.conflict.is_empty() {
            return div()
                .id(key)
                .px(px(14.))
                .h(px(32.))
                .flex()
                .items_center()
                .rounded(px(8.))
                .border_1()
                .border_color(c(ui::line()))
                .text_size(px(13.))
                .text_color(c(ui::faint()))
                .child("Fix the clash first");
        }
        let b = if failing {
            ui::ghost_i(key, IconName::GitMerge, "Approve anyway").text_color(c(ui::t_red()))
        } else {
            ui::solid_i(key, IconName::GitMerge, label)
        };
        b.on_click(cx.listener(move |this, _, _, cx| this.approve(id, cx)))
    }

    /// Checker's run of the project's checks on a job: the verdict with Run
    /// again, then each check with its time, and the end of a failing one's
    /// output. The review and the workspace's Checks tab both show it.
    pub(super) fn checks_block(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let (said, verdict) = work::verdict(&job.checks, job.checking);
        let verdict_color = match verdict {
            Verdict::Pass => ui::t_green(),
            Verdict::Fail => ui::t_red(),
            Verdict::Couldnt => ui::t_amber(),
            Verdict::Running => ui::t_blue(),
            Verdict::None => ui::muted(),
        };
        let checker = self.avatar(Member::Checker, 22.);
        let mut proof = div()
            .px(px(16.))
            .py(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(checker)
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(verdict_color))
                            .child(said),
                    )
                    .when(!job.checking, |d| {
                        d.child(
                            ui::ghost_i(("recheck", id as usize), IconName::RotateCw, "Run again")
                                .h(px(26.))
                                .px(px(9.))
                                .text_size(px(12.))
                                .on_click(cx.listener(move |this, _, _, cx| this.recheck(id, cx))),
                        )
                    }),
            );
        if job.checking {
            proof = proof.child(ui::sweep(ui::BLUE, 0.).h(px(3.)));
        }
        for (n, check) in job.checks.iter().enumerate() {
            let (mark, col) = if check.ok {
                ("pass", ui::t_green())
            } else if check.infra == work::IGNORED {
                ("ignored", ui::faint())
            } else if !check.infra.is_empty() {
                ("couldn't run", ui::t_amber())
            } else {
                ("fail", ui::t_red())
            };
            proof = proof.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(12.5))
                    .child(
                        div()
                            .flex_1()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(ui::body()))
                            .child(check.command.clone()),
                    )
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .text_color(c(ui::faint()))
                            .child(format!("{}s", check.secs)),
                    )
                    .child(div().w(px(84.)).text_color(c(col)).child(mark)),
            );
            if !check.ok && !check.tail.trim().is_empty() {
                let tail: Vec<&str> = check.tail.lines().rev().take(14).collect();
                let tail: Vec<&str> = tail.into_iter().rev().collect();
                proof = proof.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(c(ui::well()))
                        .border_1()
                        .border_color(c(ui::line()))
                        .font_family(ui::MONO)
                        .text_size(px(11.5))
                        .line_height(px(17.))
                        .text_color(c(ui::body()))
                        .child(tail.join("\n")),
                );
            }
            // A check that fails on its own: a job of its own, or leave it.
            if !check.ok && check.infra.is_empty() && !job.checking {
                let cmd = check.command.clone();
                let (fix_cmd, tail) = (cmd.clone(), check.tail.clone());
                let folder = job.folder.clone();
                proof = proof.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            ui::ghost(("check-fix", id as usize * 64 + n), "Start a job to fix it")
                                .h(px(26.))
                                .text_size(px(12.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.fix_check(id, &fix_cmd, &tail, folder.clone(), cx)
                                })),
                        )
                        .child(
                            ui::ghost(
                                ("check-ignore", id as usize * 64 + n),
                                "Ignore for this job",
                            )
                            .h(px(26.))
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.ignore_check(id, &cmd, cx)),
                            ),
                        ),
                );
            }
        }
        // Approve ahead of the checks: it merges the moment they're green.
        if job.status == Status::Review && verdict != Verdict::Pass && !job.checks.is_empty()
            || job.status == Status::Review && job.checking
        {
            let on = job.merge_on_green;
            proof = proof.child(
                div()
                    .id(("merge-green", id as usize))
                    .mt(px(4.))
                    .pt(px(10.))
                    .border_t_1()
                    .border_color(c(ui::line2()))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::ink()))
                                    .child("Merge when checks pass"),
                            )
                            .child(div().text_size(px(12.)).text_color(c(ui::muted())).child(
                                "Approve now, and it merges the moment every check is green.",
                            )),
                    )
                    .child(super::prefs::switch(on))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(j) = this.crew.store.job_mut(id) {
                            j.merge_on_green = !on;
                        }
                        this.crew.store.save();
                        cx.notify();
                    })),
            );
        }
        proof
    }

    /// The workspace's Checks tab: Checker's run as a header with Run all
    /// again, a mono tally, one row per check with its output when it
    /// failed, Merge when checks pass, and where the checks come from.
    pub(super) fn checks_tab(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let n = job.checks.len();
        let checker = self.avatar(Member::Checker, 22.);
        let when = job
            .ended
            .map(|t| match super::store::ago(t).as_str() {
                "just now" => "just now".to_string(),
                a => format!("{a} ago"),
            })
            .unwrap_or_default();
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(checker)
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(if job.checking {
                        "Checker is running the checks".to_string()
                    } else if n == 0 {
                        "No checks have run".to_string()
                    } else {
                        format!("Checker ran {n} check{}", if n == 1 { "" } else { "s" })
                    }),
            )
            .when(!job.checking && !when.is_empty() && n > 0, |d| {
                d.child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(11.5))
                        .text_color(c(ui::faint()))
                        .child(when),
                )
            })
            .child(div().flex_1())
            .when(!job.checking && job.mode.writes(), |d| {
                d.child(
                    ui::dbtn_i(
                        "checks-all",
                        IconName::RotateCw,
                        "Run all again",
                        ui::Btn::Plain,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.recheck(id, cx))),
                )
            });

        let pass = job.checks.iter().filter(|ch| ch.ok).count();
        let ignored = job
            .checks
            .iter()
            .filter(|ch| ch.infra == work::IGNORED)
            .count();
        let couldnt = job
            .checks
            .iter()
            .filter(|ch| !ch.ok && !ch.infra.is_empty() && ch.infra != work::IGNORED)
            .count();
        let fail = n - pass - ignored - couldnt;
        let mut tally = div()
            .flex()
            .gap(px(6.))
            .font_family(ui::MONO)
            .text_size(px(11.5));
        let mut parts: Vec<(String, u32)> = Vec::new();
        if pass > 0 {
            parts.push((format!("{pass} passed"), ui::t_green()));
        }
        if fail > 0 {
            parts.push((format!("{fail} failed"), ui::t_red()));
        }
        if couldnt > 0 {
            parts.push((format!("{couldnt} couldn't run"), ui::t_amber()));
        }
        if ignored > 0 {
            parts.push((format!("{ignored} ignored"), ui::faint()));
        }
        for (i, (t, col)) in parts.into_iter().enumerate() {
            if i > 0 {
                tally = tally.child(div().text_color(c(ui::faint())).child("\u{b7}"));
            }
            tally = tally.child(div().text_color(c(col)).child(t));
        }

        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(6.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .overflow_hidden();
        if job.checking {
            list = list.child(ui::sweep(ui::BLUE, 0.).h(px(3.)));
        }
        for (k, check) in job.checks.iter().enumerate() {
            let ignored = check.infra == work::IGNORED;
            let (icon, col) = if check.ok {
                (IconName::Check, ui::t_green())
            } else if ignored {
                (IconName::Minus, ui::faint())
            } else if !check.infra.is_empty() {
                (IconName::TriangleAlert, ui::t_amber())
            } else {
                (IconName::X, ui::t_red())
            };
            let failed = !check.ok && check.infra.is_empty();
            let tail: Vec<&str> = check.tail.lines().rev().take(12).collect();
            let tail: Vec<String> = tail.into_iter().rev().map(str::to_string).collect();
            let mut row = div()
                .px(px(12.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .when(k > 0, |d| d.border_t_1().border_color(c(ui::line())))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .size(px(18.))
                                .flex_none()
                                .rounded(px(3.))
                                .bg(ca(col, 0.12))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(icon).size(px(12.)).text_color(c(col))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(c(ui::ink()))
                                        .child(check_name(&check.command)),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .font_family(ui::MONO)
                                        .text_size(px(11.))
                                        .text_color(c(ui::faint()))
                                        .child(if check.infra.is_empty() || ignored {
                                            check.command.clone()
                                        } else {
                                            format!("{} \u{b7} {}", check.command, check.infra)
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .font_family(ui::MONO)
                                .text_size(px(11.))
                                .text_color(c(ui::faint()))
                                .child(format!("{}s", check.secs)),
                        )
                        .when(!job.checking && job.mode.writes(), |d| {
                            d.child(
                                ui::dbtn_i(
                                    ("check-again", k),
                                    IconName::RotateCw,
                                    "Run again",
                                    ui::Btn::Ghost,
                                )
                                .tooltip(|window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(
                                        "Checker runs every check again",
                                    )
                                    .build(window, cx)
                                })
                                .on_click(cx.listener(move |this, _, _, cx| this.recheck(id, cx))),
                            )
                        }),
                );
            if !check.ok && !ignored && !tail.iter().all(|l| l.trim().is_empty()) {
                let mut out = div()
                    .ml(px(28.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(4.))
                    .bg(c(ui::panel()))
                    .border_1()
                    .border_color(c(ui::line_hi()))
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .line_height(px(19.))
                    .text_color(c(ui::body()));
                for (i, l) in tail.into_iter().enumerate() {
                    out = out.child(
                        div()
                            .when(i == 0 && failed, |d| d.text_color(c(ui::t_red())))
                            .child(l),
                    );
                }
                row = row.child(out);
            }
            if failed && !job.checking {
                let cmd = check.command.clone();
                let (fix_cmd, tail) = (cmd.clone(), check.tail.clone());
                let folder = job.folder.clone();
                row = row.child(
                    div()
                        .ml(px(28.))
                        .flex()
                        .gap(px(6.))
                        .child(
                            ui::dbtn(("check-fix", k), "Start a job to fix it", ui::Btn::Plain)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.fix_check(id, &fix_cmd, &tail, folder.clone(), cx)
                                })),
                        )
                        .child(
                            ui::dbtn(("check-ignore", k), "Ignore for this job", ui::Btn::Ghost)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.ignore_check(id, &cmd, cx)
                                })),
                        ),
                );
            }
            list = list.child(row);
        }

        let on = job.merge_on_green;
        let merge =
            (job.status == Status::Review).then(|| {
                div()
                    .id(("merge-green-tab", id as usize))
                    .px(px(14.))
                    .py(px(12.))
                    .rounded(px(6.))
                    .bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child("Merge when checks pass"),
                            )
                            .child(div().text_size(px(12.)).text_color(c(ui::body())).child(
                                "Approve now, and it merges the moment every check is green.",
                            )),
                    )
                    .child(super::prefs::switch(on))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(j) = this.crew.store.job_mut(id) {
                            j.merge_on_green = !on;
                        }
                        this.crew.store.save();
                        cx.notify();
                    }))
            });

        let source = check_source(&job.folder.path);
        let foot = div()
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .text_size(px(12.))
            .text_color(c(ui::body()))
            .child("Checks come from")
            .child(
                div()
                    .when(source != "the project", |d| {
                        d.font_family(ui::MONO).text_color(c(ui::ink()))
                    })
                    .child(format!("{source}.")),
            )
            .child("Change them in")
            .child(
                div()
                    .id("checks-settings")
                    .cursor_pointer()
                    .text_color(c(ui::accent_text()))
                    .hover(|s| s.underline())
                    .child("Settings, How work runs.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.go(super::Place::Settings(super::Pane::Work), cx)
                    })),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(head)
            .when(n > 0, |d| d.child(tally))
            .when(n > 0 || job.checking, |d| d.child(list))
            .when(n == 0 && !job.checking, |d| {
                d.child(div().text_size(px(13.)).text_color(c(ui::body())).child(
                    if job.mode.writes() {
                        "Checker runs the project's checks when the bot finishes."
                    } else {
                        "A job that only reads has no checks to run."
                    },
                ))
            })
            .children(merge)
            .when(job.mode.writes(), |d| d.child(foot))
    }

    /// The review on its own, without the feed's indent: for the peek pane
    /// and the workspace's Changes tab.
    pub(super) fn review_body(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if job.status == Status::Shipped && !job.shipped.is_empty() {
            return Some(
                div()
                    .ml(px(40.))
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(12.))
                    .bg(c(ui::ok_tone().1))
                    .border_1()
                    .border_color(c(ui::ok_tone().2))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(13.))
                    .text_color(c(ui::t_green()))
                    .child(div().size(px(7.)).rounded_full().bg(c(ui::GREEN)))
                    .child(job.shipped.clone()),
            );
        }
        if job.status == Status::Shipping {
            return Some(self.pr_card(job, cx));
        }
        if job.status != Status::Review || job.changes.is_empty() {
            return None;
        }
        let id = job.id;
        let (files, added, removed) = (job.changes.len(), job.lines().0, job.lines().1);
        // Head: where it stands, the size of the change, and the branch.
        let head = div()
            .px(px(16.))
            .py(px(12.))
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(10.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(super::home::pill("To review", ui::ask_tone()))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!(
                        "{files} {}",
                        if files == 1 { "file" } else { "files" }
                    )),
            )
            .child(
                div()
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_color(c(ui::t_green()))
                            .child(format!("+{added}")),
                    )
                    .child(
                        div()
                            .text_color(c(ui::t_red()))
                            .child(format!("\u{2212}{removed}")),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c(ui::muted()))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(branch_chip(&job.branch))
                    .child("into")
                    .child(branch_chip(&job.base)),
            );

        // Checker's run of the checks.
        let proof = self.checks_block(job, cx);

        // The files, each one opening its diff.
        let mut list = div().flex().flex_col();
        for (i, f) in job.changes.iter().enumerate() {
            let open = self
                .diff_open
                .as_ref()
                .is_some_and(|(j, p)| *j == id && *p == f.path);
            let path = f.path.clone();
            list = list.child(
                div()
                    .id(("change", i))
                    .h(px(32.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .when(i > 0, |d| d.border_t_1().border_color(c(ui::line2())))
                    .when(open, |d| d.bg(c(ui::hover())))
                    .hover(|s| s.bg(c(ui::hover())))
                    .child(
                        div()
                            .w(px(14.))
                            .font_family(ui::MONO)
                            .text_size(px(11.))
                            .text_color(c(match f.status.chars().next() {
                                Some('A') => ui::t_green(),
                                Some('D') => ui::t_red(),
                                _ => ui::t_amber(),
                            }))
                            .child(f.status.chars().next().unwrap_or('M').to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(ui::body()))
                            .child(f.path.clone()),
                    )
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .flex()
                            .gap(px(6.))
                            .when(f.added > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(c(ui::t_green()))
                                        .child(format!("+{}", f.added)),
                                )
                            })
                            .when(f.removed > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(c(ui::t_red()))
                                        .child(format!("\u{2212}{}", f.removed)),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let key = (id, path.clone());
                        if this.diff_open.as_ref() == Some(&key) {
                            this.diff_open = None;
                        } else {
                            // Read once on opening, not on every frame.
                            this.diff_text = this
                                .crew
                                .store
                                .job(id)
                                .map(|j| work::diff(j, &key.1))
                                .unwrap_or_default();
                            this.diff_open = Some(key);
                        }
                        cx.notify();
                    })),
            );
            if open {
                let text = self.diff_text.clone();
                list = list.child(self.diff_lines(id, &f.path, &text, DIFF_LINES, cx));
            }
        }

        // The decision. GitHub projects can switch to merging here, and back.
        let label = self.approve_label(job);
        let notes = self.line_notes.iter().filter(|n| n.job == id).count();
        let way = self.way(job);
        let github = ship::remote_url(&job.folder.path).is_some_and(|u| ship::is_github(&u));
        let switch = match way {
            ship::Way::PullRequest => Some(("Merge here instead", ship::Way::Merge)),
            ship::Way::Merge if github => Some(("Open a PR instead", ship::Way::PullRequest)),
            ship::Way::Merge => None,
        };
        let foot = div()
            .px(px(16.))
            .py(px(12.))
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.))
            .border_t_1()
            .border_color(c(ui::line()))
            .child(self.approve_button(("approve", id as usize), job, label, cx))
            .when_some(switch, |d, (words, to)| {
                d.child(
                    div()
                        .id(("way", id as usize))
                        .text_size(px(12.))
                        .text_color(c(ui::muted()))
                        .cursor_pointer()
                        .hover(|s| s.text_color(c(ui::ink())))
                        .child(words)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_way(id, to, cx))),
                )
            })
            .when(notes > 0, |d| {
                d.child(
                    ui::ghost_i(
                        ("send-notes", id as usize),
                        IconName::MessageSquareReply,
                        format!(
                            "Send back with {notes} comment{}",
                            if notes == 1 { "" } else { "s" }
                        ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.send_line_notes(id, cx))),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(160.))
                    .text_size(px(12.))
                    .text_color(c(ui::faint()))
                    .child(if notes > 0 {
                        "Or write below to add more words."
                    } else {
                        "Open a file to see its diff and comment on a line, or write below to send it back."
                    }),
            )
            // A clash card above already offers Discard; don't offer it twice.
            .when(job.conflict.is_empty(), |d| {
                d.child(
                    ui::ghost_i(("discard", id as usize), IconName::Trash, "Discard")
                        .text_color(c(ui::t_red()))
                        .on_click(cx.listener(move |this, _, _, cx| this.discard_job(id, cx))),
                )
            });

        Some(
            div()
                .rounded(px(12.))
                .bg(c(ui::panel()))
                .border_1()
                .border_color(ca(ui::AMBER, 0.35))
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(head)
                .child(proof)
                .child(list)
                .child(foot),
        )
    }
}

impl Shell {
    /// The workspace's Changes tab, as the design draws it: the size of the
    /// change and the branch with the buttons to decide, the files, and the
    /// open file's diff, where a click on a line comments on it. A job on its
    /// way out, or out, shows its pull request or where it went instead.
    pub(super) fn changes_pane(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if matches!(job.status, Status::Shipping | Status::Shipped) {
            return self.review_body(job, cx);
        }
        if job.changes.is_empty() {
            return None;
        }
        let id = job.id;
        // The open file: the one you picked, else the first.
        let picked = self
            .diff_open
            .as_ref()
            .filter(|(j, p)| *j == id && job.changes.iter().any(|f| &f.path == p))
            .map(|(_, p)| p.clone());
        let path = match picked {
            Some(p) => p,
            None => {
                let first = job.changes[0].path.clone();
                self.diff_text = work::diff(job, &first);
                self.diff_open = Some((id, first.clone()));
                first
            }
        };
        let (added, removed) = job.lines();
        let files = job.changes.len();
        let review = job.status == Status::Review;
        let held = review && !job.held.is_empty() && !job.belt_ok;
        let blocked = held || !job.conflict.is_empty();
        let failing = !job.checking && work::verdict(&job.checks, false).1 == Verdict::Fail;
        let notes = self.line_notes.iter().filter(|n| n.job == id).count();
        let full = self.approve_label(job);
        let way = self.way(job);
        let github = ship::remote_url(&job.folder.path).is_some_and(|u| ship::is_github(&u));
        let switch = match way {
            ship::Way::PullRequest => Some(("Merge here instead", ship::Way::Merge)),
            ship::Way::Merge if github => Some(("Open a PR instead", ship::Way::PullRequest)),
            ship::Way::Merge => None,
        };
        let mono = |t: String, col: u32| {
            div()
                .flex_none()
                .font_family(ui::MONO)
                .text_size(px(12.))
                .text_color(c(col))
                .child(t)
        };
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(12.5))
            .text_color(c(ui::body()))
            .child(
                div()
                    .flex_none()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(ui::ink()))
                    .child(format!(
                        "{files} {}",
                        if files == 1 { "file" } else { "files" }
                    )),
            )
            .child(mono(format!("+{added}"), ui::t_green()))
            .child(mono(format!("\u{2212}{removed}"), ui::t_red()))
            .when(!job.branch.is_empty(), |d| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(ui::MONO)
                        .text_size(px(12.))
                        .child(format!("{} \u{2192} {}", job.branch, job.base)),
                )
            })
            .child(div().flex_1())
            .when(review, |d| {
                let approve_words = if failing { "Approve anyway" } else { "Approve" };
                d.child(
                    ui::dbtn("ws-discard", "Discard", ui::Btn::Ghost)
                        .on_click(cx.listener(move |this, _, _, cx| this.discard_job(id, cx))),
                )
                .child(
                    ui::dbtn(
                        "ws-send-back",
                        if notes > 0 {
                            format!("Send back \u{b7} {notes}")
                        } else {
                            "Send back".to_string()
                        },
                        ui::Btn::Plain,
                    )
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(if notes > 0 {
                            "Send your comments on its lines back"
                        } else {
                            "Say what to change in the box on the left"
                        })
                        .build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.line_notes.iter().any(|n| n.job == id) {
                            this.send_line_notes(id, cx);
                        } else {
                            this.live_ask = false;
                            this.live_input.read(cx).focus_handle(cx).focus(window, cx);
                        }
                    })),
                )
                .when(blocked, |d| {
                    d.child(
                        div()
                            .flex_none()
                            .font_family(ui::MONO)
                            .text_size(px(10.5))
                            .text_color(c(ui::faint()))
                            .child(if held { "HELD" } else { "CLASH" }),
                    )
                })
                .child(
                    ui::dbtn("ws-approve", approve_words, ui::Btn::Primary)
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(full.clone())
                                .build(window, cx)
                        })
                        .when(blocked, |d| d.opacity(0.35).cursor_default())
                        .when(!blocked, |d| {
                            d.on_click(cx.listener(move |this, _, _, cx| this.approve(id, cx)))
                        }),
                )
            });

        // The files; the open one is shaded.
        let mut list = div().flex().flex_col();
        for (i, f) in job.changes.iter().enumerate() {
            let on = f.path == path;
            let p = f.path.clone();
            list = list.child(
                div()
                    .id(("ws-file", i))
                    .h(px(23.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(c(ui::line()))
                    .cursor_pointer()
                    .when(on, |d| d.bg(c(ui::hover())).rounded_t(px(4.)))
                    .when(!on, |d| d.hover(|s| s.bg(c(ui::hover()))))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(ui::ink()))
                            .child(f.path.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .gap(px(6.))
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .when(f.added > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(c(ui::t_green()))
                                        .child(format!("+{}", f.added)),
                                )
                            })
                            .when(f.removed > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(c(ui::t_red()))
                                        .child(format!("\u{2212}{}", f.removed)),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.diff_text = this
                            .crew
                            .store
                            .job(id)
                            .map(|j| work::diff(j, &p))
                            .unwrap_or_default();
                        this.diff_open = Some((id, p.clone()));
                        this.line_at = None;
                        cx.notify();
                    })),
            );
        }

        // The open file's diff in a box: its name and first hunk on top.
        let text = self.diff_text.clone();
        let hunk = text
            .lines()
            .find(|l| l.starts_with("@@"))
            .map(|l| {
                l.splitn(4, ' ')
                    .take(4)
                    .collect::<Vec<_>>()
                    .join(" ")
                    .split(" @@")
                    .next()
                    .map(|h| format!("{h} @@"))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let lines = self.diff_lines(id, &path, &text, DIFF_LINES, cx);
        let hint = (self.line_at.as_ref().is_none_or(|(j, _, _)| *j != id) && review).then(|| {
            div()
                .mx(px(12.))
                .mb(px(12.))
                .ml(px(62.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(5.))
                .bg(c(ui::bg()))
                .border_1()
                .border_color(c(ui::line()))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.5))
                .child(
                    div()
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .bg(c(ui::pick_fill()))
                        .text_color(c(ui::pick_on()))
                        .font_family(ui::MONO)
                        .text_size(px(10.))
                        .child(super::channel::you_letter()),
                )
                .child(div().text_color(c(ui::faint())).child(format!(
                    "Click a line to comment on it, and it goes back to {}\u{2026}",
                    job.bot.name()
                )))
        });
        let diff = div()
            .rounded(px(6.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .overflow_hidden()
            .child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .flex()
                    .gap(px(10.))
                    .bg(c(ui::hover()))
                    .border_b_1()
                    .border_color(c(ui::line()))
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .text_color(c(ui::ink()))
                    .child(div().min_w_0().truncate().child(path.clone()))
                    .child(div().flex_1())
                    .child(div().flex_none().text_color(c(ui::faint())).child(hunk)),
            )
            .child(lines)
            .children(hint);

        let way_line = switch.filter(|_| review).map(|(words, to)| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(c(ui::faint()))
                .child(match way {
                    ship::Way::PullRequest => "Approve opens a pull request.".to_string(),
                    ship::Way::Merge => format!("Approve merges into {} here.", job.base),
                })
                .child(
                    div()
                        .id("ws-way")
                        .text_color(c(ui::accent_text()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .child(words)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_way(id, to, cx))),
                )
        });

        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(head)
                .children(way_line)
                .child(list)
                .child(diff),
        )
    }
}

impl Shell {
    /// The pull request Shipper opened: its checks, auto-merge, and the buttons
    /// to merge now or close it.
    fn pr_card(&mut self, job: &Job, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let shipper = self.avatar(Member::Shipper, 22.);
        let opening = job.pr.is_empty();
        let failed = !job.pr_failed.is_empty() && job.pr_line.contains("failed");
        let line_color = if opening {
            ui::t_blue()
        } else if failed {
            ui::t_red()
        } else if job.pr_line.starts_with("All") {
            ui::t_green()
        } else {
            ui::t_blue()
        };
        let title = if opening {
            "Opening the pull request".to_string()
        } else if job.pr_number > 0 {
            format!("Pull request #{}", job.pr_number)
        } else {
            "Pull request".to_string()
        };
        let url = job.pr.clone();
        let auto = job.auto_merge;
        let mut card = div()
            .ml(px(40.))
            .rounded(px(12.))
            .bg(c(ui::panel()))
            .border_1()
            .border_color(ca(0x14B8A6, 0.40))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(c(ui::line()))
                    .child(shipper)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(div().text_size(px(12.)).text_color(c(line_color)).child(
                                if opening {
                                    "Squashing, catching up with the base and pushing".to_string()
                                } else if job.pr_line.is_empty() {
                                    "Looking at its checks".to_string()
                                } else {
                                    job.pr_line.clone()
                                },
                            )),
                    )
                    .when(!opening, |d| {
                        d.child(
                            ui::ghost_i(
                                ("pr-open", id as usize),
                                IconName::Github,
                                "View on GitHub",
                            )
                            .h(px(28.))
                            .text_size(px(12.5))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                    }),
            );
        if opening || (!failed && !job.pr_line.starts_with("All")) {
            card = card.child(ui::sweep(0x14B8A6, 0.).h(px(2.)).rounded_none());
        }
        if !opening {
            card = card.child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .id(("auto", id as usize))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .child(toggle(auto))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(c(ui::body()))
                                    .child("Merge when checks pass"),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_auto_merge(id, !auto, cx)
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::ghost_i(
                            ("pr-close", id as usize),
                            IconName::GitPullRequestClosed,
                            "Close PR",
                        )
                        .h(px(28.))
                        .text_size(px(12.5))
                        .on_click(cx.listener(move |this, _, _, cx| this.land_pr(id, false, cx))),
                    )
                    .child(
                        ui::solid_i(("pr-merge", id as usize), IconName::GitMerge, "Merge now")
                            .h(px(28.))
                            .text_size(px(12.5))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.land_pr(id, true, cx)),
                            ),
                    ),
            );
        }
        card
    }
}

/// A small on/off switch.
fn toggle(on: bool) -> Div {
    div()
        .w(px(30.))
        .h(px(18.))
        .flex_none()
        .rounded_full()
        .bg(c(if on { ui::GREEN } else { ui::off() }))
        .relative()
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 14. } else { 2. }))
                .size(px(14.))
                .rounded_full()
                .bg(c(0xFFFFFF)),
        )
}

fn branch_chip(name: &str) -> Div {
    div()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(5.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line()))
        .font_family(ui::MONO)
        .text_size(px(11.5))
        .text_color(c(ui::body()))
        .child(name.to_string())
}

/// A comment on one line of a job's diff, waiting to go back with Send back.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LineNote {
    pub job: u64,
    pub path: String,
    /// The line's place in the diff as shown, to draw the comment under it.
    pub at: usize,
    /// The line number in the changed file ("14"), or where a removed line
    /// was ("14, removed").
    pub line: String,
    /// The line itself, as the diff shows it.
    pub code: String,
    pub text: String,
}

/// The lines of a unified diff from its first hunk, each with its number in
/// the new file (for added and kept lines) or where it was (removed lines).
pub(super) fn numbered(diff: &str) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    let mut new_line = 0usize;
    let mut started = false;
    for l in diff.lines() {
        if l.starts_with("@@") {
            started = true;
            // @@ -a,b +c,d @@
            new_line = l
                .split_whitespace()
                .find(|w| w.starts_with('+'))
                .and_then(|w| w[1..].split(',').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            out.push((String::new(), l));
            continue;
        }
        if !started {
            continue;
        }
        if l.starts_with('-') {
            out.push((format!("{new_line}, removed"), l));
        } else {
            out.push((new_line.to_string(), l));
            new_line += 1;
        }
    }
    out
}

/// What the bot is told for a set of line comments.
pub(super) fn notes_message(notes: &[&LineNote]) -> String {
    let mut out = String::from("Changes asked for on these lines of your diff:\n");
    for n in notes {
        let code = n.code.get(1..).unwrap_or("").trim();
        let code: String = code.chars().take(100).collect();
        out.push_str(&format!(
            "\n- `{}:{}`{}: {}",
            n.path,
            n.line,
            if code.is_empty() {
                String::new()
            } else {
                format!(" (`{code}`)")
            },
            n.text.trim()
        ));
    }
    out
}

impl Shell {
    /// A job's diff for one file, coloured line by line; click a line to
    /// comment on it.
    pub(super) fn diff_lines(
        &mut self,
        job: u64,
        path: &str,
        diff: &str,
        limit: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut body = div()
            .py(px(6.))
            .font_family(ui::MONO)
            .text_size(px(12.))
            .line_height(px(20.));
        let lines = numbered(diff);
        if lines.is_empty() {
            return body.child(
                div().px(px(16.)).text_color(c(ui::faint())).child(
                    "Couldn't show this file's changes here. Open the folder to look at it.",
                ),
            );
        }
        let total = lines.len();
        let limit = limit.min(DIFF_LINES);
        for (at, (num, l)) in lines.into_iter().take(limit).enumerate() {
            let hunk = l.starts_with("@@");
            let (mark, bg) = if l.starts_with('+') {
                ("+", Some(ca(ui::t_green(), 0.08)))
            } else if l.starts_with('-') {
                ("\u{2212}", Some(ca(ui::t_red(), 0.07)))
            } else {
                (" ", None)
            };
            let mark_col = if l.starts_with('+') {
                ui::t_green()
            } else {
                ui::t_red()
            };
            let picked = self
                .line_at
                .as_ref()
                .is_some_and(|(j, p, a)| *j == job && p == path && *a == at);
            let p = path.to_string();
            let shown_num = if hunk {
                String::new()
            } else {
                num.split(',').next().unwrap_or("").to_string()
            };
            let code = if hunk {
                l.to_string()
            } else {
                l.get(1..).unwrap_or("").to_string()
            };
            body = body.child(
                div()
                    .id(SharedString::from(format!("dl-{job}-{path}-{at}")))
                    .pr(px(10.))
                    .flex()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_color(c(if hunk { ui::faint() } else { ui::ink() }))
                    .when_some(bg, |d, bg| d.bg(bg))
                    .when(picked, |d| d.bg(ca(ui::AMBER, 0.16)))
                    .when(!hunk, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(ca(ui::AMBER, 0.10)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.line_at = Some((job, p.clone(), at));
                                // The box is drawn on the next frame: focus it then.
                                this.focus_moved = std::time::Instant::now();
                                let input = this.line_input.clone();
                                window.on_next_frame(move |window, cx| {
                                    input.update(cx, |i, cx| i.focus(window, cx))
                                });
                                cx.notify();
                            }))
                    })
                    .child(
                        div()
                            .w(px(38.))
                            .flex_none()
                            .pr(px(10.))
                            .flex()
                            .justify_end()
                            .text_color(c(ui::faint()))
                            .child(shown_num),
                    )
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .text_color(c(mark_col))
                            .child(mark),
                    )
                    .child(if code.is_empty() {
                        " ".to_string()
                    } else {
                        code
                    }),
            );
            // Comments already on this line.
            for (i, note) in self
                .line_notes
                .iter()
                .enumerate()
                .filter(|(_, x)| x.job == job && x.path == path && x.at == at)
            {
                let text = note.text.clone();
                let line = note.line.split(',').next().unwrap_or("").to_string();
                body = body.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .bg(c(ui::raised()))
                        .border_l(px(3.))
                        .border_color(c(ui::accent_bar()))
                        .border_t_1()
                        .border_b_1()
                        .font_family("Geist")
                        .child(
                            div()
                                .size(px(20.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .bg(c(ui::pick_fill()))
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(ui::pick_on()))
                                .child("Y"),
                        )
                        .child(
                            div()
                                .flex_1()
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
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(c(ui::ink()))
                                                .child("You"),
                                        )
                                        .child(
                                            div()
                                                .font_family(ui::MONO)
                                                .text_size(px(11.))
                                                .text_color(c(ui::faint()))
                                                .child(format!("line {line}")),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(19.))
                                        .whitespace_normal()
                                        .text_color(c(ui::ink()))
                                        .child(text),
                                ),
                        )
                        .child(
                            ui::icon_btn(SharedString::from(format!("dn-{job}-{i}")), IconName::X)
                                .size(px(20.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if i < this.line_notes.len() {
                                        this.line_notes.remove(i);
                                    }
                                    cx.notify();
                                })),
                        ),
                );
            }
            // The box to write one, under the picked line.
            if picked {
                body = body.child(
                    div()
                        .mx(px(16.))
                        .my(px(4.))
                        .h(px(32.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(8.))
                        .bg(c(ui::panel()))
                        .border_1()
                        .border_color(ca(ui::AMBER, 0.5))
                        .font_family("Geist")
                        .text_size(px(12.5))
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&self.line_input)
                                    .appearance(false)
                                    .bordered(false),
                            ),
                        )
                        .child(
                            ui::ghost(SharedString::from(format!("dl-cancel-{job}")), "Cancel")
                                .h(px(24.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.line_at = None;
                                    cx.notify();
                                })),
                        ),
                );
            }
        }
        if total > limit && limit == DIFF_LINES {
            body = body.child(
                div()
                    .px(px(16.))
                    .text_color(c(ui::faint()))
                    .child(format!("{} more lines", total - limit)),
            );
        }
        body
    }

    /// Adds the comment typed on the picked line.
    pub(super) fn add_line_note(&mut self, text: String, cx: &mut Context<Self>) {
        let Some((job, path, at)) = self.line_at.take() else {
            return;
        };
        let lines = numbered(&self.diff_text);
        let Some((line, code)) = lines.get(at).map(|(n, l)| (n.clone(), l.to_string())) else {
            return;
        };
        self.line_notes.push(LineNote {
            job,
            path,
            at,
            line,
            code,
            text,
        });
        cx.notify();
    }

    /// Sends a job back with its line comments and any more words, as one
    /// message.
    pub(super) fn send_back_with(&mut self, id: u64, words: String, cx: &mut Context<Self>) {
        let notes: Vec<&LineNote> = self.line_notes.iter().filter(|n| n.job == id).collect();
        let mut text = if notes.is_empty() {
            String::new()
        } else {
            notes_message(&notes)
        };
        let words = words.trim();
        if !words.is_empty() {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(words);
        }
        if text.is_empty() {
            return;
        }
        self.line_notes.retain(|n| n.job != id);
        self.line_at = None;
        self.follow_up(id, text, cx);
    }

    /// Sends a job back with its line comments, as one message.
    pub(super) fn send_line_notes(&mut self, id: u64, cx: &mut Context<Self>) {
        let notes: Vec<&LineNote> = self.line_notes.iter().filter(|n| n.job == id).collect();
        if notes.is_empty() {
            return;
        }
        let text = notes_message(&notes);
        self.line_notes.retain(|n| n.job != id);
        self.line_at = None;
        self.diff_open = None;
        self.follow_up(id, text, cx);
    }
}

/// A check's short name from its command: "npm test" reads "Tests".
pub(super) fn check_name(command: &str) -> String {
    let low = command.to_lowercase();
    for (word, name) in [
        ("clippy", "Clippy"),
        ("lint", "Lint"),
        ("eslint", "Lint"),
        ("fmt", "Format"),
        ("format", "Format"),
        ("prettier", "Format"),
        ("typecheck", "Types"),
        ("tsc", "Types"),
        ("mypy", "Types"),
        ("test", "Tests"),
        ("pytest", "Tests"),
        ("build", "Build"),
    ] {
        if low
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|w| w == word)
        {
            return name.into();
        }
    }
    let last = command.split_whitespace().last().unwrap_or(command);
    let mut chars = last.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars).collect(),
        None => "Check".into(),
    }
}

/// The file the project's checks come from.
fn check_source(dir: &str) -> &'static str {
    let d = std::path::Path::new(dir);
    for f in [
        "squad.toml",
        "package.json",
        "Cargo.toml",
        "pyproject.toml",
        "go.mod",
        "Makefile",
    ] {
        if d.join(f).exists() {
            return f;
        }
    }
    "the project"
}

#[cfg(test)]
mod tests {
    use super::{check_name, notes_message, numbered, LineNote};

    #[test]
    fn checks_read_by_what_they_do() {
        assert_eq!(check_name("npm test"), "Tests");
        assert_eq!(check_name("pnpm lint"), "Lint");
        assert_eq!(check_name("cargo clippy -- -D warnings"), "Clippy");
        assert_eq!(check_name("make check"), "Check");
    }

    #[test]
    fn lines_are_numbered_from_their_hunk() {
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -10,3 +12,4 @@ fn x\n keep\n-old\n+new\n+more\n";
        let n = numbered(diff);
        let nums: Vec<&str> = n.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(nums, vec!["", "12", "13, removed", "13", "14"]);
    }

    #[test]
    fn comments_read_as_one_message() {
        let a = LineNote {
            job: 1,
            path: "src/a.ts".into(),
            at: 2,
            line: "14".into(),
            code: "+  const total = price * 1.2".into(),
            text: "Use the tax helper".into(),
        };
        let msg = notes_message(&[&a]);
        assert!(
            msg.contains("`src/a.ts:14` (`const total = price * 1.2`): Use the tax helper"),
            "{msg}"
        );
    }
}
