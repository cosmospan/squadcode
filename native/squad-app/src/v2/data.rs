//! The crew: who each bot is, what it is for, and how it looks.

use std::sync::RwLock;

use squad_holo::fin::{Fin, Outfit};
use squad_holo::Tint;

use super::ui;

/// A bot the person made in the maker on Crew: its name, what it's for, the
/// built-in bot whose kind of work it does, and how its Fin looks.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Made {
    pub id: u16,
    pub name: String,
    pub role: String,
    pub like: Member,
    /// `look[0]` is its colour, an index into [`super::mascot::COLOURS`]. The
    /// rest is left from the old bubble maker and no longer read.
    pub look: [u8; 4],
    /// Its Fin outfit, as [`Fin::outfit_code`] has it; bare when missing.
    #[serde(default)]
    pub outfit: String,
    /// What it knows best, as words a job may use ("stripe", "billing"): a job
    /// that uses them goes to it.
    #[serde(default)]
    pub skills: Vec<String>,
    /// How it works, told to its engine on every job ("Run vitest before you
    /// stop").
    #[serde(default)]
    pub rules: Vec<String>,
    /// The project folder it's only for; empty for a bot on every project's crew.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project: String,
}

/// Made bots with their words kept for the life of the app, so `Member` stays a
/// small `Copy` value whose names are `&'static str` like the built-in crew's.
struct Kept {
    id: u16,
    name: &'static str,
    role: &'static str,
    persona: &'static str,
    skills: &'static str,
    like: Member,
    tint: Tint,
    outfit: Outfit,
    project: String,
}

static MADE: RwLock<Vec<Kept>> = RwLock::new(Vec::new());

/// The project open now, by folder: bots made for another project stay off the
/// crew here. Empty (no window, or no project open) shows every bot.
static HERE: RwLock<String> = RwLock::new(String::new());

/// Sets the project open now (see [`HERE`]).
pub fn set_here(folder: &str) {
    let mut here = HERE.write().unwrap_or_else(|e| e.into_inner());
    if *here != folder {
        *here = folder.to_string();
    }
}

