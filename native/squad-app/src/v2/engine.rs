//! Which engine runs under each bot, and asking a bot a question for real.
//!
//! An engine is one of the coding agents Squad Code already drives (Claude Code, Codex,
//! Gemini, OpenCode…) and the model it runs on. Each bot can have its own; the rest
//! follow the default agent from Settings. Squad Code's own model calls (shaping jobs,
//! writing the one question) use the assistant agent and model, and a second agent,
//! best from another vendor, can double-check decisions.

use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

use crate::agent::{self, Event};
use crate::settings::{AppSettings, BotEngine};

use super::data::Member;

/// The settings key for a bot.
pub fn bot_key(who: Member) -> String {
    match who {
        // By id, so renaming a made bot keeps its engine.
        Member::Made(id) => format!("made-{id}"),
        _ => who.name().to_lowercase(),
    }
}

/// The engines Settings and the first-run walkthrough always list, installed or
/// not; every other one is behind "Add an engine".
pub const COMMON: [&str; 6] = [
    "claude",
    "codex",
    "cursor",
    "opencode",
    "antigravity",
    "gemini",
];

/// The company behind an agent, so the challenger can be from another one.
pub fn vendor(agent_id: &str) -> &'static str {
    match agent_id {
        "claude" => "Anthropic",
        "codex" => "OpenAI",
        "gemini" | "antigravity" => "Google",
        "copilot" => "GitHub",
        "cursor" => "Cursor",
        "amp" => "Sourcegraph",
        "droid" => "Factory",
        "qwen" => "Alibaba",
        "grok" => "xAI",
        "opencode" => "Any model",
        _ => "Other",
    }
}

/// Index into `agent::PROVIDERS` of an agent id, or of the default agent for "".
pub fn index_of(st: &AppSettings, id: &str) -> usize {
    let id = if id.is_empty() {
        st.default_provider.as_str()
    } else {
        id
    };
    agent::PROVIDERS
        .iter()
        .position(|p| p.id == id)
        .unwrap_or(0)
}

/// Whether an agent can run on this computer.
pub fn ready(st: &AppSettings, i: usize) -> bool {
    let p = &agent::PROVIDERS[i];
    agent::available(p.bin, &st.agent(p.id).binary)
}

/// The engine a bot runs on: an agent index and a model ("" for its default).
pub fn engine(st: &AppSettings, who: Member) -> (usize, String) {
    let own = st
        .bot_engines
        .get(&bot_key(who))
        .cloned()
        .unwrap_or_default();
    let i = index_of(st, &own.agent);
    let model = if own.agent.is_empty() && own.model.is_empty() {
        st.agent(agent::PROVIDERS[i].id).model_override
    } else {
        own.model
    };
    (i, model)
}

/// The engine a job runs on: the one its race picked, else its bot's.
pub fn for_job(st: &AppSettings, job: &super::store::Job) -> (usize, String) {
    if job.pick.is_empty() {
        return engine(st, job.bot);
    }
    let i = index_of(st, &job.pick);
    let model = if job.pick_model.is_empty() {
        st.agent(agent::PROVIDERS[i].id).model_override
    } else {
        job.pick_model.clone()
    };
    (i, model)
}

/// The engines a race runs on: the bot's own first, then other engines that are
/// ready on this computer, at most `n`.
pub fn race_engines(st: &AppSettings, who: Member, n: usize) -> Vec<usize> {
    let (own, _) = engine(st, who);
    let mut out = vec![own];
    let default = index_of(st, "");
    // The default agent next, then the rest in the order Squad Code lists them.
    let rest = std::iter::once(default).chain(0..agent::PROVIDERS.len());
    for i in rest {
        if out.len() >= n {
            break;
        }
        if !out.contains(&i) && ready(st, i) {
            out.push(i);
        }
    }
    out
}

/// Another engine ready on this computer for a job whose own engine can't run
/// it now: the default agent first, then the rest.
pub fn fallback(st: &AppSettings, job: &super::store::Job) -> Option<usize> {
    let (own, _) = for_job(st, job);
    race_engines(st, job.bot, 8)
        .into_iter()
        .find(|i| *i != own && agent::PROVIDERS[*i].id != job.agent)
}

/// "Claude Code" or "Claude Code · opus" for an agent index and model.
pub fn label_of(i: usize, model: &str) -> String {
    let name = agent::PROVIDERS[i].name;
    match crate::agent_pick::AgentPick::model_label(i, model) {
        Some(m) => format!("{name} · {m}"),
        None => name.to_string(),
    }
}

/// "Claude Code" or "Claude Code · opus".
pub fn label(st: &AppSettings, who: Member) -> String {
    let (i, model) = engine(st, who);
    let name = agent::PROVIDERS[i].name;
    match crate::agent_pick::AgentPick::model_label(i, &model) {
        Some(m) => format!("{name} · {m}"),
        None => name.to_string(),
    }
}

pub fn set(st: &mut AppSettings, who: Member, engine: BotEngine) {
    if engine == BotEngine::default() {
        st.bot_engines.remove(&bot_key(who));
    } else {
        st.bot_engines.insert(bot_key(who), engine);
    }
}

/// A question on its way to a bot's engine.
pub struct Ask {
    pub who: Member,
    pub engine: String,
    pub agent_id: &'static str,
    pub started: Instant,
    rx: Receiver<Event>,
}

