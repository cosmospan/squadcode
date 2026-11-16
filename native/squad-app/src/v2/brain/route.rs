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
    /// The whole ask as Peek understood it from the conversation, written
    /// out so the bot can do it alone: a reply like "all together" only
    /// makes sense with the question it answers. Empty when Peek didn't
    /// write one; the person's own words go to the bot then.
    pub task: String,
    /// A short title for the job's card and branch, from the whole ask and
    /// spelled right; empty to keep the person's words.
    pub title: String,
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

    /// Peek's line as it hands the job on: why this bot.
    pub fn handoff(&self) -> String {
        self.reason()
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
    fn the_handoff_says_why() {
        let r = Route {
            mode: Mode::Fix,
            bot: Member::Fixer,
            sure: 0.9,
            why: String::new(),
            by: "engine".into(),
            site: String::new(),
            task: String::new(),
            title: String::new(),
        };
        assert!(r.handoff().contains("bug"), "{}", r.handoff());
    }

    #[test]
    fn a_kind_of_work_goes_to_its_bot() {
        assert_eq!(doer(Mode::Fix, &Member::CREW), Member::Fixer);
    }
}