/// The project a made bot is only for; empty for every project.
pub fn project_of(who: Member) -> String {
    match who {
        Member::Made(id) => made(id, |k| k.project.clone()).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Built-in bots the person removed from their crew (or didn't add).
static REMOVED: RwLock<Vec<Member>> = RwLock::new(Vec::new());

/// Sets which built-in bots are not on the crew.
pub fn set_removed(removed: &[Member]) {
    *REMOVED.write().unwrap_or_else(|e| e.into_inner()) = removed.to_vec();
}

/// The bots the walkthrough's Crew step has ticked to start with, besides
/// Peek: one to build, one to fix and one to keep the rules. Peek leads every
/// crew and offers a bot the crew is missing when a job needs one (see
/// [`Member::needed_for`]); more can be added or made on Crew any time.
pub const STARTER_CREW: [Member; 3] = [Member::Builder, Member::Fixer, Member::Guard];

/// Makes `made` the bots the app knows about (on load, and after a change).
pub fn set_made(made: &[Made]) {
    let mut kept = MADE.write().unwrap_or_else(|e| e.into_inner());
    // Words are leaked once per distinct text; a few made bots, rarely renamed.
    let keep = |old: &[Kept], text: &str, f: fn(&Kept) -> &'static str| -> &'static str {
        old.iter()
            .map(f)
            .find(|t| *t == text)
            .unwrap_or_else(|| Box::leak(text.to_string().into_boxed_str()))
    };
    let next: Vec<Kept> = made
        .iter()
        .map(|m| {
            let mut persona = format!(
                "You are {}, a bot the person made for Squad Code's crew. Your job: {}.",
                m.name,
                m.role.trim_end_matches('.'),
            );
            if !m.skills.is_empty() {
                persona.push_str(&format!(" You know {} best.", m.skills.join(", ")));
            }
            if !m.rules.is_empty() {
                persona.push_str(" Your rules:");
                for r in &m.rules {
                    persona.push_str(&format!("\n- {}", r.trim()));
                }
                persona.push('\n');
            } else {
                persona.push(' ');
            }
            persona.push_str(m.like.persona());
            let skills = m.skills.join(" ");
            Kept {
                id: m.id,
                name: keep(&kept, &m.name, |k| k.name),
                role: keep(&kept, &m.role, |k| k.role),
                persona: keep(&kept, &persona, |k| k.persona),
                skills: keep(&kept, &skills, |k| k.skills),
                like: m.like,
                tint: super::mascot::colour_at(m.look[0]),
                outfit: Fin::outfit_from_code(&m.outfit),
                project: m.project.clone(),
            }
        })
        .collect();
    *kept = next;
}

fn made<T>(id: u16, f: impl FnOnce(&Kept) -> T) -> Option<T> {
    let kept = MADE.read().unwrap_or_else(|e| e.into_inner());
    kept.iter().find(|k| k.id == id).map(f)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Member {
    /// Leads the crew in every project: answers questions, hands jobs to the
    /// right bot and offers one the crew is missing. Always on the crew; it
    /// can't be removed, and a made bot can't be another Fin.
    Fin,
    Builder,
    Checker,
    Guard,
    Fixer,
    Explainer,
    Keeper,
    /// Owns git: opens pull requests, watches their checks, merges and cleans up.
    Shipper,
    /// A bot the person made, by its id.
    Made(u16),
}

impl Member {
    pub const CREW: [Member; 7] = [
        Member::Builder,
        Member::Checker,
        Member::Guard,
        Member::Fixer,
        Member::Explainer,
        Member::Keeper,
        Member::Shipper,
    ];

    /// The built-in bots whose kind of work a made bot can do.
    pub const KINDS: [Member; 6] = [
        Member::Builder,
        Member::Checker,
        Member::Guard,
        Member::Fixer,
        Member::Explainer,
        Member::Keeper,
    ];

    /// The person's crew: Fin, the built-in bots they kept, then the ones they
    /// made.
    pub fn all() -> Vec<Member> {
        let removed = REMOVED.read().unwrap_or_else(|e| e.into_inner());
        let here = HERE.read().unwrap_or_else(|e| e.into_inner()).clone();
        Member::everyone()
            .into_iter()
            .filter(|m| !removed.contains(m))
            .filter(|m| {
                let only = project_of(*m);
                here.is_empty() || only.is_empty() || only == here
            })
            .collect()
    }

    /// Every bot, kept in the crew or not.
    pub fn everyone() -> Vec<Member> {
        let kept = MADE.read().unwrap_or_else(|e| e.into_inner());
        std::iter::once(Member::Fin)
            .chain(Member::CREW)
            .chain(kept.iter().map(|k| Member::Made(k.id)))
            .collect()
    }

    /// Whether this bot is in the person's crew.
    pub fn on_crew(self) -> bool {
        if self == Member::Fin {
            return true;
        }
        !REMOVED
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&self)
            && (!self.is_made() || made(self.made_id(), |_| ()).is_some())
    }

    fn made_id(self) -> u16 {
        match self {
            Member::Made(id) => id,
            _ => 0,
        }
    }

    /// The crew member who takes jobs of this kind: the built-in one if it's on the
    /// crew, else a made bot that does that kind of work, else Fin for a
    /// question.
    pub fn doer_for(mode: Mode) -> Option<Member> {
        let all = Member::all();
        let own = Member::for_mode(mode);
        if all.contains(&own) {
            return Some(own);
        }
        // Shipper takes no jobs of its own: it ships what the others made.
        all.into_iter()
            .find(|m| !matches!(m, Member::Shipper | Member::Fin) && m.mode() == mode)
            .or((mode == Mode::Ask).then_some(Member::Fin))
    }

    /// The bot Fin offers to add when a job of this kind comes in and nobody on
    /// the crew does that work; `None` when someone already does.
    pub fn needed_for(mode: Mode) -> Option<Member> {
        match Member::doer_for(mode) {
            Some(_) => None,
            None => Some(Member::for_mode(mode)),
        }
    }

    /// The built-in bot whose kind of work this one does (itself, for the six).
    pub fn like(self) -> Member {
        match self {
            Member::Made(id) => made(id, |k| k.like).unwrap_or(Member::Explainer),
            m => m,
        }
    }

    pub fn is_made(self) -> bool {
        matches!(self, Member::Made(_))
    }

    pub fn name(self) -> &'static str {
        match self {
            Member::Made(id) => made(id, |k| k.name).unwrap_or("Bot"),
            Member::Fin => "Peek",
            Member::Builder => "Builder",
            Member::Checker => "Checker",
            Member::Guard => "Guard",
            Member::Fixer => "Fixer",
            Member::Explainer => "Explainer",
            Member::Keeper => "Keeper",
            Member::Shipper => "Shipper",
        }
    }

    pub fn role(self) -> &'static str {
        match self {
            Member::Made(id) => made(id, |k| k.role).unwrap_or("Your bot"),
            Member::Fin => "Leads your crew",
            Member::Builder => "Writes the code",
            Member::Checker => "Tries it for real",
            Member::Guard => "Keeps your rules",
            Member::Fixer => "Hunts the bug",
            Member::Explainer => "Plain words",
            Member::Keeper => "Tidies up",
            Member::Shipper => "Opens and merges PRs",
        }
    }

    /// One line on what to hand this bot, for empty chats and the crew cards.
    pub fn pitch(self) -> &'static str {
        match self {
            Member::Made(_) => self.role(),
            Member::Fin => {
                "Ask it anything about the code. It hands jobs to the right bot and offers one your crew is missing."
            }
            Member::Builder => {
                "Give it a feature in a sentence. It plans, writes the code and the tests."
            }
            Member::Checker => {
                "Ask it to run the tests or try a flow, and it tells you what broke."
            }
            Member::Guard => {
                "It reviews changes for risks to login, payments and data before they land."
            }
            Member::Fixer => "Paste an error or describe a bug. It finds the cause and fixes it.",
            Member::Explainer => {
                "Ask anything about the code. It reads it and answers in plain words."
            }
            Member::Keeper => "Dependencies, flaky tests, dead code: the chores nobody gets to.",
            Member::Shipper => {
                "When you approve a job, it opens the pull request, watches the checks and merges."
            }
        }
    }

    /// Things to try in an empty chat.
    pub fn starters(self) -> [&'static str; 3] {
        match self {
            Member::Made(_) => self.like().starters(),
            Member::Fin => [
                "What does this project do, in plain words?",
                "Which bots would help most here?",
                "What should we work on first?",
            ],
            Member::Builder => [
                "How would you add a settings page here?",
                "What's the smallest change to add dark mode?",
                "Where would a new API endpoint go?",
            ],
            Member::Checker => [
                "How do I run the tests in this project?",
                "Which parts have no tests at all?",
                "What should I check before releasing?",
            ],
            Member::Guard => [
                "Is anything risky in my uncommitted changes?",
                "Where does this project handle passwords?",
                "Are any secrets checked into the code?",
            ],
            Member::Fixer => [
                "What's the most likely crash in this code?",
                "Are there any obvious bugs in the main flow?",
                "Why might this be slow?",
            ],
            Member::Explainer => [
                "What does this project do, in plain words?",
                "How is the code organised?",
                "Walk me through what happens on startup.",
            ],
            Member::Keeper => [
                "Which dependencies are out of date?",
                "Is there dead code I can delete?",
                "What would you tidy first?",
            ],
            Member::Shipper => [
                "Which branches are waiting to be merged?",
                "What changed on main today?",
                "Are there old branches I can delete?",
            ],
        }
    }

    /// The colour and outfit a made bot was given in the maker.
    pub fn made_look(self) -> Option<(Tint, Outfit)> {
        match self {
            Member::Made(id) => made(id, |k| (k.tint, k.outfit)),
            _ => None,
        }
    }

    /// The bot's accent, for its name, status and progress.
    pub fn accent(self) -> u32 {
        match self {
            Member::Made(_) => self.made_look().map_or(Tint::VIOLET, |(t, _)| t).strong().0,
            Member::Fin => ui::RUNO,
            Member::Builder => ui::ORANGE,
            Member::Checker => ui::GREEN,
            Member::Guard => ui::AMBER,
            Member::Fixer => ui::RED,
            Member::Explainer => ui::BLUE,
            Member::Keeper => ui::VIOLET,
            Member::Shipper => 0x14B8A6,
        }
    }

    /// The bot a new job in this mode goes to.
    pub fn for_mode(mode: Mode) -> Member {
        match mode {
            Mode::Ask => Member::Explainer,
            Mode::Build => Member::Builder,
            Mode::Fix => Member::Fixer,
            Mode::Review => Member::Guard,
            Mode::Test => Member::Checker,
            Mode::Tidy => Member::Keeper,
        }
    }

    /// What the bot is, told to its engine so the answer sounds like it.
    pub fn persona(self) -> &'static str {
        match self {
            Member::Made(id) => made(id, |k| k.persona).unwrap_or(""),
            Member::Fin => "You are Peek, who leads the crew in Squad Code and is the person's everyday assistant. Talk with them about anything: say hi back, answer questions, research, plan their day, think things through, and explain the files in this folder in plain words when asked. Many people using you don't code, so keep it plain and friendly, and lead with the answer. Don't change files yourself. When what they want is real work on this folder (building, fixing, testing, reviewing or tidying), say which bot should take it and that they can just ask for it in the thread.",
            Member::Builder => "You are Builder, a bot in Squad Code's crew that writes code. Plan briefly, make the change with tests, and keep to the project's style.",
            Member::Checker => "You are Checker, a bot in Squad Code's crew that tries things for real. Run the project's tests and checks; when it's a web app and you have Squad Code's browser tools, start it and click through what changed in the job's browser. Report plainly what passed and what broke.",
            Member::Guard => "You are Guard, a bot in Squad Code's crew that keeps the team's rules. Point out risks to login, payments, data and secrets in plain words.",
            Member::Fixer => "You are Fixer, a bot in Squad Code's crew that hunts bugs. Find the cause with evidence, fix it, and add a test that would have caught it.",
            Member::Explainer => "You are Explainer, a bot in Squad Code's crew. You answer questions about this codebase in plain words for people who may not code. Lead with the answer, then the evidence (files, numbers).",
            Member::Keeper => "You are Keeper, a bot in Squad Code's crew that tidies up: dependencies, flaky tests, dead code. Keep changes small and safe.",
            Member::Shipper => "You are Shipper, the bot in Squad Code's crew that owns git: branches, pull requests, merges and releases. Answer about the repository's branches and history in plain words, and never push or change anything yourself when asked a question.",
        }
    }

    /// A stable key for the frame cache.
    pub fn key(self) -> u64 {
        match self {
            Member::Builder => 1,
            Member::Checker => 2,
            Member::Guard => 3,
            Member::Fixer => 4,
            Member::Explainer => 5,
            Member::Keeper => 6,
            Member::Shipper => 7,
            Member::Fin => 8,
            Member::Made(id) => 100 + id as u64,
        }
    }

    /// The kind of job this bot takes on Today.
    pub fn mode(self) -> Mode {
        match self.like() {
            Member::Builder => Mode::Build,
            Member::Checker => Mode::Test,
            Member::Guard => Mode::Review,
            Member::Fixer => Mode::Fix,
            Member::Keeper => Mode::Tidy,
            _ => Mode::Ask,
        }
    }
}

