//! Setting up a new bot: what it's for, the kind of work it does, what it knows
//! best, its rules, and the rest of its setup (instructions, skill files,
//! connected apps, permissions, memory).
//!
//! The person says what the bot is for in a sentence, and the rest is drafted:
//! 1. [`full_prompt`], asked of the person's engine in the project's folder:
//!    it reads the project and drafts the whole bot, picking from the skill
//!    files and connected apps already on this computer and writing new skills
//!    where none fit. [`parse_full`] checks the answer.
//! 2. [`quick`], at once and built in, for when no engine can run: the kind
//!    of work the person picked (building, if none), its skills from the words
//!    that name its subject, and a safe set of rules for that kind ([`draft`]).
//!
//! The skills are what routing matches a new job against, so a job about
//! Stripe goes to the bot that knows Stripe.

use std::sync::mpsc::{channel, Receiver};

use serde_json::Value;

use crate::agent::{self, Event};
use crate::settings::AppSettings;

use super::super::data::{Member, Mode};
use super::words;

/// A bot's setup, before it joins the crew.
#[derive(Clone, Debug, PartialEq)]
pub struct Design {
    pub role: String,
    pub like: Member,
    pub skills: Vec<String>,
    pub rules: Vec<String>,
}

/// Words that say nothing about a bot's subject.
const PLAIN: [&str; 48] = [
    "the", "a", "an", "and", "or", "for", "to", "of", "in", "on", "it", "is", "my", "our", "your",
    "this", "that", "with", "bot", "job", "jobs", "all", "any", "who", "what", "when", "make",
    "makes", "keep", "keeps", "handle", "handles", "look", "looks", "after", "every", "things",
    "stuff", "work", "works", "fix", "fixes", "write", "writes", "code", "tests", "test", "up",
];

/// The words that name what a bot is about: "Stripe Pro: payments, Stripe
/// webhooks and billing bugs" → stripe, pro, payments, webhooks, billing, bugs.
pub fn skill_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words(text) {
        if w.len() < 3
            || PLAIN.contains(&w.as_str())
            || super::route::COMMON.contains(&w.as_str())
            || w.chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        if !out.contains(&w) {
            out.push(w);
        }
    }
    out.truncate(10);
    out
}

/// A safe starting set of rules for each kind of work.
pub fn rules_for(like: Member) -> Vec<String> {
    let r: &[&str] = match like.mode() {
        Mode::Fix => &[
            "Find the cause before changing code, and show the evidence.",
            "Add a test that fails without the fix.",
            "Keep the change to what the bug needs.",
        ],
        Mode::Build => &[
            "Read how the project already does similar things, and match it.",
            "Add tests for what you build.",
            "Ask before adding a new dependency.",
        ],
        Mode::Test => &[
            "Run the project's own test command, and report what passed and what broke.",
            "Never skip, disable or loosen a test to make it pass.",
        ],
        Mode::Review => &[
            "Change nothing: read and report.",
            "Lead with the riskiest thing you found, with the file and line.",
        ],
        Mode::Tidy => &[
            "Change how the code reads, never what it does.",
            "Run the tests before and after.",
        ],
        Mode::Ask => &[
            "Change nothing: answer from the code, with the files you read.",
            "Use plain words for people who may not code.",
        ],
    };
    r.iter().map(|s| s.to_string()).collect()
}

/// The built-in setup for a bot named `name` that's for `role`. `like` is the
/// kind of work the person picked, or None to work it out from the role.
pub fn draft(name: &str, role: &str, like: Option<Member>) -> Design {
    // With no kind picked, a made bot builds; the person can change it.
    let like = like.unwrap_or(Member::Builder);
    let role = if role.trim().is_empty() {
        like.role().to_string()
    } else {
        role.trim().to_string()
    };
    Design {
        skills: skill_words(&format!("{name} {role}")),
        rules: rules_for(like),
        role,
        like,
    }
}

/// Everything the engine drafts for a new bot, checked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Full {
    pub name: String,
    pub role: String,
    /// The kind of work, when the engine named one.
    pub like: Option<Member>,
    pub skills: Vec<String>,
    pub rules: Vec<String>,
    pub instructions: String,
    pub memory: Vec<String>,
    pub reads: Vec<String>,
    /// Skill files already on this computer it should use, by name.
    pub use_skills: Vec<String>,
    /// New skill files to write for it: (name, description, body).
    pub new_skills: Vec<(String, String, String)>,
    /// Connected apps already on this computer it should use, by name.
    pub use_mcp: Vec<String>,
    pub perms: super::super::setup::Perms,
}

