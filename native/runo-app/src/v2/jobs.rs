//! Running a job for real: the bot's engine (a coding agent) works in the
//! person's folder, and what it says and does streams into the job's feed.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

use crate::agent::{self, Event, Target};
use crate::harness::AgentEvent;
use crate::settings::AppSettings;

use super::data::Mode;
use super::engine;
use super::store::{now, Item, Job, Status};

/// A job's agent process, while it runs.
pub struct Runner {
    pub id: u64,
    handle: agent::Handle,
    rx: Receiver<Event>,
    parser: Option<crate::stream::Parser>,
    errors: Vec<String>,
    stopped: bool,
    /// When the engine reported its turn over, and whether it went well. A
    /// run that started a server keeps its output open after that, so the
    /// job ends on this rather than waiting for the process.
    result: Option<(std::time::Instant, bool)>,
}

/// How long after the engine's last word a run that hasn't exited counts as done.
const LINGER: std::time::Duration = std::time::Duration::from_secs(3);

/// What a bot that changes files may run without asking: any command, so
/// it can install, build, test and start what it made, in its own worktree.
pub const MAY_RUN: &[&str] = &["Bash"];

/// The engine's rules for the commands a bot never runs on its own (see
/// `brain::permit::EVERY_TIME`), Claude Code style. Without a way to ask
/// mid-run, the engine refuses these.
pub fn never_run() -> Vec<String> {
    super::brain::permit::EVERY_TIME
        .iter()
        .map(|c| format!("Bash({}:*)", c.trim()))
        .collect()
}

/// Whether hard-to-undo commands are asked about rather than refused: only
/// Claude Code with Squad Code's prompt tool can ask mid-run.
fn asks_first(provider: &str, prompt_tool: bool) -> bool {
    provider == "claude" && prompt_tool
}

/// What a bot that changes files is told about running things.
fn run_rule(checks: &[String]) -> String {
    let mut out = String::from(
        " You can run commands here: install, build, run the tests, and start what you made to see it work. \
         Start a server in the background, check it (for example with curl on localhost), then stop it, so the job can finish. \
         Don't push, deploy, publish or delete outside this folder; Squad Code does those after the person says yes.",
    );
    if !checks.is_empty() {
        out.push_str(&format!(
            " The project's checks are {}; Squad Code runs them again when you finish.",
            checks.join(", ")
        ));
    }
    out
}

/// Adds `more` to a comma-separated rule list, each once.
fn add_rules(list: &str, more: &[String]) -> String {
    let mut rules: Vec<String> = list
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    for r in more {
        if !rules.contains(r) {
            rules.push(r.clone());
        }
    }
    rules.join(",")
}

/// What the bot is told, around the person's words.
fn prompt(job: &Job, text: &str, follow_up: bool, checks: &[String]) -> String {
    if follow_up {
        return text.to_string();
    }
    let rules = match job.mode {
        Mode::Ask | Mode::Review => {
            "Read the code to be sure, and don't change any files.".to_string()
        }
        _ => format!(
            "Make the change in this folder. Keep a short todo list as you work.{}",
            run_rule(checks)
        ),
    };
    let part = if job.context.is_empty() {
        String::new()
    } else {
        format!("{}\n\n", job.context)
    };
    format!(
        "{}\n\n{part}{rules} When you finish, wrap up like a friendly teammate, for someone who may not code: open with the result in one upbeat line (what works now, or what you found), then how to try it in a sentence or two. Put each link (like http://localhost:3000), command or file path on its own, written out in full, so Squad Code can turn it into a button. If you need something from the person, end with one clear question, with the choices spelled out. Keep it short and warm; a little personality is welcome, a wall of text is not.\n\nThe job: {text}",
        super::setup::brief(job.bot)
    )
}

