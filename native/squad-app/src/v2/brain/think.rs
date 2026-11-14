//! Peek reads each message with the person's own engine.
//!
//! Every message Peek gets is one short call on the person's own engine
//! (Claude Code, Codex, Grok…, on their own plan), on its smallest model,
//! which answers with a decision as JSON: reply itself, ask back, stop
//! what's running, run the app, answer a question about the project, or
//! hand the work to a named bot with a one-line reason. Peek's words are the
//! model's.

use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use serde_json::Value;

use super::front::Front;
use super::route::{self, Route};
use crate::agent::{self, Event};
use crate::settings::AppSettings;
use crate::v2::data::{Member, Mode};

/// How long Peek waits for the engine before the rules decide.
pub const LIMIT: Duration = Duration::from_secs(40);

/// What Peek knows when it reads a message.
pub struct Seen<'a> {
    /// The project's name.
    pub project: &'a str,
    /// The thread's name, like "tech-discussion"; empty when there is none.
    pub thread: &'a str,
    /// The thread's running summary and the person's notes on it; empty
    /// when it has none yet.
    pub summary: &'a str,
    /// The conversation so far, oldest first, as (who, words).
    pub recent: &'a [(String, String)],
    /// The bots on the crew.
    pub crew: &'a [Member],
    /// Jobs running here, as "#4 Builder: add a login page".
    pub running: &'a [String],
    /// The bots with an open job in this thread, which more work for them
    /// goes into.
    pub in_thread: &'a [Member],
    /// Where jobs can run besides this computer, in a sentence or two;
    /// empty when no server is connected.
    pub server: &'a str,
    /// How the person chose to send it with the box's chips: "Do" for a job,
    /// "Ask" for an answer; empty when they left it to Peek.
    pub picked: &'a str,
}

fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "\u{2026}"
    }
}

/// Who Peek is and how it decides: the same for every read. The warm engine
/// gets it once, as its system prompt; a cold read puts it before the turn.
pub const RULES: &str = "You are Peek, who leads a crew of AI bots in Squad Code, an app for developers and for people who don't code. \
You read every message the person sends before anyone works on it, and decide what happens. Don't use any tools; just reply. \
Never tell the person to open a terminal or run something themselves: Squad runs commands and apps for them, inside the app. A command you write in backticks gets a Run button that does that.\n\n\
Each message to you gives the project, the thread and its summary, the crew, what's running, where jobs run, the conversation, and what the person just wrote. \
Decide, reading it the way a sharp colleague would: typos, slang and half sentences are normal (\"whoa re you\" is \"who are you\"). \
A short reply like \"1\", \"yes\" or \"the second\" answers the last question or list of choices in the conversation: act on that choice, and when a bot asked it, carry on with that bot. \
Never start work on a guess.\n\
Not every message is about the code. A thread can be for discussion, planning or talk among the team, and its name often says so. \
When the person is discussing, planning, brainstorming, or talking about people, teams, process or ideas rather than this project's files, \
use \"reply\" and take part as a thoughtful teammate: respond to what they said, add a useful point, or ask one follow-up about the topic itself. \
Don't steer a discussion toward building something in the app, and don't offer code work as choices unless they asked for it. \
If they say it's a discussion, believe them.\n\
- \"reply\": chat, greetings, thanks, questions about you or the crew, and general questions you can answer well yourself. Write the reply.\n\
- \"ask\": you can't tell what they want, or real work is too vague to start. Write one short question, with up to 3 choices if they help. In a discussion, prefer \"reply\".\n\
- \"stop\": they want what's running stopped or cancelled.\n\
- \"run\": they want the app started or run so they can see it (\"runthe app\", \"start it up\", \"let me see it\"). Squad Code starts it; no bot is needed.\n\
- \"answer\": a question about this project's code or files, which needs reading them. A bot answers without changing anything. Not for discussion or general talk.\n\
- \"hand\": real work on the project. Pick the bot whose role fits best; when the message carries on work a bot has open in this thread, pick that bot, since it goes into its open job. Each bot does one kind of work, so the bot decides it. Say why in one short, friendly sentence in your own voice, like \"Sounds like a bug in the Safari checkout.\" Don't name the bot or say you're bringing it in. \
If they want it done on their server (a VPS, the cloud, \"remote\", the server's name), set \"where\" to \"server\": Squad Code copies the project there if it isn't yet and runs the job there, so never say the crew can only work on this computer. If they want it on this computer, \"here\". Otherwise leave it empty. \
If they want the server but none is connected, \"ask\" them to connect one under Settings, My server.\n\n\
Reply with JSON only, on one line:\n\
{\"do\": \"reply|ask|stop|run|answer|hand\", \"say\": \"what Peek says\", \"choices\": [], \"bot\": \"a name from the crew, or empty\", \"kind\": \"build|fix|test|review|tidy|ask, only when bot is empty\", \"where\": \"server|here, or empty\"}\n\
Keep \"say\" warm, plain and short, no markdown: two sentences at most, or up to four when you're taking part in a discussion.";