/// What the person's engine is asked, in the project's folder, to draft a bot
/// from a sentence: `about` is what the person wrote, `name` the name they
/// gave (may be empty). `skills` and `apps` are what this computer already
/// has, as (name, what it's for), so the draft can reuse them.
pub fn full_prompt(
    about: &str,
    name: &str,
    project: &str,
    skills: &[(String, String)],
    apps: &[String],
) -> String {
    let list = |items: Vec<String>| {
        if items.is_empty() {
            "(none)".to_string()
        } else {
            items.join("\n")
        }
    };
    let skills = list(
        skills
            .iter()
            .take(60)
            .map(|(n, d)| format!("- {n}: {}", d.chars().take(140).collect::<String>()))
            .collect(),
    );
    let apps = list(apps.iter().take(40).map(|a| format!("- {a}")).collect());
    let name = if name.trim().is_empty() {
        "(pick a short, friendly one-word name)".to_string()
    } else {
        name.trim().to_string()
    };
    format!(
        "Squad Code's crew is getting a new bot for the project \"{project}\" (the current folder). \
         The person described it as: \"{about}\". Name: {name}.\n\n\
         Read the project first (its package or build files, CI config, tests, docs, contributing notes), \
         then design the bot to fit it. Change no files.\n\n\
         Skill files already on this computer:\n{skills}\n\n\
         Connected apps (MCP servers) already on this computer:\n{apps}\n\n\
         Reply with JSON only, nothing else:\n\
         {{\"name\": \"its name\",\n\
         \"role\": \"what it's for, one short line\",\n\
         \"kind\": \"one of build, fix, test, review, tidy, ask: the kind of work it mostly does\",\n\
         \"skills\": [\"3 to 10 single lowercase words naming its own area, the things only it handles \
         (a payments bot: stripe, webhooks, invoices); never general words like code, app, api or tests\"],\n\
         \"instructions\": \"Markdown, 8 to 25 lines: who it is, which folders and files it owns, how it works here, \
         what good work looks like. Specific to this project.\",\n\
         \"rules\": [\"3 to 6 short rules, such as the exact test command, folders to leave alone, when to ask first\"],\n\
         \"memory\": [\"3 to 6 facts about this project it should know from day one (where config lives, conventions, commands)\"],\n\
         \"reads\": [\"up to 5 files, folders or doc links it should read before a job\"],\n\
         \"use_skills\": [\"names from the skill files above that fit it; empty if none do\"],\n\
         \"new_skills\": [{{\"name\": \"short-kebab-name\", \"description\": \"when to use it, one line\", \
         \"body\": \"Markdown steps for a task it will repeat in this project\"}}],\n\
         \"use_apps\": [\"names from the connected apps above that it needs; empty if none\"],\n\
         \"may\": {{\"edit\": true, \"install\": true, \"web\": true, \"push\": true}}}}\n\n\
         new_skills: at most 2, only for a repeated task no existing skill covers. \
         may: edit is false for a bot that only reads and reports; install, web and push false when the person ruled them out \
         or the bot has no need for them. Everything must stay true for months: how to work here, never what's true today."
    )
}

/// The engine's draft, checked: None when the reply holds no usable bot.
pub fn parse_full(reply: &str) -> Option<Full> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let v: Value = serde_json::from_str(reply.get(start..=end)?).ok()?;
    let text = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let list = |k: &str, max_words: usize, max: usize| -> Vec<String> {
        v[k].as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && s.split_whitespace().count() <= max_words)
                    .take(max)
                    .collect()
            })
            .unwrap_or_default()
    };
    let role = text("role").trim_end_matches('.').to_string();
    if role.is_empty() || role.split_whitespace().count() > 20 {
        return None;
    }
    let like = match text("kind").to_lowercase().as_str() {
        "build" => Some(Mode::Build),
        "fix" => Some(Mode::Fix),
        "test" => Some(Mode::Test),
        "review" => Some(Mode::Review),
        "tidy" => Some(Mode::Tidy),
        "ask" => Some(Mode::Ask),
        _ => None,
    }
    .map(Member::for_mode);
    let skills: Vec<String> = list("skills", 2, 10)
        .into_iter()
        .map(|s| s.to_lowercase())
        .filter(|s| !super::route::COMMON.contains(&s.as_str()) && !PLAIN.contains(&s.as_str()))
        .collect();
    let new_skills = v["new_skills"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let f = |k: &str| x[k].as_str().unwrap_or("").trim().to_string();
                    let (n, d, b) = (f("name"), f("description"), f("body"));
                    (!n.is_empty() && !b.is_empty()).then_some((n, d, b))
                })
                .take(2)
                .collect()
        })
        .unwrap_or_default();
    let may = |k: &str| v["may"][k].as_bool().unwrap_or(true);
    let name: String = text("name").chars().take(24).collect();
    Some(Full {
        name: name.trim().to_string(),
        role,
        like,
        skills,
        rules: list("rules", 40, 8),
        instructions: text("instructions"),
        memory: list("memory", 40, 8),
        reads: list("reads", 6, 6),
        use_skills: list("use_skills", 3, 12),
        new_skills,
        use_mcp: list("use_apps", 4, 12),
        perms: super::super::setup::Perms {
            edit: may("edit"),
            install: may("install"),
            web: may("web"),
            push: may("push"),
        },
    })
}