/// Starts `job` (or a follow-up in it) on the bot's engine.
pub fn start(
    st: &AppSettings,
    job: &mut Job,
    text: &str,
    follow_up: bool,
) -> Result<Runner, String> {
    let (i, model) = engine::for_job(st, job);
    let p = &agent::PROVIDERS[i];
    let mut opts = st.agent(p.id);
    // "Always allow" rules saved for this project from a job's card.
    if let Some(rules) = opts
        .project_tools
        .get(&job.folder.path)
        .filter(|r| !r.trim().is_empty())
        .cloned()
    {
        opts.allowed_tools = [opts.allowed_tools.trim(), rules.trim()]
            .into_iter()
            .filter(|r| !r.is_empty())
            .collect::<Vec<_>>()
            .join(",");
    }
    if !agent::available(p.bin, &opts.binary) {
        return Err(format!(
            "{} isn't installed on this computer. Pick another engine for {} in Engines.",
            p.name,
            job.bot.name()
        ));
    }
    // The bot's own setup: its connected apps, and what it may not do.
    let setup = super::setup::of(job.bot);
    opts.bot_mcp = setup.mcp.clone();
    opts.bot_servers = setup
        .servers
        .iter()
        .map(|s| (s.name.clone(), s.spec()))
        .collect();
    opts.disallowed_tools = add_rules(&opts.disallowed_tools, &setup.tool_rules());
    let mode = if job.mode.writes() && setup.perms.edit {
        agent::effective_mode(p.id, "edit")
    } else {
        "plan"
    };
    if let Some(why) = agent::mode_error(p.id, mode) {
        return Err(why);
    }
    let mut sel: HashMap<String, String> = HashMap::new();
    sel.insert("mode".into(), mode.into());
    if !model.is_empty() {
        sel.insert("model".into(), model.clone());
    }
    let resume = follow_up
        && job.agent == p.id
        && !job.conversation.is_empty()
        && agent::can_resume(p.id, &job.conversation);
    if !resume {
        job.conversation = if agent::names_conversation(p.id) {
            agent::new_uuid()
        } else {
            String::new()
        };
    }
    let dir = job.dir().to_string();
    // A bot that changes files may run the project's checks, so it can see
    // its change work instead of being refused.
    let checks = if job.mode.writes() {
        super::work::checks_for(&dir)
    } else {
        Vec::new()
    };
    // A bot that changes files works in its own worktree, so it may run
    // what it needs to build and test; only the hard-to-undo commands stay
    // off. A tool the person turned off in Settings stays off: their deny
    // rules win over this allow.
    // When Claude Code can ask through Squad Code's prompt tool (a run with
    // memory), the hard-to-undo commands are asked about with Go ahead /
    // Deny instead of refused: Claude never asks about a disallowed tool.
    // Runo's own MCP server asks you, in the job's card, before a tool the bot
    // isn't allowed runs, rather than the run being refused.
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let memory = (!exe.is_empty() && p.id == "claude").then_some((exe.as_str(), dir.as_str()));
    if job.mode.writes() {
        let may: Vec<String> = MAY_RUN.iter().map(|t| t.to_string()).collect();
        opts.allowed_tools = add_rules(&opts.allowed_tools, &may);
        if !asks_first(p.id, memory.is_some()) {
            opts.disallowed_tools = add_rules(&opts.disallowed_tools, &never_run());
        }
    }
    let sent = prompt(job, text, resume, &checks);
    let spec = agent::RunSpec {
        provider: p.id,
        prompt: &sent,
        resume,
        sel: &sel,
        opts: &opts,
        in_git: std::path::Path::new(&dir).join(".git").exists(),
        session_id: &job.conversation,
        images: &[],
        memory,
        local: true,
        thread: 0,
        trusted: false,
    };
    let (tx, rx) = channel();
    super::machine::use_server_screen();
    let handle = agent::run_agent(Target::Terminal(job.id), &spec, &dir, None, tx)?;
    job.agent = p.id.to_string();
    job.engine = engine::label_of(i, &model);
    job.status = Status::Running;
    job.ended = None;
    job.seen = false;
    Ok(Runner {
        id: job.id,
        handle,
        rx,
        parser: crate::stream::Parser::for_provider(p.id, &dir),
        errors: Vec::new(),
        stopped: false,
        result: None,
    })
}