/// A long message cut down to about `n` characters, keeping its start and
/// its end: a bot's question or list of choices comes last, so the end is
/// what a short reply like "1" or "yes" answers.
fn clip_ends(s: &str, n: usize) -> String {
    let s = s.trim();
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let head = n / 3;
    let tail = n - head;
    let start: String = s.chars().take(head).collect();
    let end: String = s.chars().skip(count - tail).collect();
    format!("{} \u{2026} {}", start.trim_end(), end.trim_start())
}

/// The conversation as the engine reads it: the last few messages, oldest
/// first, each clipped at both ends. The newest message gets the most room,
/// so a question a bot just asked reaches Peek whole.
pub fn conversation(seen: &Seen) -> Vec<String> {
    let shown: Vec<&(String, String)> = seen.recent.iter().rev().take(8).rev().collect();
    let last = shown.len().saturating_sub(1);
    shown
        .into_iter()
        .enumerate()
        .map(|(i, (who, said))| {
            let room = if i == last { 2000 } else { 400 };
            format!("{who}: {}", clip_ends(said, room))
        })
        .collect()
}

/// What changes from one read to the next. `conversation` is the part of
/// the conversation to show, under `heading`.
pub fn turn(text: &str, seen: &Seen, heading: &str, conversation: &[String]) -> String {
    let crew: Vec<String> = seen
        .crew
        .iter()
        .filter(|m| **m != Member::Fin)
        .map(|m| {
            format!(
                "- {} ({} work): {}{}",
                m.name(),
                m.mode().label().to_lowercase(),
                m.role(),
                if seen.in_thread.contains(m) {
                    " [has an open job in this thread]"
                } else {
                    ""
                }
            )
        })
        .collect();
    let running = if seen.running.is_empty() {
        "nothing".to_string()
    } else {
        seen.running.join("; ")
    };
    format!(
        "Project: {project}\n\
Thread: {thread}\n\
{summary}\
The crew:\n{crew}\n\
Running right now: {running}\n\
Where jobs run: {server}\n\
{heading}\n{conversation}\n\n\
The person just wrote: \"{text}\"\n\
{picked}\
Reply with the JSON decision only.",
        project = seen.project,
        summary = if seen.summary.trim().is_empty() {
            String::new()
        } else {
            format!("{}\n", seen.summary.trim())
        },
        thread = if seen.thread.is_empty() {
            "(none)".to_string()
        } else {
            format!("#{}", seen.thread)
        },
        server = if seen.server.is_empty() {
            "only this computer: no server is connected."
        } else {
            seen.server
        },
        crew = crew.join("\n"),
        conversation = if conversation.is_empty() {
            "(none)".to_string()
        } else {
            conversation.join("\n")
        },
        text = clip(text, 2000).replace('"', "'"),
        picked = match seen.picked {
            "Do" => "They picked Do: they want it done as a job, so \"hand\" it to the bot it's for, unless it plainly isn't work on the project.\n",
            "Ask" => "They picked Ask: they want an answer, not a job, so \"reply\" or \"answer\".\n",
            _ => "",
        },
    )
}

/// Everything the engine reads for one message, for a cold read.
pub fn prompt(text: &str, seen: &Seen) -> String {
    format!(
        "{RULES}\n\n{}",
        turn(text, seen, "The conversation so far:", &conversation(seen))
    )
}

