//! Goals: something too big for one job, split into a few jobs for the crew.
//!
//! You describe the goal; Squad Code's own model splits it into two to six jobs, each
//! for the bot whose kind of work it is. You look over the split, drop what you
//! don't want, and start it. The jobs then run one after another, each on its
//! own branch: the next starts once the one before it is shipped, so each job
//! builds on the last. A job that stops or is thrown away pauses the goal
//! until you say what to do.

use std::sync::mpsc::{channel, Receiver};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::agent::{self, Event};
use crate::settings::AppSettings;

use super::data::{Member, Mode};
use super::home;
use super::store::{Folder, Job, Status};
use super::ui::{self, c};
use super::{Place, Shell};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub mode: Mode,
    pub title: String,
    /// The job doing it, once it started; 0 before.
    pub job: u64,
    /// You chose to go on without it after its job stopped.
    pub skipped: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    /// Squad Code is splitting it into jobs.
    #[default]
    Planning,
    /// The split is ready for you to look over.
    Ready,
    Running,
    /// A job stopped or was thrown away: waiting for you.
    Paused,
    Done,
    /// The split failed; the reason is in `note`.
    Failed,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Goal {
    pub id: u64,
    /// The # thread it was planned in; 0 is the project's #general.
    pub thread: u64,
    pub text: String,
    pub folder: Folder,
    pub steps: Vec<Step>,
    pub state: State,
    pub note: String,
    pub started: u64,
}