/// What kind of job the person is handing over.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Mode {
    Ask,
    #[default]
    Build,
    Fix,
    Review,
    Test,
    Tidy,
}

impl Mode {
    pub const ALL: [Mode; 6] = [
        Mode::Ask,
        Mode::Build,
        Mode::Fix,
        Mode::Review,
        Mode::Test,
        Mode::Tidy,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Ask => "Ask",
            Mode::Build => "Build",
            Mode::Fix => "Fix",
            Mode::Review => "Review",
            Mode::Test => "Test",
            Mode::Tidy => "Tidy",
        }
    }

    /// The work itself, for "nobody on your crew … yet".
    pub fn work(self) -> &'static str {
        match self {
            Mode::Ask => "answers questions about the code",
            Mode::Build => "writes new code",
            Mode::Fix => "fixes bugs",
            Mode::Review => "reviews changes",
            Mode::Test => "runs the tests",
            Mode::Tidy => "tidies up",
        }
    }

    /// Whether the job may change files. Ask and Review only read.
    pub fn writes(self) -> bool {
        !matches!(self, Mode::Ask | Mode::Review)
    }

    /// The job box's hint while the kind of work is still read from the text.
    pub const ANY_PLACEHOLDER: &'static str = "Ask Peek anything, or give the crew a job…";

    pub fn placeholder(self) -> &'static str {
        match self {
            Mode::Ask => "Ask anything…",
            Mode::Build => "Describe what to build, in a sentence or two…",
            Mode::Fix => "Paste an error, or describe what's broken…",
            Mode::Review => "What should Guard look at? Leave empty for your uncommitted changes…",
            Mode::Test => "What should Checker run or try? Leave empty for the whole test suite…",
            Mode::Tidy => "What should Keeper tidy? Leave empty and it picks…",
        }
    }

    /// The job when the box is left empty, for the modes that have one.
    /// The kind of work a request reads as, so the job box never asks: a
    /// question is Ask, an error or bug is Fix, and so on; anything else is
    /// Build. The first word counts most, then keywords anywhere in it.
    pub fn guess(text: &str) -> Mode {
        let t = text.trim().to_lowercase();
        if t.is_empty() {
            return Mode::Ask;
        }
        let first = t
            .split(|c: char| !c.is_alphanumeric())
            .find(|w| !w.is_empty())
            .unwrap_or("");
        let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
        match first {
            "fix" | "debug" | "repair" | "resolve" => return Mode::Fix,
            "test" | "run" | "verify" | "try" => return Mode::Test,
            "review" | "audit" | "inspect" => return Mode::Review,
            "tidy" | "refactor" | "clean" | "cleanup" | "rename" | "simplify" | "format"
            | "lint" | "reorganize" | "reorganise" => return Mode::Tidy,
            "add" | "build" | "make" | "create" | "implement" | "write" | "new" => {
                return Mode::Build
            }
            "what" | "why" | "how" | "where" | "which" | "who" | "when" | "explain" | "does"
            | "do" | "is" | "are" | "can" | "should" | "tell" | "describe" | "summarize"
            | "summarise" => return Mode::Ask,
            _ => {}
        }
        if t.ends_with('?') {
            Mode::Ask
        } else if has(&[
            "error",
            "bug",
            "broken",
            "crash",
            "exception",
            "traceback",
            "panic",
            "fails",
            "failing",
            "doesn't work",
            "not working",
            "stack trace",
        ]) {
            Mode::Fix
        } else if has(&["review", "look over", "audit"]) {
            Mode::Review
        } else if has(&["refactor", "clean up", "dead code", "tidy"]) {
            Mode::Tidy
        } else if has(&["run the tests", "test suite", "coverage"]) {
            Mode::Test
        } else if has(&[
            "page",
            "button",
            "screen",
            "form",
            "feature",
            "component",
            "endpoint",
            " api",
            "function",
            "module",
            "database",
            "schema",
            "the app",
            "the site",
            "website",
            "css",
            "script",
            "code",
            "repo",
            "branch",
            "dark mode",
            "login",
            "signup",
            "checkout",
            "cart",
        ]) {
            // Talk about a piece of the product: work for Builder.
            Mode::Build
        } else {
            // Anything else is a conversation: hi, plans, research, questions.
            // Peek answers, and suggests a job when one would help.
            Mode::Ask
        }
    }

    pub fn default_job(self) -> Option<&'static str> {
        match self {
            Mode::Review => Some("Review my uncommitted changes"),
            Mode::Test => Some("Run the test suite and report what fails"),
            Mode::Tidy => Some("Find the most useful small cleanup and do it"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    /// The made bots are one list for the app: tests that set it take turns.
    static CREW_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    use super::*;

    #[test]
    fn the_job_box_reads_the_kind_of_work() {
        assert_eq!(Mode::guess("Add a dark mode toggle"), Mode::Build);
        assert_eq!(Mode::guess("Checkout page shows a blank cart"), Mode::Build);
        assert_eq!(Mode::guess("How does the retry logic work?"), Mode::Ask);
        assert_eq!(Mode::guess("the login form, is it validated?"), Mode::Ask);
        assert_eq!(
            Mode::guess("TypeError: cannot read property 'id' of undefined"),
            Mode::Fix
        );
        assert_eq!(Mode::guess("Fix the coupon field on mobile"), Mode::Fix);
        assert_eq!(Mode::guess("Review my uncommitted changes"), Mode::Review);
        assert_eq!(Mode::guess("Run the tests and report failures"), Mode::Test);
        assert_eq!(Mode::guess("Refactor the payment module"), Mode::Tidy);
        assert_eq!(Mode::guess(""), Mode::Ask);
        // Everyday talk is a conversation, not a job.
        assert_eq!(Mode::guess("hi"), Mode::Ask);
        assert_eq!(Mode::guess("good morning"), Mode::Ask);
        assert_eq!(Mode::guess("plan my week around the gym"), Mode::Ask);
    }

    #[test]
    fn a_bot_for_one_project_stays_there() {
        let _g = CREW_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let bot = |id: u16, project: &str| Made {
            id,
            name: format!("Bot{id}"),
            role: "Helps".into(),
            like: Member::Builder,
            look: [0; 4],
            outfit: String::new(),
            skills: Vec::new(),
            rules: Vec::new(),
            project: project.into(),
        };
        set_made(&[bot(7, ""), bot(8, "/work/shop")]);
        set_here("/work/blog");
        assert!(Member::all().contains(&Member::Made(7)));
        assert!(!Member::all().contains(&Member::Made(8)));
        set_here("/work/shop");
        assert!(Member::all().contains(&Member::Made(8)));
        assert_eq!(project_of(Member::Made(8)), "/work/shop");
        // No project open: every bot.
        set_here("");
        assert!(Member::all().contains(&Member::Made(8)));
        set_made(&[]);
    }

    #[test]
    fn made_bots_join_the_crew() {
        let _g = CREW_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_made(&[Made {
            id: 1,
            name: "Scout".into(),
            role: "Checks the docs are current".into(),
            like: Member::Checker,
            look: [4, 2, 1, 0],
            outfit: String::new(),
            skills: Vec::new(),
            rules: Vec::new(),
            project: String::new(),
        }]);
        let all = Member::all();
        assert_eq!(all.len(), Member::CREW.len() + 2);
        let scout = Member::Made(1);
        assert_eq!(scout.name(), "Scout");
        assert_eq!(scout.role(), "Checks the docs are current");
        assert_eq!(scout.mode(), Mode::Test);
        assert!(scout.persona().contains("Scout"));
        assert_eq!(scout.accent(), squad_holo::Tint::BLUE.strong().0);
        // Renaming keeps the same bot.
        set_made(&[Made {
            id: 1,
            name: "Scout 2".into(),
            role: "Checks the docs are current".into(),
            like: Member::Checker,
            look: [4, 2, 1, 0],
            outfit: String::new(),
            skills: Vec::new(),
            rules: Vec::new(),
            project: String::new(),
        }]);
        assert_eq!(Member::Made(1).name(), "Scout 2");
        // Removing Checker: Scout takes its Test jobs.
        set_removed(&[Member::Checker]);
        assert_eq!(Member::doer_for(Mode::Test), Some(Member::Made(1)));
        assert!(!Member::Checker.on_crew());
        set_made(&[]);
        assert_eq!(Member::doer_for(Mode::Test), None);
        assert_eq!(Member::needed_for(Mode::Test), Some(Member::Checker));
        set_removed(&[]);
        assert_eq!(Member::doer_for(Mode::Test), Some(Member::Checker));
        assert_eq!(Member::needed_for(Mode::Test), None);
        assert_eq!(Member::all().len(), Member::CREW.len() + 1);
        assert_eq!(Member::Made(1).name(), "Bot");
        // Fin is always on the crew and answers questions when nobody else does.
        set_made(&[]);
        set_removed(&Member::CREW);
        assert_eq!(Member::all(), vec![Member::Fin]);
        assert!(Member::Fin.on_crew());
        assert_eq!(Member::doer_for(Mode::Ask), Some(Member::Fin));
        assert_eq!(Member::needed_for(Mode::Ask), None);
        assert_eq!(Member::needed_for(Mode::Fix), Some(Member::Fixer));
        // Explainer, once added, takes questions again.
        set_removed(&[]);
        assert_eq!(Member::doer_for(Mode::Ask), Some(Member::Explainer));
    }
}