/// The engine's decision as Peek acts on it, and Peek's own line for a
/// hand-off. None when the reply doesn't hold a decision that makes sense,
/// so the rules decide instead.
pub fn parse(reply: &str, text: &str, crew: &[Member]) -> Option<(Front, String)> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let v: Value = serde_json::from_str(reply.get(start..=end)?).ok()?;
    let say = v["say"].as_str().unwrap_or("").trim().to_string();
    let front = match v["do"].as_str()?.trim() {
        "reply" if !say.is_empty() => Front::Chat { reply: say.clone() },
        "ask" if !say.is_empty() => Front::AskBack {
            question: say.clone(),
            choices: v["choices"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str())
                        .map(|c| c.trim().to_string())
                        .filter(|c| !c.is_empty() && c.chars().count() <= 60)
                        .take(3)
                        .collect()
                })
                .unwrap_or_default(),
        },
        "stop" => Front::Stop,
        "run" => Front::RunApp,
        "answer" => Front::Answer {
            question: text.to_string(),
            code: true,
        },
        "hand" => {
            let mode = match v["kind"].as_str().unwrap_or("").trim() {
                "fix" => Mode::Fix,
                "test" => Mode::Test,
                "review" => Mode::Review,
                "tidy" => Mode::Tidy,
                "ask" => Mode::Ask,
                _ => Mode::Build,
            };
            let named = v["bot"].as_str().unwrap_or("").trim().to_lowercase();
            // The bot decides the kind of work; the kind only picks a bot
            // when none was named.
            let bot = crew
                .iter()
                .copied()
                .find(|m| *m != Member::Fin && m.name().to_lowercase() == named)
                .unwrap_or_else(|| route::doer(mode, crew));
            let r = Route {
                mode: bot.mode(),
                bot,
                sure: 0.9,
                why: say.clone(),
                by: "engine".into(),
                site: match v["where"].as_str().unwrap_or("").trim() {
                    s @ ("server" | "here") => s.to_string(),
                    _ => String::new(),
                },
            };
            // The guard rails hold, whoever decides.
            match super::guard::risk(text) {
                Some(why) => Front::Confirm { why, route: r },
                None => Front::Hand(r),
            }
        }
        _ => return None,
    };
    Some((front, say))
}

/// Asks the person's engine to read `text`, on another thread. Err when no
/// engine can run here; the rules decide then.
pub fn ask(
    st: &AppSettings,
    dir: &str,
    text: &str,
    seen: &Seen,
) -> Result<Receiver<Event>, String> {
    let i = crate::settings::assistant_provider(st);
    let p = &agent::PROVIDERS[i];
    let configured = st.agent(p.id).binary;
    if !agent::available(p.bin, &configured) {
        return Err(format!("{} isn't installed", p.name));
    }
    let model = crate::settings::assistant_model(st, p.id);
    // The warm engine answers in a second or two; a cold start takes six.
    if super::warm::can(p.id) {
        let bin = if configured.trim().is_empty() {
            p.bin.to_string()
        } else {
            configured.trim().to_string()
        };
        let said = conversation(seen);
        let turn_for = |new_only: bool, shown: &[String]| {
            let heading = if new_only {
                "New in the conversation since your last read:"
            } else {
                "The conversation so far:"
            };
            turn(text, seen, heading, shown)
        };
        if let Some(rx) = super::warm::ask(&bin, &model, dir, &said, &turn_for, LIMIT) {
            return Ok(rx);
        }
    }
    let command = agent::ask_command_on(p.id, &configured, &prompt(text, seen), &model);
    let (tx, rx) = channel();
    agent::oneshot_within(command, dir, LIMIT, tx);
    Ok(rx)
}

/// Reads `text` with the engine and waits for the decision, for a terminal.
/// Err says why the engine couldn't decide.
pub fn decide(
    st: &AppSettings,
    dir: &str,
    text: &str,
    seen: &Seen,
) -> Result<(Front, String), String> {
    let rx = ask(st, dir, text, seen)?;
    let reply = match rx.recv_timeout(LIMIT + Duration::from_secs(5)) {
        Ok(Event::Output(r)) => r?,
        _ => return Err(format!("{} didn't answer in time", engine_name(st))),
    };
    parse(&agent::strip_ansi(&reply), text, seen.crew)
        .ok_or_else(|| format!("{}'s answer didn't make sense", engine_name(st)))
}

/// Starts Peek's engine warm for `dir`, so the next message reads fast.
/// Nothing for an engine that can't be kept warm, or one not installed.
pub fn prepare(st: &AppSettings, dir: &str) {
    if dir.is_empty() {
        return;
    }
    let p = &agent::PROVIDERS[crate::settings::assistant_provider(st)];
    if !super::warm::can(p.id) {
        return;
    }
    let configured = st.agent(p.id).binary;
    if !agent::available(p.bin, &configured) {
        return;
    }
    let bin = if configured.trim().is_empty() {
        p.bin.to_string()
    } else {
        configured.trim().to_string()
    };
    let model = crate::settings::assistant_model(st, p.id);
    super::warm::prepare(&bin, &model, dir);
}