/// A draft without the engine, from the words alone, for when no engine can
/// run or the project isn't open: kind, skills and rules for that kind.
pub fn quick(about: &str, name: &str) -> Full {
    let name = if name.trim().is_empty() {
        guess_name(about)
    } else {
        name.trim().to_string()
    };
    // The sentence is the role when it's short; a long one is cut at its first clause.
    let role: String = about
        .split(['.', ':', ';'])
        .next()
        .unwrap_or(about)
        .trim()
        .chars()
        .take(90)
        .collect();
    let d = draft(&name, &role, None);
    Full {
        instructions: format!("# {name}\n\n{}\n\n{}", about.trim(), d.like.persona()),
        name,
        role: d.role,
        like: Some(d.like),
        skills: subject_words(about),
        rules: d.rules,
        ..Full::default()
    }
}

/// Words that say what a bot does, not what it's about: no name or skill.
const DOING: [&str; 30] = [
    "own", "owns", "find", "finds", "review", "reviews", "prepare", "prepares", "upgrade",
    "upgrades", "check", "checks", "never", "always", "touch", "green", "live", "update",
    "updates", "help", "helps", "run", "runs", "keeps", "fixing", "writing", "making", "new",
    "only", "before",
];

/// The words that name what a bot is about, without the ones saying what it does.
fn subject_words(text: &str) -> Vec<String> {
    skill_words(text)
        .into_iter()
        .filter(|w| !DOING.contains(&w.as_str()))
        .collect()
}

/// A one-word name from what the bot is for: the first name in it ("Stripe"),
/// else its first subject word, capitalised.
fn guess_name(about: &str) -> String {
    let named = about
        .split(|c: char| !c.is_alphanumeric())
        .skip(1)
        .find(|w| w.len() > 2 && w.chars().next().is_some_and(char::is_uppercase))
        .map(str::to_lowercase);
    named
        .or_else(|| subject_words(about).into_iter().next())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .unwrap_or_else(|| "Scout".to_string())
}

/// The engine that drafts bots: the person's default agent and model, which
/// knows the project best. ("Claude Code", agent index, model).
pub fn drafter(st: &AppSettings) -> (usize, String) {
    let i = super::super::engine::index_of(st, "");
    (i, st.agent(agent::PROVIDERS[i].id).model_override)
}

/// Asks the drafting engine, in `folder`, on another thread. Err when it
/// can't run here; the caller falls back to [`quick`].
pub fn ask_full(
    st: &AppSettings,
    folder: &super::super::store::Folder,
    prompt: &str,
) -> Result<Receiver<Event>, String> {
    let (i, model) = drafter(st);
    let p = &agent::PROVIDERS[i];
    let configured = st.agent(p.id).binary;
    if !agent::available(p.bin, &configured) {
        return Err(format!("{} isn't installed", p.name));
    }
    let command = agent::ask_command_on(p.id, &configured, prompt, &model);
    let (tx, rx) = channel();
    agent::oneshot("v2-maker".into(), command, &folder.path, tx);
    Ok(rx)
}

/// The skills and connected apps on this computer, for the draft to pick from.
pub fn on_computer(found: &crate::settings::Discovered) -> (Vec<(String, String)>, Vec<String>) {
    let skills = found
        .skills
        .iter()
        .map(|s| (s.name.clone(), s.description.clone()))
        .collect();
    let mut apps: Vec<String> = found
        .mcp
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.name.clone())
        .collect();
    apps.dedup();
    (skills, apps)
}