impl Goal {
    /// The bots its jobs need that aren't on the crew yet, each once, in the
    /// order the jobs come. Starting the goal adds them.
    pub fn needs(&self) -> Vec<Member> {
        let mut out: Vec<Member> = Vec::new();
        for s in self.steps.iter().filter(|s| !s.skipped && s.job == 0) {
            if let Some(m) = Member::needed_for(s.mode) {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
        }
        out
    }

    /// The step it is on: the first one not yet shipped or finished.
    pub fn current(&self, jobs: &[Job]) -> Option<usize> {
        self.steps.iter().position(|s| {
            !s.skipped
                && (s.job == 0
                    || !jobs
                        .iter()
                        .find(|j| j.id == s.job)
                        .is_some_and(step_finished))
        })
    }
}

/// A step's job is behind it: shipped, or finished when it changes nothing.
pub fn step_finished(j: &Job) -> bool {
    match j.status {
        Status::Shipped => true,
        Status::Done => j.worktree.is_empty(),
        _ => false,
    }
}

/// A step's job stopped in a way that needs you before the goal goes on.
pub fn step_stuck(j: &Job) -> bool {
    matches!(
        j.status,
        Status::Failed | Status::Stopped | Status::Interrupted | Status::Discarded
    )
}

/// The words Squad Code's model is given to split a goal.
pub fn split_prompt(goal: &str, project: &str) -> String {
    format!(
        "Split this goal for the project \"{project}\" (the current folder) into 2 to 6 jobs a coding agent can each finish in one sitting. Read the code first so the jobs fit it. Order them so each builds on the ones before.\n\nReply with only one line per job, nothing else, in the form\nKIND: what to do, in one sentence\nwhere KIND is one of build, fix, test, tidy or review.\n\nThe goal: {goal}"
    )
}

/// The jobs in the model's reply. Lines that don't fit are skipped.
pub fn parse_split(reply: &str) -> Vec<Step> {
    reply
        .lines()
        .filter_map(|l| {
            let l = l
                .trim()
                .trim_start_matches(|c: char| {
                    c.is_ascii_digit() || matches!(c, '.' | ')' | '-' | '*' | ' ')
                })
                .trim_matches('`')
                .replace("**", "");
            let (kind, what) = l.split_once(':')?;
            let mode = match kind.trim().to_ascii_lowercase().as_str() {
                "build" => Mode::Build,
                "fix" => Mode::Fix,
                "test" => Mode::Test,
                "tidy" => Mode::Tidy,
                "review" => Mode::Review,
                _ => return None,
            };
            let title = what.trim().trim_end_matches('.').to_string();
            (!title.is_empty()).then_some(Step {
                mode,
                title,
                ..Default::default()
            })
        })
        .take(8)
        .collect()
}

/// Asks Squad Code's model to split a goal, on another thread.
pub fn plan(st: &AppSettings, folder: &Folder, goal: &str) -> Result<Receiver<Event>, String> {
    let i = crate::settings::assistant_provider(st);
    let p = &agent::PROVIDERS[i];
    let configured = st.agent(p.id).binary;
    if !agent::available(p.bin, &configured) {
        return Err(format!(
            "{} isn't installed, so Squad Code can't split the goal. Pick another engine for Squad Code in Engines.",
            p.name
        ));
    }
    let model = crate::settings::assistant_model(st, p.id);
    let command =
        agent::ask_command_on(p.id, &configured, &split_prompt(goal, &folder.name), &model);
    let (tx, rx) = channel();
    agent::oneshot(command, &folder.path, tx);
    Ok(rx)
}

/// What the bot is told about the goal its job is part of.
pub fn context(goal: &Goal, step: usize) -> String {
    format!(
        "This job is part {} of {} of a bigger goal: {}. Do only this part; the others come after.",
        step + 1,
        goal.steps.len(),
        goal.text
    )
}

impl Shell {
    /// Goals still going, as a row of small cards over the jobs.
    pub(super) fn goals_strip(&mut self, cx: &mut Context<Self>) -> Option<(usize, Div)> {
        let open: Vec<Goal> = self
            .crew
            .store
            .goals
            .iter()
            .filter(|g| g.state != State::Done)
            .cloned()
            .collect();
        if open.is_empty() {
            return None;
        }
        let jobs = self.crew.store.jobs.clone();
        let count = open.len();
        let mut row = div().flex().flex_wrap().gap(px(12.));
        for g in open {
            let id = g.id;
            let done = g
                .steps
                .iter()
                .filter(|s| jobs.iter().any(|j| j.id == s.job && step_finished(j)))
                .count();
            let n = g.steps.len().max(1);
            let (word, color) = match g.state {
                State::Planning => ("Splitting", ui::t_blue()),
                State::Ready => ("Ready to start", ui::t_amber()),
                State::Running => ("Running", ui::t_blue()),
                State::Paused => ("Needs you", ui::t_amber()),
                State::Failed => ("Couldn't split", ui::t_red()),
                State::Done => ("Done", ui::t_green()),
            };
            row = row.child(
                ui::card()
                    .id(("goal-card", id as usize))
                    .flex_1()
                    .min_w(px(240.))
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(ui::line_hi())))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::ink()))
                                    .truncate()
                                    .child(g.text.clone()),
                            )
                            .child(ui::status(color, word)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .child(ui::bar(done as f32 / n as f32, ui::BLUE)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(c(ui::muted()))
                                    .child(format!("{done} of {} jobs", g.steps.len())),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Goal(id), cx))),
            );
        }
        Some((count, row))
    }

    pub(super) fn goal_page(&mut self, id: u64, cx: &mut Context<Self>) -> AnyElement {
        let Some(goal) = self.crew.store.goals.iter().find(|g| g.id == id).cloned() else {
            return self.empty_place("This goal is gone", "It was removed.", cx);
        };
        let jobs = self.crew.store.jobs.clone();
        let current = goal.current(&jobs);
        let needs = goal.needs();
        let shipped = goal
            .steps
            .iter()
            .filter(|s| jobs.iter().any(|j| j.id == s.job && step_finished(j)))
            .count();
        let line = match goal.state {
            State::Planning => "Squad Code is reading the code and splitting this into jobs\u{2026}".to_string(),
            State::Ready if !needs.is_empty() => format!(
                "Look over the jobs, then start. Peek: this plan needs {}, who {} on your crew yet. Starting adds {}.",
                super::and_list(&needs.iter().map(|m| m.name()).collect::<Vec<_>>()),
                if needs.len() == 1 { "isn't" } else { "aren't" },
                if needs.len() == 1 { "it" } else { "them" },
            ),
            State::Ready => "Look over the jobs, drop any you don't want, then start. They run one after another, each building on the last.".to_string(),
            State::Running => format!("{shipped} of {} done. The next job starts when the one before it ships.", goal.steps.len()),
            State::Paused => "A job stopped, so the goal is waiting for you.".to_string(),
            State::Done => format!("All {} jobs are done.", goal.steps.len()),
            State::Failed => goal.note.clone(),
        };
        let mut head = div().flex().items_end().gap(px(10.)).child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(ui::caption(format!(
                    "PLAN \u{b7} {}",
                    goal.folder.name.to_uppercase()
                )))
                .child(
                    div()
                        .text_size(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(goal.text.clone()),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(c(ui::muted()))
                        .child(line),
                ),
        );
        match goal.state {
            State::Ready => {
                head = head
                    .child(
                        ui::ghost("goal-drop", "Discard")
                            .on_click(cx.listener(move |this, _, _, cx| this.drop_goal(id, cx))),
                    )
                    .child(
                        ui::solid(
                            "goal-start",
                            match needs.as_slice() {
                                [] => format!("Start {} jobs", goal.steps.len()),
                                [one] => format!("Add {} and start", one.name()),
                                _ => "Add the bots and start".to_string(),
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.start_goal(id, cx))),
                    );
            }
            State::Paused => {
                head = head
                    .child(
                        ui::ghost("goal-stop", "Stop the goal")
                            .on_click(cx.listener(move |this, _, _, cx| this.drop_goal(id, cx))),
                    )
                    .child(
                        ui::solid("goal-next", "Go on to the next job")
                            .on_click(cx.listener(move |this, _, _, cx| this.skip_step(id, cx))),
                    );
            }
            State::Failed => {
                head = head.child(
                    ui::ghost("goal-drop", "Discard")
                        .on_click(cx.listener(move |this, _, _, cx| this.drop_goal(id, cx))),
                );
            }
            _ => {}
        }

        let mut list = div()
            .w_full()
            .bg(c(ui::panel()))
            .border_1()
            .border_color(c(ui::line()))
            .rounded(px(ui::CARD_R))
            .overflow_hidden();
        if goal.state == State::Planning {
            list = list.child(div().p(px(18.)).child(ui::sweep(ui::RUNO, 0.)));
        }
        for (k, s) in goal.steps.iter().enumerate() {
            let who = Member::doer_for(s.mode).unwrap_or(Member::for_mode(s.mode));
            let face = self.avatar(who, 24.);
            let job = jobs.iter().find(|j| j.id == s.job);
            let pill = match job {
                Some(j) => {
                    let (w, t) = home::status_of(j);
                    home::pill(&w, t)
                }
                None if goal.state == State::Running && current == Some(k) => {
                    home::pill("Starting", ui::run_tone())
                }
                None => home::pill("Waiting", ui::grey_tone()),
            };
            let job_id = s.job;
            list = list.child(
                div()
                    .id(("goal-step", k))
                    .h(px(54.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .border_b_1()
                    .border_color(c(ui::line2()))
                    .when(current == Some(k) && goal.state != State::Ready, |d| {
                        d.bg(c(ui::raised()))
                    })
                    .child(
                        div()
                            .w(px(18.))
                            .font_family(ui::mono())
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child(format!("{}", k + 1)),
                    )
                    .child(face)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .text_color(c(ui::ink()))
                                    .truncate()
                                    .child(s.title.clone()),
                            )
                            .child({
                                let ready = goal.state == State::Ready;
                                div()
                                    .id(("goal-kind", k))
                                    .text_size(px(11.5))
                                    .text_color(c(ui::faint()))
                                    .child(format!(
                                        "{} \u{b7} {}{}",
                                        s.mode.label(),
                                        who.name(),
                                        if ready { "  \u{2192} change" } else { "" }
                                    ))
                                    // Before it starts, a click moves the step to
                                    // the next kind of job, and so the next bot.
                                    .when(ready, |d| {
                                        d.cursor_pointer()
                                            .hover(|s| s.text_color(c(ui::ink())))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(st) = this
                                                    .crew
                                                    .store
                                                    .goals
                                                    .iter_mut()
                                                    .find(|g| g.id == id)
                                                    .and_then(|g| g.steps.get_mut(k))
                                                {
                                                    let all = super::data::Mode::ALL;
                                                    let at = all
                                                        .iter()
                                                        .position(|m| *m == st.mode)
                                                        .unwrap_or(0);
                                                    st.mode = all[(at + 1) % all.len()];
                                                }
                                                this.crew.store.save();
                                                cx.notify();
                                            }))
                                    })
                            }),
                    )
                    .child(pill)
                    .when(job_id != 0, |d| {
                        d.child(ui::ghost(("goal-open", k), "Open").h(px(28.)).on_click(
                            cx.listener(move |this, _, _, cx| this.go(Place::Live(job_id), cx)),
                        ))
                    })
                    .when(goal.state == State::Ready && goal.steps.len() > 1, |d| {
                        d.child(
                            div()
                                .id(("goal-up", k))
                                // The first keeps the room, so the rows line up.
                                .when(k == 0, |d| d.opacity(0.))
                                .px(px(8.))
                                .text_size(px(12.))
                                .text_color(c(ui::faint()))
                                .cursor_pointer()
                                .hover(|s| s.text_color(c(ui::ink())))
                                .child("Move up")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(g) =
                                        this.crew.store.goals.iter_mut().find(|g| g.id == id)
                                    {
                                        if k > 0 && k < g.steps.len() {
                                            g.steps.swap(k - 1, k);
                                        }
                                    }
                                    this.crew.store.save();
                                    cx.notify();
                                })),
                        )
                    })
                    .when(goal.state == State::Ready && goal.steps.len() > 1, |d| {
                        d.child(
                            div()
                                .id(("goal-remove", k))
                                .px(px(8.))
                                .text_size(px(12.))
                                .text_color(c(ui::faint()))
                                .cursor_pointer()
                                .hover(|s| s.text_color(c(ui::t_red())))
                                .child("Remove")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(g) =
                                        this.crew.store.goals.iter_mut().find(|g| g.id == id)
                                    {
                                        if k < g.steps.len() {
                                            g.steps.remove(k);
                                        }
                                    }
                                    this.crew.store.save();
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }
        div()
            .id("goal")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(960.))
                    .px(px(32.))
                    .py(px(28.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(head)
                    .child(list),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::super::data::Mode;
    use super::super::store::{Job, Status};
    use super::{parse_split, step_finished, Goal, Step};

    #[test]
    fn the_split_reads_kind_and_job() {
        let reply = "Here's the plan:\n1. build: Add a gift card table.\n2. **Fix**: Stop the total going negative\n- test: Cover gift cards end to end\nnonsense line\ndeploy: ship it";
        let steps = parse_split(reply);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].mode, Mode::Build);
        assert_eq!(steps[0].title, "Add a gift card table");
        assert_eq!(steps[1].mode, Mode::Fix);
        assert_eq!(steps[2].mode, Mode::Test);
    }

    #[test]
    fn the_goal_moves_on_once_a_step_ships() {
        let goal = Goal {
            steps: vec![
                Step {
                    mode: Mode::Build,
                    title: "a".into(),
                    job: 1,
                    skipped: false,
                },
                Step {
                    mode: Mode::Test,
                    title: "b".into(),
                    job: 0,
                    skipped: false,
                },
            ],
            ..Default::default()
        };
        let mut j = Job {
            id: 1,
            status: Status::Review,
            ..Default::default()
        };
        assert_eq!(goal.current(&[j.clone()]), Some(0));
        j.status = Status::Shipped;
        assert!(step_finished(&j));
        assert_eq!(goal.current(&[j]), Some(1));
    }
}