impl Runner {
    pub fn stop(&mut self) {
        self.stopped = true;
        self.handle.kill();
    }

    /// Moves what arrived into the job. Returns true once the run has ended.
    pub fn pump(&mut self, job: &mut Job) -> bool {
        let mut ended = None;
        for _ in 0..400 {
            let Ok(ev) = self.rx.try_recv() else { break };
            match ev {
                Event::Line(_, line) if self.result.is_none() && is_result(&line) => {
                    self.result = Some((
                        std::time::Instant::now(),
                        !line.contains("\"is_error\":true"),
                    ));
                    if let Some(p) = &mut self.parser {
                        for e in p.feed(&line) {
                            apply(job, e);
                        }
                    }
                }
                Event::Line(_, line) => match &mut self.parser {
                    Some(p) => {
                        for e in p.feed(&line) {
                            apply(job, e);
                        }
                    }
                    None => say(job, &agent::strip_ansi(&line), true),
                },
                Event::Json(_, line, v) => {
                    if v["type"] == "result" && self.result.is_none() {
                        self.result = Some((std::time::Instant::now(), v["is_error"] != true));
                    }
                    if let Some(p) = &mut self.parser {
                        for e in p.feed_parsed(&line, Some(v)) {
                            apply(job, e);
                        }
                    }
                }
                Event::Err(_, line) => {
                    self.errors.push(agent::strip_ansi(&line));
                    if self.errors.len() > 40 {
                        self.errors.remove(0);
                    }
                }
                Event::Done(_, ok) => ended = Some(ok),
                Event::Output(..) | Event::Partial(..) => {}
            }
        }
        // The engine finished its turn but something it started (a dev
        // server, say) still holds the output open: the job is done all the same.
        let mut lingering = false;
        if ended.is_none() && !self.stopped {
            if let Some((_, ok)) = self.result.filter(|(at, _)| at.elapsed() > LINGER) {
                ended = Some(ok);
                lingering = true;
            }
        }
        let Some(ok) = ended else { return false };
        if lingering && ok {
            job.feed.push(Item::Note(
                "It left something running in the background, like the app's server. It keeps going until you stop it or quit it from the terminal."
                    .into(),
            ));
        }
        // Steps still marked running ended with the run.
        for it in &mut job.feed {
            if let Item::Tool { state, .. } = it {
                if *state == 0 {
                    *state = if ok { 1 } else { 2 };
                }
            }
        }
        job.ended = Some(now());
        job.status = if self.stopped {
            Status::Stopped
        } else if ok {
            Status::Done
        } else {
            Status::Failed
        };
        if job.status == Status::Failed && !job.feed.iter().any(|i| matches!(i, Item::Error(_))) {
            let log = self.errors.join("\n");
            let why = agent::sign_in_hint(&job.agent, &log).unwrap_or_else(|| {
                self.errors
                    .iter()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(|l| format!("The engine stopped: {}", l.trim()))
                    .unwrap_or_else(|| "The engine stopped without saying why.".into())
            });
            job.feed.push(Item::Error(why));
        }
        if job.status == Status::Done {
            for s in &mut job.plan {
                if s.1 == 1 {
                    s.1 = 2;
                }
            }
        }
        true
    }
}

/// Appends reply text, joining it to the bot's last words when they are still open.
fn say(job: &mut Job, text: &str, line: bool) {
    if text.is_empty() {
        return;
    }
    if let Some(Item::Text(last)) = job.feed.last_mut() {
        if line {
            last.push('\n');
        }
        last.push_str(text);
        return;
    }
    job.feed.push(Item::Text(text.trim_start().to_string()));
}