/// Turns the draft's picks into a setup for `who`: names into the skill
/// files and servers found on this computer, and new skills written as files
/// in the bot's folder.
pub fn setup_from(
    who: Member,
    full: &Full,
    found: &crate::settings::Discovered,
) -> super::super::setup::Setup {
    use super::super::setup::{write_skill, Setup, SkillRef};
    let mut skill_files: Vec<SkillRef> = Vec::new();
    for name in &full.use_skills {
        if let Some(s) = found
            .skills
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name.trim()))
        {
            if !skill_files.iter().any(|k| k.name == s.name) {
                skill_files.push(SkillRef {
                    name: s.name.clone(),
                    description: s.description.clone(),
                    path: skill_md(&s.path),
                });
            }
        }
    }
    for (name, description, body) in &full.new_skills {
        if let Ok(s) = write_skill(who, name, description, body) {
            skill_files.retain(|k| k.name != s.name);
            skill_files.push(s);
        }
    }
    let mut mcp: Vec<String> = Vec::new();
    for name in &full.use_mcp {
        if let Some(m) = found
            .mcp
            .iter()
            .find(|m| m.enabled && m.name.eq_ignore_ascii_case(name.trim()))
        {
            let key = crate::mcp::key(m);
            if !mcp.contains(&key) {
                mcp.push(key);
            }
        }
    }
    Setup {
        instructions: full.instructions.clone(),
        skill_files,
        mcp,
        servers: Vec::new(),
        perms: full.perms,
        reads: full.reads.clone(),
        memory: full.memory.clone(),
        drafted: true,
    }
}

/// A skill's SKILL.md, when `path` is its folder.
pub fn skill_md(path: &std::path::Path) -> std::path::PathBuf {
    if path.is_dir() {
        path.join("SKILL.md")
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bot_for_bugs_works_like_fixer_and_knows_its_subject() {
        let d = draft(
            "Stripe Pro",
            "fixes Stripe webhook and billing bugs",
            Some(Member::Fixer),
        );
        assert_eq!(d.like, Member::Fixer);
        assert!(d.skills.contains(&"stripe".to_string()), "{:?}", d.skills);
        assert!(d.skills.contains(&"billing".to_string()));
        assert!(!d.skills.contains(&"fixes".to_string()));
        assert!(!d.rules.is_empty());
    }

    #[test]
    fn with_no_pick_and_no_engine_a_bot_builds() {
        assert_eq!(draft("Helper", "does things", None).like, Member::Builder);
    }

    #[test]
    fn the_persons_pick_wins() {
        let d = draft(
            "Scribe",
            "keeps the README up to date",
            Some(Member::Builder),
        );
        assert_eq!(d.like, Member::Builder);
        assert!(d.skills.contains(&"readme".to_string()));
    }

    #[test]
    fn a_review_bot_changes_nothing() {
        let d = draft(
            "Lock",
            "reviews auth changes for security risks",
            Some(Member::Guard),
        );
        assert_eq!(d.like, Member::Guard);
        assert!(d.rules[0].contains("Change nothing"));
    }

    #[test]
    fn the_engines_draft_is_checked() {
        let good = "Here:\n{\"name\": \"Ledger\", \"role\": \"Owns billing.\", \"kind\": \"fix\", \"skills\": [\"Stripe\", \"billing\", \"a whole sentence is not a skill\"], \"instructions\": \"# Ledger\\nYou own src/billing.\", \"rules\": [\"Run pnpm vitest before you stop.\"], \"memory\": [\"Money is integer cents\"], \"reads\": [\"docs/payments.md\"], \"use_skills\": [\"tdd\"], \"new_skills\": [{\"name\": \"stripe-webhooks\", \"description\": \"Replaying events\", \"body\": \"1. Run stripe trigger\"}, {\"name\": \"empty\"}], \"use_apps\": [\"github\"], \"may\": {\"install\": false}}";
        let f = parse_full(good).unwrap();
        assert_eq!(f.name, "Ledger");
        assert_eq!(f.role, "Owns billing");
        assert_eq!(f.like, Some(Member::Fixer));
        assert_eq!(f.skills, vec!["stripe", "billing"]);
        assert!(f.instructions.starts_with("# Ledger"));
        assert_eq!(f.new_skills.len(), 1);
        assert_eq!(f.use_mcp, vec!["github"]);
        assert!(!f.perms.install && f.perms.edit && f.perms.push);
        assert!(parse_full("{\"role\": \"\"}").is_none());
        assert!(parse_full("sorry").is_none());
    }

    #[test]
    fn a_quick_draft_needs_no_engine() {
        let f = quick(
            "Keeps the README and changelog up to date. Never touches code.",
            "",
        );
        assert_eq!(f.name, "Readme");
        assert!(f.role.starts_with("Keeps the README"));
        assert!(f.instructions.contains("Never touches code"));
        assert!(!f.rules.is_empty());
        let f = quick("Own our Stripe integration: never touch live keys.", "");
        assert_eq!(f.name, "Stripe");
        assert!(!f.skills.contains(&"own".to_string()) && !f.skills.contains(&"never".to_string()));
    }

    #[test]
    fn the_prompt_offers_what_the_computer_has() {
        let p = full_prompt(
            "owns stripe",
            "",
            "cove",
            &[("tdd".into(), "test first".into())],
            &["github".into()],
        );
        assert!(p.contains("- tdd: test first"));
        assert!(p.contains("- github"));
        assert!(p.contains("pick a short"));
    }
}
