//! Where Peek sends a job: the bot, the kind of work, and Peek's line as it
//! brings the bot in. The person's own engine makes the pick (see
//! [`super::think`]); nothing here guesses from keywords.

use super::super::data::{Member, Mode};

/// Where a sentence goes, how sure the guess is, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub mode: Mode,
    pub bot: Member,
    /// 0 to 1: how sure the engine was.
    pub sure: f32,
    /// "\u{201c}crash\u{201d} and \u{201c}error\u{201d} sound like a bug", for a tooltip.
    pub why: String,
    /// Where the answer came from: "engine", or who picked.
    pub by: String,
    /// Where the person asked for it to run: "server", "here", or "" for
    /// their usual choice.
    pub site: String,
}

impl Route {
    /// Why this bot, in a few words: "Sounds like a bug to hunt down."
    pub fn reason(&self) -> String {
        if self.bot.is_made() {
            return format!("This one's right up {}'s street.", self.bot.name());
        }
        match self.mode {
            Mode::Build => "This is a build job.",
            Mode::Fix => "Sounds like a bug to hunt down.",
            Mode::Test => "Let's try it for real.",
            Mode::Review => "This wants a careful second look.",
            Mode::Tidy => "Cleanup time.",
            Mode::Ask => "Good question, and the answer is in the code.",
        }
        .to_string()
    }

    /// Peek's line as it hands the job on: why, then whether the bot is
    /// new to the project, invited in, or already here.
    pub fn handoff(&self, joins: Joins) -> String {
        format!("{} {}", self.reason(), joins.line(self.bot))
    }
}

/// Where a bot stands when Peek brings it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joins {
    /// It has never worked on this project: Peek onboards it.
    New,
    /// It has worked on this project, not in this thread: Peek invites it.
    Back,
    /// It's already working in this thread.
    Here,
}

impl Joins {
    /// Where `bot` stands, from the bots that have worked on this project
    /// and those in this thread.
    pub fn of(bot: Member, project: &[Member], thread: &[Member]) -> Joins {
        if thread.contains(&bot) {
            Joins::Here
        } else if project.contains(&bot) {
            Joins::Back
        } else {
            Joins::New
        }
    }

    /// Peek's words for it.
    pub fn line(self, bot: Member) -> String {
        let name = bot.name();
        match self {
            Joins::New => format!(
                "Onboarding {name}: it's new to this project, so it'll look around the code first."
            ),
            Joins::Back => {
                format!("Inviting {name} into this thread; it's worked on this project before.")
            }
            Joins::Here => format!("{name} is already in this thread, so it's picking this up."),
        }
    }

    /// How the engine is told where a bot stands.
    pub fn note(self) -> &'static str {
        match self {
            Joins::New => "never worked on this project",
            Joins::Back => "has worked on this project, not in this thread",
            Joins::Here => "already in this thread",
        }
    }
}

/// Words too common to tie a sentence to a made bot: glue words, and the
/// tools and kinds of work nearly every job names.
pub const COMMON: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "or",
    "for",
    "to",
    "of",
    "in",
    "on",
    "it",
    "is",
    "my",
    "our",
    "your",
    "this",
    "that",
    "with",
    "bot",
    "job",
    "jobs",
    "code",
    "all",
    "any",
    "app",
    "api",
    "npm",
    "javascript",
    "typescript",
    "node",
    "python",
    "rust",
    "project",
    "file",
    "files",
    "repo",
    "test",
    "tests",
    "build",
    "builds",
    "fix",
    "fixes",
    "bug",
    "bugs",
    "run",
    "runs",
    "add",
    "make",
    "update",
    "change",
    "changes",
    "version",
    "bump",
    "new",
    "feature",
    "error",
    "errors",
];

/// The bot on `crew` that takes jobs of this kind: the built-in one if it's
/// there, else a made bot that does that work, else the built-in one anyway.
pub fn doer(mode: Mode, crew: &[Member]) -> Member {
    let own = Member::for_mode(mode);
    if crew.contains(&own) {
        return own;
    }
    crew.iter()
        .copied()
        .find(|m| *m != Member::Shipper && m.mode() == mode)
        .unwrap_or(own)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handoff_names_the_bot_and_why() {
        let r = Route {
            mode: Mode::Fix,
            bot: Member::Fixer,
            sure: 0.9,
            why: String::new(),
            by: "engine".into(),
            site: String::new(),
        };
        let h = r.handoff(Joins::New);
        assert!(h.contains("Fixer") && h.contains("bug"), "{h}");
        assert!(h.contains("Onboarding"), "{h}");
        assert!(r.handoff(Joins::Back).contains("Inviting"));
        assert!(r.handoff(Joins::Here).contains("already in this thread"));
    }

    #[test]
    fn where_a_bot_stands() {
        let (b, f) = (Member::Builder, Member::Fixer);
        assert_eq!(Joins::of(b, &[], &[]), Joins::New);
        assert_eq!(Joins::of(b, &[b], &[]), Joins::Back);
        assert_eq!(Joins::of(b, &[b, f], &[b]), Joins::Here);
    }

    #[test]
    fn a_kind_of_work_goes_to_its_bot() {
        assert_eq!(doer(Mode::Fix, &Member::CREW), Member::Fixer);
    }
}