fn apply(job: &mut Job, e: AgentEvent) {
    use crate::harness::ToolKind;
    match e {
        AgentEvent::Text(t) => say(job, &t, false),
        AgentEvent::Plan(t) => job.feed.push(Item::Text(t)),
        AgentEvent::Tool(step) => {
            if matches!(step.kind, ToolKind::Todo) {
                return;
            }
            if matches!(
                step.kind,
                ToolKind::Edit | ToolKind::Write | ToolKind::Delete
            ) {
                if let Some(file) = step
                    .label
                    .split_once(' ')
                    .map(|(_, f)| f.trim().to_string())
                {
                    if !file.is_empty() && !job.files.contains(&file) {
                        job.files.push(file);
                    }
                }
            }
            let state = match step.state {
                crate::harness::StepState::Running => 0,
                crate::harness::StepState::Failed => 2,
                _ => 1,
            };
            job.feed.push(Item::Tool {
                id: step.id,
                label: step.label,
                kind: format!("{:?}", step.kind).to_lowercase(),
                state,
            });
        }
        AgentEvent::ToolDone { id, ok, .. } => {
            for it in job.feed.iter_mut().rev() {
                if let Item::Tool { id: i, state, .. } = it {
                    if *i == id && !id.is_empty() {
                        *state = if ok { 1 } else { 2 };
                        break;
                    }
                }
            }
        }
        AgentEvent::Summary(s) => job.summary = s,
        // A run reports its tokens so far; a follow-up starts a new count, so
        // keep the larger rather than adding one run's count twice.
        AgentEvent::Usage(u) => job.tokens = job.tokens.max(u.input + u.output),
        // The plan's limit windows, so the status bar follows them live.
        AgentEvent::Limit(l) => crate::usage::note_live(&job.agent, &l, crate::usage::now_secs()),
        AgentEvent::Error(s) => job.feed.push(Item::Error(s)),
        AgentEvent::Notice(s) => job.feed.push(Item::Note(s)),
        AgentEvent::Denied(tools) => job.feed.push(Item::Note(format!(
            "It was stopped from using {}. Bots don't push, deploy, publish or wipe files on their own; ask in a follow-up if this job needs it.",
            tools.join(", ")
        ))),
        AgentEvent::Todos(steps) => {
            use crate::crew::Mark;
            job.plan = steps
                .into_iter()
                .map(|s| {
                    let m = match s.mark {
                        Mark::Todo => 0,
                        Mark::Doing => 1,
                        Mark::Done => 2,
                    };
                    (s.text, m)
                })
                .collect();
        }
        AgentEvent::Conversation(c) => job.conversation = c,
        AgentEvent::Questions(qs) => {
            for q in qs {
                job.feed
                    .push(Item::Note(format!("It asked: {}", q.question)));
            }
        }
        _ => {}
    }
}

/// Whether a line of an engine's output is its end-of-turn result.
fn is_result(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('{') && (t.contains("\"type\":\"result\"") || t.contains("\"type\": \"result\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engines_last_word_is_spotted() {
        assert!(is_result(r#"{"type":"result","is_error":false}"#));
        assert!(!is_result(r#"{"type":"assistant"}"#));
        assert!(!is_result("the result is in"));
    }

    #[test]
    fn a_bot_that_changes_files_may_run_things_but_not_ship_or_wipe() {
        assert_eq!(MAY_RUN, &["Bash"]);
        assert!(asks_first("claude", true));
        assert!(!asks_first("claude", false) && !asks_first("grok", true));
        let never = never_run();
        for want in [
            "Bash(git push:*)",
            "Bash(rm -rf:*)",
            "Bash(sudo:*)",
            "Bash(vercel:*)",
        ] {
            assert!(never.iter().any(|r| r == want), "{want}");
        }
        for ok in ["npm", "node", "cargo", "pytest", "curl"] {
            assert!(!never.iter().any(|r| r == &format!("Bash({ok}:*)")), "{ok}");
        }
        let rule = run_rule(&["npm test".to_string()]);
        assert!(
            rule.contains("background") && rule.contains("npm test"),
            "{rule}"
        );
        assert_eq!(
            add_rules("Read, Bash", &["Bash".into(), "Edit".into()]),
            "Read,Bash,Edit"
        );
    }
}
