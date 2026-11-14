//! A crew that learns. When you send a bot's work back, or leave comments on
//! its diff, Squad asks the crew's engine whether there's a lasting lesson in
//! what you said ("use the date helpers in src/lib/date.ts, never raw Date"),
//! and keeps it for that bot in that project. Every later job the bot starts
//! there is told its lessons, so the same note doesn't have to be given twice.

use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use super::data::Member;
use super::store::Job;

/// Most lessons kept per bot per project; the oldest go first.
pub const KEEP: usize = 20;

/// One thing a bot learned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lesson {
    pub bot: Member,
    /// The project's folder path.
    pub folder: String,
    pub text: String,
    /// The job it came from.
    pub job: u64,
    pub at: u64,
    /// Every bot on the crew follows it in this project, not just `bot`.
    pub crew: bool,
}

impl Default for Lesson {
    fn default() -> Self {
        Lesson {
            bot: Member::Builder,
            folder: String::new(),
            text: String::new(),
            job: 0,
            at: 0,
            crew: false,
        }
    }
}

static LESSONS: RwLock<Vec<Lesson>> = RwLock::new(Vec::new());

/// Makes `lessons` the ones jobs are told (on load, and after a change).
pub fn set_all(lessons: &[Lesson]) {
    *LESSONS.write().unwrap_or_else(|e| e.into_inner()) = lessons.to_vec();
}

/// What `who` has learned in `folder`, as words for its brief; empty when
/// nothing.
pub fn brief(who: Member, folder: &str) -> String {
    let all = LESSONS.read().unwrap_or_else(|e| e.into_inner());
    let mine: Vec<String> = all
        .iter()
        .filter(|l| (l.bot == who || l.crew) && l.folder == folder)
        .map(|l| format!("- {}", l.text.trim()))
        .collect();
    if mine.is_empty() {
        return String::new();
    }
    format!(
        "What you learned from the person's reviews in this project. Follow these:\n{}",
        mine.join("\n")
    )
}

/// Adds a lesson, unless the bot already knows it, keeping at most [`KEEP`]
/// for that bot and project. True when it was added.
pub fn add(lessons: &mut Vec<Lesson>, new: Lesson) -> bool {
    let key = |t: &str| {
        t.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let k = key(&new.text);
    if k.len() < 8
        || lessons
            .iter()
            .any(|l| l.bot == new.bot && l.folder == new.folder && key(&l.text) == k)
    {
        return false;
    }
    lessons.push(new.clone());
    let same: Vec<usize> = lessons
        .iter()
        .enumerate()
        .filter(|(_, l)| l.bot == new.bot && l.folder == new.folder)
        .map(|(i, _)| i)
        .collect();
    if same.len() > KEEP {
        lessons.remove(same[0]);
    }
    true
}

/// What the engine is asked about a send-back.
pub fn prompt(job: &Job, said: &str) -> String {
    format!(
        "You help a coding bot called {bot} learn from code review. The person asked {bot} for this:\n\n\
         {title}\n\n\
         and then sent the work back with:\n\n\
         {said}\n\n\
         Is there a lasting rule in what they said, one that should apply to {bot}'s future work in this \
         project (a convention, a tool or helper to use, something to always or never do)? A request about \
         this one change only (rename this, fix that bug, move the button) is not a rule.\n\n\
         Answer with the rule as one short imperative sentence of at most 25 words, naming the files, \
         helpers or commands it's about. If there is no lasting rule, answer exactly NONE. Answer with \
         nothing else. Don't read or change any files.",
        bot = job.bot.name(),
        title = job.label(),
        said = said.trim(),
    )
}

/// The rule in the engine's answer, if it gave one.
pub fn parse(reply: &str) -> Option<String> {
    let line = reply
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .trim_matches(|c: char| c == '"' || c == '`' || c == '*')
        .trim_start_matches("- ")
        .trim();
    let upper = line.to_uppercase();
    if upper.starts_with("NONE") || upper.starts_with("NO LASTING") || upper.starts_with("NO RULE")
    {
        return None;
    }
    let words = line.split_whitespace().count();
    (3..=40).contains(&words).then(|| line.to_string())
}

/// Whether what the person said is worth asking about: a few words at least.
pub fn worth_asking(said: &str) -> bool {
    said.split_whitespace().count() >= 4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(text: &str) -> Lesson {
        Lesson {
            bot: Member::Builder,
            folder: "/p".into(),
            text: text.into(),
            ..Lesson::default()
        }
    }

    #[test]
    fn rules_are_read_and_nones_dropped() {
        assert_eq!(
            parse("Use the helpers in src/lib/date.ts instead of raw Date objects.\n"),
            Some("Use the helpers in src/lib/date.ts instead of raw Date objects.".into())
        );
        assert_eq!(parse("NONE"), None);
        assert_eq!(parse("none."), None);
        assert_eq!(
            parse("\"Always run pnpm lint before finishing.\""),
            Some("Always run pnpm lint before finishing.".into())
        );
        assert_eq!(parse("Yes"), None);
    }

    #[test]
    fn lessons_are_kept_once_and_capped() {
        let mut all = Vec::new();
        assert!(add(&mut all, lesson("Use pnpm, never npm.")));
        assert!(!add(&mut all, lesson("use pnpm never npm")));
        for i in 0..KEEP + 3 {
            add(
                &mut all,
                lesson(&format!("Rule number {i} for the project")),
            );
        }
        assert_eq!(all.len(), KEEP);
        assert!(!all.iter().any(|l| l.text.contains("pnpm")));
    }

    #[test]
    fn the_brief_is_per_bot_and_project() {
        set_all(&[
            lesson("Use pnpm, never npm."),
            Lesson {
                folder: "/other".into(),
                ..lesson("Tabs, not spaces, always.")
            },
        ]);
        let b = brief(Member::Builder, "/p");
        assert!(b.contains("Use pnpm") && !b.contains("Tabs"));
        assert!(brief(Member::Fixer, "/p").is_empty());
        set_all(&[]);
    }
}
