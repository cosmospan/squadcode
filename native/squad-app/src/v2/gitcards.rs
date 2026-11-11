//! Git, where the work is talked about: the lines and cards a job shows in its
//! thread about its branch. Where it stands (its branch, commits on it, how
//! far its base moved), the seatbelt holding it before a push or a merge, and
//! Peek's heads-up when another job changes the same lines.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::store::{Job, Status};
use super::ui::{self, c, ca};
use super::Shell;

/// "squad/apple-pay · 3 commits · 2 behind main", for a job with a branch.
pub fn branch_words(j: &Job) -> Option<(String, String, Option<String>)> {
    if j.branch.is_empty() {
        return None;
    }
    let commits = match j.ahead {
        0 => String::new(),
        1 => "1 commit".to_string(),
        n => format!("{n} commits"),
    };
    let behind = (j.behind > 0).then(|| {
        let base = if j.base.is_empty() {
            "its base"
        } else {
            &j.base
        };
        format!("{} behind {base}", j.behind)
    });
    Some((j.branch.clone(), commits, behind))
}

impl Shell {
    /// The branch a job works on, how many commits it has made there and how
    /// far its base moved since it started.
    pub(super) fn branch_line(&self, j: &Job) -> Option<Div> {
        if !matches!(
            j.status,
            Status::Running | Status::Done | Status::Review | Status::Shipping
        ) {
            return None;
        }
        let (branch, commits, behind) = branch_words(j)?;
        let dot = || div().text_color(c(ui::faint())).child("\u{b7}");
        Some(
            div()
                .mx(px(14.))
                .mb(px(8.))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(8.))
                .bg(c(ui::well()))
                .border_1()
                .border_color(c(ui::line2()))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.))
                .text_color(c(ui::muted()))
                .child(
                    Icon::new(IconName::GitBranch)
                        .size(px(12.))
                        .text_color(c(ui::muted())),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(ui::MONO)
                        .text_color(c(ui::body()))
                        .child(branch),
                )
                .when(!commits.is_empty(), |d| d.child(dot()).child(commits))
                .when_some(behind, |d, b| {
                    d.child(dot())
                        .child(div().text_color(c(ui::t_amber())).child(b))
                }),
        )
    }

    /// The seatbelt held Approve: what it found, and the three ways on.
    pub(super) fn belt_card(&mut self, j: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if j.held.is_empty() || j.status != Status::Review {
            return None;
        }
        let id = j.id;
        let tone = ui::bad_tone();
        let mut rows = div().flex().flex_col();
        for (n, line) in j.held.iter().enumerate() {
            let (place, what) = line
                .split_once(" \u{b7} ")
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .unwrap_or_else(|| (String::new(), line.clone()));
            rows = rows.child(
                div()
                    .px(px(12.))
                    .py(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(12.5))
                    .when(n > 0, |d| d.border_t_1().border_color(c(tone.2)))
                    .child(div().text_color(c(ui::body())).child(what))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .text_color(c(ui::muted()))
                            .child(place),
                    ),
            );
        }
        let anyway = match self.crew.way(j) {
            super::ship::Way::PullRequest => "Push anyway",
            super::ship::Way::Merge => "Merge anyway",
        };
        Some(
            div()
                .mx(px(14.))
                .mb(px(10.))
                .rounded(px(12.))
                .bg(c(tone.1))
                .border_1()
                .border_color(c(tone.2))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .px(px(12.))
                        .pt(px(10.))
                        .pb(px(6.))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(
                            Icon::new(IconName::ShieldAlert)
                                .mt(px(2.))
                                .size(px(14.))
                                .flex_none()
                                .text_color(c(tone.0)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child("Seatbelt: Shipper stopped before it left your computer"),
                        ),
                )
                .child(rows)
                .child(
                    div()
                        .px(px(12.))
                        .py(px(10.))
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(
                            ui::solid(
                                ("belt-take-out", id as usize),
                                format!("Ask {} to take them out", j.bot.name()),
                            )
                            .h(px(28.))
                            .on_click(cx.listener(move |this, _, _, cx| this.take_out(id, cx))),
                        )
                        .child(
                            ui::ghost(("belt-anyway", id as usize), anyway)
                                .h(px(28.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.approve_anyway(id, cx)),
                                ),
                        ),
                ),
        )
    }

    /// Your uncommitted changes a merge set aside and couldn't put back,
    /// because they clash with what it brought in: which files, and bringing
    /// them back with the clash marked or leaving them in the stash.
    pub(super) fn in_the_way_card(&mut self, j: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if j.kept.is_empty() {
            return None;
        }
        let id = j.id;
        let tone = ui::ask_tone();
        let mut rows = div().flex().flex_col();
        for (n, path) in j.in_the_way.iter().enumerate() {
            rows = rows.child(
                div()
                    .px(px(12.))
                    .py(px(5.))
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .text_color(c(ui::body()))
                    .when(n > 0, |d| d.border_t_1().border_color(c(tone.2)))
                    .min_w_0()
                    .truncate()
                    .child(path.clone()),
            );
        }
        let one = j.in_the_way.len() == 1;
        Some(
            div()
                .mx(px(14.))
                .mb(px(10.))
                .rounded(px(12.))
                .bg(c(tone.1))
                .border_1()
                .border_color(c(tone.2))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(12.))
                        .pt(px(10.))
                        .pb(px(2.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(
                            Icon::new(IconName::CircleAlert)
                                .size(px(14.))
                                .text_color(c(tone.0)),
                        )
                        // Its own box, so a narrow panel wraps the title instead of
                        // running it past the card's edge.
                        .child(div().flex_1().min_w_0().child(if one {
                            "Your changes to this file clash with the merge"
                        } else {
                            "Your changes to these files clash with the merge"
                        })),
                )
                .child(
                    div()
                        .px(px(12.))
                        .pb(px(6.))
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .child(format!(
                            "It merged, and your uncommitted changes in {} were set aside for it. They change the same lines, so they weren't put back. Nothing is lost.",
                            j.folder.path
                        )),
                )
                .child(rows)
                .child(
                    div()
                        .px(px(12.))
                        .py(px(10.))
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(
                            ui::solid(("kept-back", id as usize), "Bring them back to sort out")
                                .h(px(28.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.bring_back(id, cx)),
                                ),
                        )
                        .child(
                            ui::ghost(("kept-leave", id as usize), "Leave them in the stash")
                                .h(px(28.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.leave_kept(id, cx)),
                                ),
                        ),
                ),
        )
    }

    /// Peek's heads-up: other jobs in the project change the same lines.
    /// After a send-back: the lesson the crew found in what you said, to keep
    /// for this bot, for the whole crew, or let go. Nothing is kept until you
    /// say so.
    pub(super) fn lesson_card(&mut self, j: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if j.lesson_offer.is_empty() {
            return None;
        }
        let id = j.id;
        let bot = j.bot;
        let face = self.avatar(bot, 18.);
        let text = j.lesson_offer.clone();
        let (keep, all) = (text.clone(), text.clone());
        Some(
            div()
                .mx(px(14.))
                .mb(px(10.))
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
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(face)
                        .child(format!("Keep this as a lesson for {}?", bot.name())),
                )
                .child(
                    div()
                        .pl(px(10.))
                        .border_l_2()
                        .border_color(c(bot.accent()))
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(c(ui::body()))
                        .child(text),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(c(ui::muted()))
                        .child(format!(
                            "{} will follow it in every job in {} from now on. You can forget it any time under Crew, Memory.",
                            bot.name(),
                            j.folder.name
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            ui::solid(("lesson-keep", id as usize), "Keep it")
                                .h(px(28.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.crew.keep_lesson(id, &keep, false);
                                    cx.notify();
                                })),
                        )
                        .child(
                            ui::ghost(("lesson-crew", id as usize), "For the whole crew")
                                .h(px(28.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.crew.keep_lesson(id, &all, true);
                                    cx.notify();
                                })),
                        )
                        .child(
                            ui::ghost(("lesson-skip", id as usize), "Not now")
                                .h(px(28.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.crew.drop_lesson(id);
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }

    pub(super) fn clash_card(&mut self, j: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if j.clash_with.is_empty()
            || !matches!(j.status, Status::Running | Status::Done | Status::Review)
        {
            return None;
        }
        let violet = ui::t_violet();
        let mut card = div()
            .mx(px(14.))
            .mb(px(10.))
            .px(px(12.))
            .py(px(9.))
            .rounded(px(12.))
            .bg(ca(ui::VIOLET, 0.07))
            .border_1()
            .border_color(ca(ui::VIOLET, 0.28))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(Icon::new(IconName::Radar).size(px(14.)).text_color(c(violet)))
                    .child("Clash ahead"),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::muted()))
                    .child("Whichever is approved second will need its branch brought up to date. Nothing in either folder was touched."),
            );
        for (other, words) in &j.clash_with {
            let other = *other;
            let title = self
                .crew
                .store
                .job(other)
                .map(|o| o.label().to_string())
                .unwrap_or_default();
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.5))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(c(ui::body()))
                            .child(format!("#{other} {title} \u{b7} {words}")),
                    )
                    .child(
                        ui::ghost(
                            ("clash-open", (j.id as usize) * 4096 + other as usize),
                            format!("Open #{other}"),
                        )
                        .h(px(26.))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_job(other, cx))),
                    ),
            );
        }
        Some(card)
    }
}