/// How a bot answers: about the code it reads the code first, but not every
/// message is about the code, and in a discussion it takes part instead of
/// describing the project's files.
const TALK_RULE: &str = " Never change any files. When the person asks about this project's code, read the code to be sure. When they're discussing, planning or asking about something else (people, teams, process, ideas), take part as a thoughtful teammate: respond to what they said, using what you know, and don't list or describe the project's files unless they ask.";

/// How a bot says an ask is real work. It can only read here, so when the
/// person wants the code changed it offers a job, which the person starts.
const JOB_RULE: &str = " If, and only if, the person wants you to change, add, fix or build something in the code (not just to explain it), end with one more line on its own: \"JOB: <the change, as a short instruction>\". For questions, greetings and explanations, never add that line.";

/// Splits a bot's reply into its words and the job it offers, if any: the
/// last line when it reads "JOB: …". Empty when it offers none.
pub fn split_job(reply: &str) -> (String, String) {
    let reply = reply.trim_end();
    let (head, last) = match reply.rsplit_once('\n') {
        Some((h, l)) => (h, l),
        None => ("", reply),
    };
    let tag = last.trim().trim_matches(['*', '_', '`']).trim();
    let task = tag
        .get(..4)
        .filter(|t| t.eq_ignore_ascii_case("job:"))
        .map(|_| {
            tag[4..]
                .trim()
                .trim_matches(['*', '_', '`', '"'])
                .trim()
                .to_string()
        });
    match task {
        Some(t) if !t.is_empty() => (head.trim_end().to_string(), t),
        // "JOB:" with nothing after it offers nothing, and isn't shown.
        Some(_) => (head.trim_end().to_string(), String::new()),
        None => (reply.to_string(), String::new()),
    }
}

/// What came back.
pub enum Reply {
    Answer(String),
    Failed(String),
}

impl Ask {
    /// Asks `who` the question on its own engine, read-only, in the project folder.
    pub fn start(
        st: &AppSettings,
        who: Member,
        folder: &super::store::Folder,
        question: &str,
        history: &[(bool, String)],
    ) -> Result<Ask, String> {
        let (i, model) = engine(st, who);
        let p = &agent::PROVIDERS[i];
        let configured = st.agent(p.id).binary;
        if !agent::available(p.bin, &configured) {
            return Err(format!(
                "{} isn't installed on this computer. Pick another engine for {} in Engines.",
                p.name,
                who.name()
            ));
        }
        let (project, dir) = (&folder.name, &folder.path);
        let mut prompt = format!(
            "{}{TALK_RULE}\n\nThe project is \"{project}\", in the current folder.\n",
            super::setup::brief(who)
        );
        let recent: Vec<_> = history.iter().rev().take(6).collect();
        if !recent.is_empty() {
            prompt.push_str("\nThe conversation so far:\n");
            for (mine, text) in recent.into_iter().rev() {
                prompt.push_str(if *mine { "Person: " } else { "You: " });
                prompt.push_str(text);
                prompt.push('\n');
            }
        }
        prompt.push_str(&format!(
            "\nQuestion: {question}\n\nAnswer in under 150 words.{JOB_RULE}"
        ));
        let command = agent::ask_command_on(p.id, &configured, &prompt, &model);
        let (tx, rx) = channel();
        agent::oneshot(command, dir, tx);
        Ok(Ask {
            who,
            engine: label(st, who),
            agent_id: p.id,
            started: Instant::now(),
            rx,
        })
    }

    /// The reply, once it has arrived.
    pub fn poll(&self) -> Option<Reply> {
        match self.rx.try_recv() {
            Ok(Event::Output(Ok(text))) => {
                Some(Reply::Answer(agent::strip_ansi(&text).trim().to_string()))
            }
            Ok(Event::Output(Err(err))) => {
                let help = agent::sign_in_hint(self.agent_id, &err);
                let first = err
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("it stopped without saying why")
                    .trim()
                    .to_string();
                Some(Reply::Failed(
                    help.unwrap_or_else(|| format!("The engine stopped: {first}")),
                ))
            }
            Ok(_) => None,
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Some(Reply::Failed("The engine stopped before answering.".into()))
            }
        }
    }
}

#[cfg(test)]
mod job_tests {
    use super::split_job;

    #[test]
    fn a_question_offers_no_job() {
        let (words, job) = split_job("I'm Pixel, the UI-polishing bot.");
        assert_eq!(words, "I'm Pixel, the UI-polishing bot.");
        assert!(job.is_empty());
    }

    #[test]
    fn a_job_line_at_the_end_is_the_offer() {
        let (words, job) = split_job(
            "They're 28px tall.\nI'd go to 32px.\n\n**JOB: Make the sidebar buttons 32px tall**\n",
        );
        assert_eq!(words, "They're 28px tall.\nI'd go to 32px.");
        assert_eq!(job, "Make the sidebar buttons 32px tall");
        let (_, job) = split_job("Sure.\njob: \"fix the login\"");
        assert_eq!(job, "fix the login");
    }

    #[test]
    fn job_in_the_middle_or_empty_offers_nothing() {
        let (words, job) = split_job("JOB: not last\nmore words");
        assert!(job.is_empty() && words.contains("not last"));
        let (words, job) = split_job("Hm.\nJOB:");
        assert!(job.is_empty() && words == "Hm.");
    }
}
