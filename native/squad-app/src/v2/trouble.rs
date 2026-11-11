//! When a job doesn't end well: what went wrong, in plain words, and the one
//! or two things that get it going again. An engine out of its plan or not
//! installed offers another engine; a stopped job can carry on; a branch that
//! clashes with its base can be handed back to the bot to bring up to date.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::store::{Item, Job, Status};
use super::ui::{self, c};
use super::Pane;
use super::{Place, Shell};

/// What went wrong with a job.
#[derive(Clone, Debug, PartialEq)]
pub enum Trouble {
    /// The engine's plan ran out; when it resets, if it said.
    Limit(String),
    /// The engine isn't installed here.
    Missing,
    /// The engine isn't signed in.
    SignIn,
    /// Its base moved and these files now clash.
    Conflict(Vec<String>),
    Stopped,
    Interrupted,
    /// It stopped with a problem, in the engine's words.
    Failed(String),
}

/// The last error the job reported, else the bot's last words.
fn last_problem(job: &Job) -> String {
    job.feed
        .iter()
        .rev()
        .find_map(|i| match i {
            Item::Error(e) => Some(e.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// What went wrong, or `None` when nothing did.
pub fn trouble(job: &Job) -> Option<Trouble> {
    if !job.conflict.is_empty() && job.status == Status::Review {
        return Some(Trouble::Conflict(job.conflict.clone()));
    }
    let err = last_problem(job);
    let said = job.last_words().unwrap_or("");
    // A plan that ran out can end a run that otherwise looks fine.
    if matches!(job.status, Status::Failed | Status::Done) {
        for text in [err.as_str(), said] {
            if let Some(limit) = crate::planlimit::from_text(text, chrono::Local::now()) {
                let when = limit
                    .resets_at
                    .map(|at| crate::planlimit::reset_clock(at, super::store::now() as i64))
                    .unwrap_or_default();
                return Some(Trouble::Limit(when));
            }
        }
    }
    match job.status {
        Status::Stopped => Some(Trouble::Stopped),
        Status::Interrupted => Some(Trouble::Interrupted),
        Status::Failed => {
            let lower = err.to_lowercase();
            Some(if lower.contains("isn't installed") {
                Trouble::Missing
            } else if lower.contains("sign in")
                || lower.contains("/login")
                || lower.contains(" login")
                || lower.contains("api key")
            {
                Trouble::SignIn
            } else if lower.contains("rate limit")
                || lower.contains("quota")
                || lower.contains("429")
                || lower.contains("credit balance")
            {
                Trouble::Limit(String::new())
            } else {
                Trouble::Failed(err)
            })
        }
        _ => None,
    }
}

/// The files a local merge stopped on, from git's message.
pub fn conflict_files(err: &str) -> Vec<String> {
    err.split_once("conflicts in ")
        .map(|(_, rest)| rest.split(". ").next().unwrap_or(rest))
        .map(|list| {
            list.trim_end_matches('.')
                .split(", ")
                .map(|f| f.trim().to_string())
                .filter(|f| !f.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// What the bot is told when its branch clashes with its base.
pub fn resolve_prompt(job: &Job) -> String {
    format!(
        "{base} moved on since you started, and these files now clash with it: {files}. Bring this branch up to date: fetch if the project has a remote, merge the latest {base} into this branch, resolve each conflict so both changes still work, run the project's checks, and commit.",
        base = if job.base.is_empty() { "The base branch" } else { &job.base },
        files = job.conflict.join(", ")
    )
}

impl Shell {
    /// The card at the end of a job that didn't end well.
    pub(super) fn trouble_card(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        let t = trouble(job)?;
        let id = job.id;
        let engine = job
            .engine
            .split(" \u{b7} ")
            .next()
            .unwrap_or("The engine")
            .to_string();
        let engine = if engine.is_empty() {
            "The engine".to_string()
        } else {
            engine
        };
        let other = {
            let st = cx.global::<crate::settings::AppSettings>();
            super::engine::fallback(st, job)
        };
        let other_name = other.map(|i| crate::agent::PROVIDERS[i].name);
        let (tone, title, line): (u32, String, String) = match &t {
            Trouble::Limit(when) => (
                ui::AMBER,
                format!("{engine} hit its usage limit"),
                if when.is_empty() {
                    "Run it on another engine now, or try again once the plan resets.".into()
                } else {
                    format!("It resets at {when}. Run it on another engine now, or try again then.")
                },
            ),
            Trouble::Missing => (
                ui::AMBER,
                format!("{engine} isn't installed on this computer"),
                "Install it, or run this job on an engine you have.".into(),
            ),
            Trouble::SignIn => (
                ui::AMBER,
                format!("{engine} isn't signed in"),
                last_problem(job),
            ),
            Trouble::Conflict(files) => (
                ui::AMBER,
                format!(
                    "{} moved on, and {} {} now clash",
                    if job.base.is_empty() { "The base branch" } else { &job.base },
                    files.len(),
                    if files.len() == 1 { "file" } else { "files" }
                ),
                format!(
                    "{}. {} can bring the branch up to date and resolve them, then it comes back for review.",
                    files.join(", "),
                    job.bot.name()
                ),
            ),
            Trouble::Stopped => (
                ui::muted(),
                "You stopped it".into(),
                if job.worktree.is_empty() {
                    "It can carry on from where it was.".into()
                } else {
                    "What it did so far is still on its branch. It can carry on from there.".into()
                },
            ),
            Trouble::Interrupted => (
                ui::muted(),
                "Squad Code closed while this was running".into(),
                "It can carry on from where it was, or start over.".into(),
            ),
            Trouble::Failed(why) => (
                ui::RED,
                "It stopped with a problem".into(),
                if why.is_empty() {
                    "The engine stopped without saying why.".into()
                } else {
                    why.clone()
                },
            ),
        };
        let setup = if matches!(t, Trouble::SignIn) {
            self.setup_runs(cx)
        } else {
            Vec::new()
        };
        let mut buttons = div().mt(px(4.)).flex().flex_wrap().gap(px(6.));
        let primary = |id: &'static str, label: String| ui::dbtn(id, label, ui::Btn::Primary);
        let plain = |id: &'static str, label: &'static str| ui::dbtn(id, label, ui::Btn::Plain);
        let ghost = |id: &'static str, label: &'static str| ui::dbtn(id, label, ui::Btn::Ghost);
        let switch = other.zip(other_name);
        match &t {
            Trouble::Conflict(_) => {
                buttons = buttons
                    .child(
                        primary(
                            "trouble-resolve",
                            format!("Ask {} to fix it", job.bot.name()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.resolve(id, cx))),
                    )
                    .child(
                        ghost("trouble-discard", "Discard")
                            .on_click(cx.listener(move |this, _, _, cx| this.discard_job(id, cx))),
                    );
            }
            Trouble::Stopped | Trouble::Interrupted => {
                buttons = buttons
                    .child(
                        primary("trouble-carry", "Carry on".into())
                            .on_click(cx.listener(move |this, _, _, cx| this.carry_on(id, cx))),
                    )
                    .child(
                        ghost("trouble-over", "Start over")
                            .on_click(cx.listener(move |this, _, _, cx| this.rerun(id, None, cx))),
                    );
                if !job.worktree.is_empty() {
                    buttons = buttons.child(
                        ghost("trouble-discard", "Discard")
                            .on_click(cx.listener(move |this, _, _, cx| this.discard_job(id, cx))),
                    );
                }
            }
            _ => {
                // Signing in runs inside Squad, its card right here.
                let login = crate::agent::PROVIDERS
                    .iter()
                    .find(|p| p.name == engine)
                    .and_then(|p| crate::agent::sign_in_cmd(p.id));
                let signing = matches!(t, Trouble::SignIn) && login.is_some();
                if let (true, Some(cmd)) = (signing, login) {
                    buttons = buttons.child(
                        primary("trouble-signin", "Sign in".into())
                            .on_click(cx.listener(move |this, _, _, cx| this.run_setup(cmd, cx))),
                    );
                }
                if let Some((i, name)) = switch {
                    let label = format!("Run on {name} instead");
                    let b = if signing {
                        ui::dbtn("trouble-other", label, ui::Btn::Plain)
                    } else {
                        primary("trouble-other", label)
                    };
                    buttons = buttons.child(
                        b.on_click(cx.listener(move |this, _, _, cx| this.rerun(id, Some(i), cx))),
                    );
                }
                match &t {
                    Trouble::Missing => {
                        buttons = buttons.child(
                            plain("trouble-install", "How to install").on_click(cx.listener(
                                |this, _, _, cx| this.go(Place::Settings(Pane::Engines), cx),
                            )),
                        );
                    }
                    Trouble::Failed(_) | Trouble::Limit(_) => {
                        buttons =
                            buttons.child(ghost("trouble-again", "Try again").on_click(
                                cx.listener(move |this, _, _, cx| this.rerun(id, None, cx)),
                            ));
                    }
                    _ => {}
                }
                if switch.is_none() && !signing {
                    buttons = buttons.child(ghost("trouble-engines", "Engines").on_click(
                        cx.listener(|this, _, _, cx| this.go(Place::Settings(Pane::Engines), cx)),
                    ));
                }
            }
        }
        // Amber: fixable, one press away. Grey: paused, nothing broke. Red:
        // it broke, read why.
        let (bar, tint) = match tone {
            ui::AMBER => (ui::t_amber(), 0.07),
            ui::RED => (ui::t_red(), 0.06),
            _ => (ui::faint(), 0.0),
        };
        let face = self.avatar(job.bot, 14.);
        Some(
            div()
                .ml(px(40.))
                .relative()
                .overflow_hidden()
                .pl(px(16.))
                .pr(px(14.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(5.))
                .rounded(px(5.))
                .bg(c(super::theme::mix(ui::raised(), bar, tint)))
                .border_1()
                .border_color(c(if tint > 0. {
                    super::theme::mix(ui::raised(), bar, 0.35)
                } else {
                    ui::line()
                }))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.))
                        .bg(c(bar)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .font_family(ui::MONO)
                        .text_size(px(11.))
                        .text_color(c(ui::faint()))
                        .child(face)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(format!("{} \u{b7} #{id} {}", job.bot.name(), job.label())),
                        ),
                )
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(if tone == ui::RED { ui::t_red() } else { ui::ink() }))
                        .child(title),
                )
                .when(!line.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(18.))
                            .text_color(c(ui::body()))
                            .child(line),
                    )
                })
                .when(other.is_none() && !matches!(t, Trouble::Conflict(_) | Trouble::Stopped | Trouble::Interrupted), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child("No other engine is ready on this computer. Add one in Engines to switch."),
                    )
                })
                .when(matches!(t, Trouble::SignIn), |d| d.children(setup))
                .child(buttons),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::{Item, Job, Status};
    use super::{conflict_files, trouble, Trouble};

    fn failed(err: &str) -> Job {
        Job {
            status: Status::Failed,
            feed: vec![Item::Error(err.into())],
            ..Default::default()
        }
    }

    #[test]
    fn troubles_are_told_apart() {
        assert_eq!(
            trouble(&failed("Claude Code isn't installed on this computer. Pick another engine for Builder in Engines.")),
            Some(Trouble::Missing)
        );
        assert!(matches!(
            trouble(&failed("You've hit your usage limit for Claude Code.")),
            Some(Trouble::Limit(_))
        ));
        assert_eq!(
            trouble(&failed(
                "Codex isn't signed in. Press Sign in and Squad runs codex login for you, right here."
            )),
            Some(Trouble::SignIn)
        );
        assert!(matches!(trouble(&failed("boom")), Some(Trouble::Failed(_))));
        let ok = Job {
            status: Status::Done,
            ..Default::default()
        };
        assert_eq!(trouble(&ok), None);
    }

    #[test]
    fn a_limit_in_the_last_words_counts_too() {
        let j = Job {
            status: Status::Done,
            feed: vec![Item::Text("Claude usage limit reached.".into())],
            ..Default::default()
        };
        assert!(matches!(trouble(&j), Some(Trouble::Limit(_))));
    }

    #[test]
    fn conflicts_name_their_files() {
        assert_eq!(
            conflict_files("Merging stopped on conflicts in src/a.rs, src/b.rs. Update the branch from main first so they can be resolved there."),
            vec!["src/a.rs".to_string(), "src/b.rs".to_string()]
        );
        assert!(conflict_files("other error").is_empty());
    }
}