impl Shell {
    /// The Git side of the right sidebar, for one project and thread: the
    /// folder's branch and how it stands against its remote, what to watch out
    /// for, uncommitted files, the crew's branches and the latest commits.
    pub(super) fn git_side(
        &mut self,
        folder: &str,
        jobs: &[Job],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("git-side")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(20.))
            .pb(px(24. + super::DOCK_ROOM))
            .child(self.git_sections(folder, jobs, cx))
    }

    /// The Git side's blocks, one under another, for the sidebar and for the
    /// project's Overview.
    pub(super) fn git_sections(
        &mut self,
        folder: &str,
        jobs: &[Job],
        cx: &mut Context<Self>,
    ) -> Div {
        let body = div().flex().flex_col();
        let quiet = |text: &str| {
            div()
                .text_size(px(12.5))
                .line_height(px(18.))
                .text_color(c(ui::faint()))
                .child(text.to_string())
        };

        let repo = self.crew.repos.get(folder).cloned();
        let Some(repo) = repo else {
            return body.child(section("This folder", quiet("Looking at git\u{2026}")));
        };
        if !repo.is_repo {
            return body.child(section(
                "This folder",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(
                                Icon::new(IconName::FolderOpen)
                                    .size(px(14.))
                                    .text_color(c(ui::muted())),
                            )
                            .child("Not a git repository"),
                    )
                    .child(quiet(
                        "Jobs here change your files directly. With git, each job \
                         gets its own branch and waits for your OK before it merges.",
                    )),
            ));
        }

        // The folder: its branch, then how it stands, as small chips.
        let st = &repo.state;
        let host = if st.host.is_empty() {
            st.remote.clone()
        } else {
            st.host.clone()
        };
        let mut chips: Vec<(u32, String)> = Vec::new();
        if st.remote.is_empty() {
            chips.push((ui::muted(), "No remote".into()));
        } else if st.behind == 0 && st.ahead == 0 {
            chips.push((ui::t_green(), format!("Up to date with {host}")));
        } else {
            if st.behind > 0 {
                chips.push((ui::t_amber(), format!("\u{2193}{} to pull", st.behind)));
            }
            if st.ahead > 0 {
                chips.push((ui::t_blue(), format!("\u{2191}{} to push", st.ahead)));
            }
        }
        chips.push(match repo.changed {
            0 => (ui::t_green(), "Clean".into()),
            1 => (ui::t_amber(), "1 file changed".into()),
            n => (ui::t_amber(), format!("{n} files changed")),
        });
        let path = folder.to_string();
        let head = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Icon::new(IconName::GitBranch)
                            .size(px(15.))
                            .text_color(c(ui::muted())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(if st.branch.is_empty() {
                                "detached HEAD".to_string()
                            } else {
                                st.branch.clone()
                            }),
                    )
                    .when(!st.remote.is_empty(), |d| {
                        d.child(
                            ui::icon_btn("git-fetch", IconName::RotateCw)
                                .size(px(26.))
                                .tooltip(move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(
                                        "Fetch from the remote",
                                    )
                                    .build(window, cx)
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.crew.fetch(&path);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(chips.into_iter().map(|(col, word)| {
                        div()
                            .h(px(22.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(6.))
                            .bg(ca(col, 0.10))
                            .border_1()
                            .border_color(ca(col, 0.22))
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(col))
                            .child(div().size(px(6.)).rounded_full().bg(c(col)))
                            .child(word)
                    })),
            );
        let mut body = body.child(section("This folder", head));

        // The crew's branches, and what to watch out for across them.
        let branched: Vec<&Job> = jobs
            .iter()
            .filter(|j| {
                !j.branch.is_empty()
                    && matches!(
                        j.status,
                        Status::Running | Status::Done | Status::Review | Status::Shipping
                    )
            })
            .collect();
        let mut watch: Vec<(u32, String, u64)> = Vec::new();
        for j in &branched {
            if !j.held.is_empty() {
                watch.push((
                    ui::t_red(),
                    format!("Seatbelt held #{} before it left", j.id),
                    j.id,
                ));
            }
        }
        let mut told: Vec<(u64, u64)> = Vec::new();
        for j in &branched {
            for (other, words) in &j.clash_with {
                let pair = (j.id.min(*other), j.id.max(*other));
                if !told.contains(&pair) {
                    told.push(pair);
                    watch.push((
                        ui::t_violet(),
                        format!("#{} and #{other} clash in {words}", j.id),
                        j.id,
                    ));
                }
            }
        }
        let behind: Vec<&&Job> = branched.iter().filter(|j| j.behind > 0).collect();
        if let Some(first) = behind.first() {
            watch.push((
                ui::t_amber(),
                match behind.len() {
                    1 => format!("#{}'s base moved since it started", first.id),
                    n => format!("{n} branches are behind their base"),
                },
                first.id,
            ));
        }
        if !watch.is_empty() {
            let mut outs = div().flex().flex_col().gap(px(6.));
            for (n, (col, words, id)) in watch.into_iter().enumerate() {
                outs = outs.child(
                    div()
                        .id(("git-watch", n))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(8.))
                        .bg(ca(col, 0.08))
                        .border_1()
                        .border_color(ca(col, 0.20))
                        .cursor_pointer()
                        .hover(|s| s.bg(ca(col, 0.14)))
                        .text_size(px(12.5))
                        .text_color(c(ui::body()))
                        .child(div().size(px(6.)).flex_none().rounded_full().bg(c(col)))
                        .child(div().min_w_0().truncate().child(words))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
                );
            }
            body = body.child(section("Watch out", outs));
        }

        // Uncommitted files in your folder.
        if repo.changed > 0 {
            let mut files = div().flex().flex_col().gap(px(2.));
            for (code, file) in &repo.changes {
                let col = match code.as_str() {
                    "??" | "A" => ui::add(),
                    "D" => ui::del(),
                    _ => ui::t_amber(),
                };
                let letter = if code == "??" { "U" } else { code.as_str() };
                files = files.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .py(px(3.))
                        .child(
                            div()
                                .w(px(14.))
                                .flex_none()
                                .font_family(ui::MONO)
                                .text_size(px(11.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(col))
                                .child(letter.chars().take(1).collect::<String>()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(ui::MONO)
                                .text_size(px(12.))
                                .text_color(c(ui::body()))
                                .child(file.clone()),
                        ),
                );
            }
            if repo.changed > repo.changes.len() {
                files = files.child(quiet(&format!(
                    "and {} more",
                    repo.changed - repo.changes.len()
                )));
            }
            body = body.child(section_n("Not committed", repo.changed, files));
        }

        // The crew's branches.
        let mut list = div().flex().flex_col().gap(px(2.));
        if branched.is_empty() {
            list = list.child(quiet(
                "No job has a branch open. Each job gets its own when it starts.",
            ));
        }
        for j in &branched {
            let id = j.id;
            let dot = match j.status {
                Status::Running => ui::t_blue(),
                Status::Shipping => ui::t_green(),
                _ => ui::t_amber(),
            };
            let mut right = vec![format!("#{id}")];
            if j.ahead > 0 {
                right.push(match j.ahead {
                    1 => "1 commit".to_string(),
                    n => format!("{n} commits"),
                });
            }
            if j.pr_number > 0 {
                right.push(format!("PR #{}", j.pr_number));
            }
            list = list.child(
                div()
                    .id(("git-branch", id as usize))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(8.))
                    .py(px(6.))
                    .mx(px(-8.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .map(|d| super::motion::hover_fill(d, None, ui::hover()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().size(px(6.)).flex_none().rounded_full().bg(c(dot)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(ui::MONO)
                                    .text_size(px(12.))
                                    .text_color(c(ui::ink()))
                                    .child(j.branch.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.5))
                                    .text_color(c(ui::faint()))
                                    .child(right.join(" \u{b7} ")),
                            ),
                    )
                    .child(
                        div()
                            .pl(px(14.))
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(c(ui::muted()))
                            .child(format!("{} \u{b7} {}", j.bot.name(), j.label())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
            );
        }
        body = body.child(section_n("Crew branches", branched.len(), list));

        // The latest commits on your branch.
        let mut log = div().flex().flex_col().gap(px(2.));
        if repo.commits.is_empty() {
            log = log.child(quiet("No commits yet."));
        }
        for (hash, subject, when) in &repo.commits {
            log = log.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .py(px(4.))
                    .child(
                        Icon::new(IconName::GitCommitHorizontal)
                            .size(px(13.))
                            .flex_none()
                            .text_color(c(ui::faint())),
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
                                    .text_color(c(ui::ink()))
                                    .child(subject.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(6.))
                                    .text_size(px(11.))
                                    .text_color(c(ui::faint()))
                                    .child(div().font_family(ui::MONO).child(hash.clone()))
                                    .child("\u{b7}")
                                    .child(when.clone()),
                            ),
                    ),
            );
        }
        let on = if st.branch.is_empty() {
            "Recent commits".to_string()
        } else {
            format!("Recent commits on {}", st.branch)
        };
        body.child(section(&on, log))
    }
}

/// One block of the Git side: its heading, its body, a hairline under it.
fn section(title: &str, inner: Div) -> Div {
    super::live::section(title, inner)
}

/// A block whose heading carries a count on the right.
fn section_n(title: &str, n: usize, inner: Div) -> Div {
    div()
        .py(px(18.))
        .border_b_1()
        .border_color(c(ui::line2()))
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(ui::heading(title, None, Some(n)))
        .child(inner)
}

#[cfg(test)]
mod tests {
    use super::super::store::Job;
    use super::branch_words;

    #[test]
    fn branch_words_read_plainly() {
        let mut j = Job::default();
        assert!(branch_words(&j).is_none());
        j.branch = "squad/apple-pay".into();
        j.base = "main".into();
        assert_eq!(
            branch_words(&j),
            Some(("squad/apple-pay".into(), String::new(), None))
        );
        j.ahead = 3;
        j.behind = 2;
        assert_eq!(
            branch_words(&j),
            Some((
                "squad/apple-pay".into(),
                "3 commits".into(),
                Some("2 behind main".into())
            ))
        );
        j.ahead = 1;
        assert_eq!(branch_words(&j).unwrap().1, "1 commit");
    }
}