/// The name of the engine Peek reads with, as the person knows it.
pub fn engine_name(st: &AppSettings) -> &'static str {
    agent::PROVIDERS[crate::settings::assistant_provider(st)].name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crew() -> Vec<Member> {
        Member::CREW.to_vec()
    }

    #[test]
    fn the_prompt_names_the_thread_so_a_discussion_reads_as_one() {
        let c = crew();
        let p = prompt(
            "lets onboard a few research teams",
            &Seen {
                project: "shop",
                thread: "tech-discussion",
                summary: "",
                recent: &[],
                crew: &c,
                running: &[],
                in_thread: &[],
                server: "",
                picked: "",
            },
        );
        assert!(p.contains("Thread: #tech-discussion"));
        assert!(p.contains("Don't steer a discussion toward building something"));
    }

    #[test]
    fn a_bots_last_question_reaches_peek_whole() {
        let c = crew();
        let long = format!(
            "We're in agreement. {} Which niche should we research first?\n1. Tutors\n2. Therapists",
            "Some reasoning. ".repeat(60)
        );
        let recent = vec![
            ("explainer".to_string(), long.clone()),
            ("explainer".to_string(), long),
        ];
        let said = conversation(&Seen {
            project: "shop",
            thread: "",
            summary: "",
            recent: &recent,
            crew: &c,
            running: &[],
            in_thread: &[],
            server: "",
            picked: "",
        });
        // Even an older long message keeps its question at the end.
        assert!(said[0].ends_with("2. Therapists"), "{}", said[0]);
        assert!(said[0].chars().count() < 450);
        assert!(said[1].contains("Which niche should we research first?"));
        assert!(said[1].starts_with("explainer: We're in agreement."));
    }

    #[test]
    fn a_chip_pick_is_told_to_peek_not_a_way_around_it() {
        let c = crew();
        let seen = |picked| Seen {
            project: "shop",
            thread: "generaldiscussion",
            summary: "",
            recent: &[],
            crew: &c,
            running: &[],
            in_thread: &[],
            server: "",
            picked,
        };
        let text = "see if you can do live search now";
        assert!(prompt(text, &seen("Do")).contains("They picked Do"));
        assert!(prompt(text, &seen("Ask")).contains("They picked Ask"));
        assert!(!prompt(text, &seen("")).contains("They picked"));
    }

    #[test]
    fn the_prompt_carries_the_crew_and_what_is_running() {
        let recent = vec![("You".to_string(), "add a login page".to_string())];
        let running = vec!["#4 Builder: add a login page".to_string()];
        let c = crew();
        let p = prompt(
            "whoa re you",
            &Seen {
                project: "shop",
                thread: "",
                summary: "",
                recent: &recent,
                crew: &c,
                running: &running,
                in_thread: &[Member::Builder],
                server: "vps: connected, Claude Code signed in.",
                picked: "",
            },
        );
        for want in [
            "Builder",
            "Fixer",
            "#4 Builder",
            "whoa re you",
            "\"do\"",
            "has an open job in this thread",
            "vps: connected",
            "Not every message is about the code",
        ] {
            assert!(p.contains(want), "{want}");
        }
    }

    #[test]
    fn replies_become_what_peek_does() {
        let c = crew();
        let (f, _) = parse(
            r#"{"do":"reply","say":"I'm Peek, I run your crew.","choices":[],"bot":"","kind":""}"#,
            "whoa re you",
            &c,
        )
        .unwrap();
        assert_eq!(
            f,
            Front::Chat {
                reply: "I'm Peek, I run your crew.".into()
            }
        );
        let (f, line) = parse(
            "Sure.\n{\"do\":\"hand\",\"say\":\"A new page, so Builder's on it.\",\"bot\":\"builder\",\"kind\":\"build\"}",
            "add a pricing page",
            &c,
        )
        .unwrap();
        match f {
            Front::Hand(r) => {
                assert_eq!(r.bot, Member::Builder);
                assert_eq!(r.mode, Mode::Build);
            }
            other => panic!("{other:?}"),
        }
        assert!(line.contains("Builder"));
        assert_eq!(
            parse(r#"{"do":"stop","say":""}"#, "stop it", &c).unwrap().0,
            Front::Stop
        );
        // The guard rails still hold.
        assert!(matches!(
            parse(
                r#"{"do":"hand","say":"Shipping it.","bot":"Shipper","kind":"build"}"#,
                "deploy the new pricing page to production",
                &c
            )
            .unwrap()
            .0,
            Front::Confirm { .. }
        ));
        // "do it on the VPS" runs there.
        match parse(
            r#"{"do":"hand","say":"A game, coming up.","bot":"Builder","kind":"build","where":"server"}"#,
            "make a snake game on the vps",
            &c,
        )
        .unwrap()
        .0
        {
            Front::Hand(r) => assert_eq!(r.site, "server"),
            other => panic!("{other:?}"),
        }
        // Nonsense falls back to the rules.
        assert!(parse("I think you mean hello", "hi", &c).is_none());
        assert!(parse(r#"{"do":"reply","say":""}"#, "hi", &c).is_none());
        assert!(parse(r#"{"do":"dance"}"#, "hi", &c).is_none());
    }
}
