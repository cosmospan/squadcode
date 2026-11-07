//! `squad`: the crew from a terminal. Reading commands (ls, show) read the
//! saved store; the rest (new, approve, discard, stop) go in a queue the open
//! app picks up within a second, so it stays the one place jobs run.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::data::{Member, Mode};
use super::store::{Job, Store};

/// One thing asked from the command line, waiting for the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "lowercase")]
pub enum Ask {
    New {
        text: String,
        mode: Mode,
        bot: String,
        dir: String,
    },
    Approve {
        id: u64,
    },
    Discard {
        id: u64,
    },
    Stop {
        id: u64,
    },
}

pub fn queue_path() -> PathBuf {
    crate::store::data_dir().join("v2-queue.jsonl")
}

/// Takes what's waiting in the queue, emptying it.
pub fn take_queue() -> Vec<Ask> {
    let p = queue_path();
    if !p.exists() {
        return Vec::new();
    }
    let taken = p.with_extension("taken");
    if std::fs::rename(&p, &taken).is_err() {
        return Vec::new();
    }
    let text = std::fs::read_to_string(&taken).unwrap_or_default();
    let _ = std::fs::remove_file(&taken);
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn push(ask: &Ask) -> Result<(), String> {
    let p = queue_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = serde_json::to_string(ask).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

const HELP: &str = "squad: hand the crew jobs from a terminal.

  squad ls                    jobs, newest first
  squad show <id>             one job: where it stands and what it changed
  squad new <words...>        a Build job in this folder
  squad fix|test|tidy|review <words...>
                             a job of that kind
  squad new --bot <name> <words...>
                             a job for one bot
  squad do <words...>         Peek picks the bot and the kind of job
  squad approve <id>          approve a job waiting for review
  squad discard <id>          throw a job's changes away
  squad stop <id>             stop a running job

On a server:
  squad serve [--listen <address>|tailscale]
                             run the crew here with no window, for other
                             devices to reach (127.0.0.1:7640 by default)
  squad serve --install       keep it running after you log out and after
                             a restart (a systemd user service)
  squad serve --set KEY=value keep a key for the agents here (such as
                             ANTHROPIC_API_KEY) and restart to use it
  squad pair [name]           a one-time code to pair a device with
  squad devices               paired devices
  squad unpair <name>         stop a device reaching the crew

Peek's routing:
  squad brain why <words...>  where Peek would send a job, and why
  squad brain ideas           a bot Peek thinks you could use
  squad brain install [file]  turn Smart Peek on with Squad's own model
  squad brain update          get the newest Peek model, if there is one
  squad brain remove          turn Smart Peek off and delete its model

Jobs run in the Squad Code app while it's open, or in `squad serve`.";

/// One line per job for `ls`.
pub fn line(j: &Job) -> String {
    let title: String = j
        .title
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(60)
        .collect();
    format!(
        "#{:<4} {:<22} {:<10} {}",
        j.id,
        j.status.label(),
        j.bot.name(),
        title
    )
}

/// Reads the words of a `new` command: an optional `--bot <name>` and the job.
pub fn parse_new(mode: Mode, args: &[String]) -> Result<(Mode, String, String), String> {
    let mut bot = String::new();
    let mut words = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--bot" {
            bot = it.next().cloned().ok_or("--bot needs a name")?;
        } else {
            words.push(a.clone());
        }
    }
    let mut mode = mode;
    if !bot.is_empty() {
        let who = Member::all()
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(&bot))
            .ok_or_else(|| format!("No bot called {bot} on the crew."))?;
        mode = who.mode();
        bot = who.name().to_string();
    }
    let text = words.join(" ").trim().to_string();
    if text.is_empty() && mode.default_job().is_none() {
        return Err("Say what the job is: squad new <words...>".into());
    }
    Ok((mode, bot, text))
}

pub fn main(args: &[String]) -> i32 {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let id = || -> Result<u64, String> {
        args.get(1)
            .map(|s| s.trim_start_matches('#'))
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("usage: squad {cmd} <job id>"))
    };
    let result: Result<String, String> = match cmd {
        "ls" | "list" => {
            let s = Store::load_view();
            Ok(if s.jobs.is_empty() {
                "No jobs yet.".to_string()
            } else {
                s.jobs
                    .iter()
                    .rev()
                    .take(30)
                    .map(line)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        "show" => id().and_then(|id| {
            let s = Store::load_view();
            let j = s.job(id).ok_or(format!("No job #{id}."))?;
            let mut out = format!(
                "#{} {}\n{} \u{b7} {} \u{b7} {}\n",
                j.id,
                j.title,
                j.bot.name(),
                j.status.label(),
                j.engine
            );
            if !j.branch.is_empty() {
                out.push_str(&format!("Branch {} from {}\n", j.branch, j.base));
            }
            for c in &j.changes {
                out.push_str(&format!(
                    "  {} {}  +{} -{}\n",
                    c.status, c.path, c.added, c.removed
                ));
            }
            if let Some(w) = j.last_words() {
                out.push('\n');
                out.push_str(&super::live::plain(w));
            }
            Ok(out)
        }),
        "new" | "build" | "fix" | "test" | "tidy" | "review" => {
            let mode = match cmd {
                "fix" => Mode::Fix,
                "test" => Mode::Test,
                "tidy" => Mode::Tidy,
                "review" => Mode::Review,
                _ => Mode::Build,
            };
            parse_new(mode, &args[1..]).and_then(|(mode, bot, text)| {
                let dir = std::env::current_dir()
                    .map(|d| d.to_string_lossy().into_owned())
                    .unwrap_or_default();
                push(&Ask::New {
                    text,
                    mode,
                    bot,
                    dir,
                })?;
                Ok("Handed to the crew. It shows up in Squad Code in a moment.".into())
            })
        }
        "do" => {
            let text = args[1..].join(" ").trim().to_string();
            if text.is_empty() {
                Err("Say what the job is: squad do <words...>".into())
            } else {
                match super::brain::cli::route_do(&text) {
                    Err(e) => Err(e),
                    Ok(r) => {
                        let dir = std::env::current_dir()
                            .map(|d| d.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        push(&Ask::New {
                            text,
                            mode: r.mode,
                            bot: r.bot.name().to_string(),
                            dir,
                        })
                        .map(|_| {
                            format!(
                                "Handed to {} as a {} job. {}",
                                r.bot.name(),
                                r.mode.label(),
                                r.why
                            )
                            .trim_end()
                            .to_string()
                        })
                    }
                }
            }
        }
        "brain" => super::brain::cli::main(&args[1..]),
        "approve" => {
            id().and_then(|id| push(&Ask::Approve { id }).map(|_| format!("Approving #{id}.")))
        }
        "discard" => {
            id().and_then(|id| push(&Ask::Discard { id }).map(|_| format!("Discarding #{id}.")))
        }
        "stop" => id().and_then(|id| push(&Ask::Stop { id }).map(|_| format!("Stopping #{id}."))),
        "serve" => return super::server::serve(&args[1..]),
        "pair" => return super::server::pair_command(&args[1..]),
        "devices" => return super::server::devices_command(),
        "unpair" => return super::server::unpair_command(&args[1..]),
        "help" | "-h" | "--help" => Ok(HELP.into()),
        other => Err(format!(
            "squad doesn't know \u{201c}{other}\u{201d}.\n\n{HELP}"
        )),
    };
    match result {
        Ok(out) => {
            println!("{out}");
            0
        }
        Err(why) => {
            eprintln!("{why}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::data::Mode;
    use super::{parse_new, Ask};

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn new_jobs_read_their_words() {
        let (mode, bot, text) = parse_new(Mode::Build, &words("add a dark mode")).unwrap();
        assert_eq!(
            (mode, bot.as_str(), text.as_str()),
            (Mode::Build, "", "add a dark mode")
        );
        let (mode, bot, _) =
            parse_new(Mode::Build, &words("--bot fixer the login is broken")).unwrap();
        assert_eq!((mode, bot.as_str()), (Mode::Fix, "Fixer"));
        assert!(parse_new(Mode::Build, &[]).is_err());
        // Tidy has a job of its own when you say nothing.
        assert!(parse_new(Mode::Tidy, &[]).is_ok());
        assert!(parse_new(Mode::Build, &words("--bot nobody x")).is_err());
    }

    #[test]
    fn asks_survive_the_queue() {
        let a = Ask::New {
            text: "x".into(),
            mode: Mode::Fix,
            bot: String::new(),
            dir: "/tmp".into(),
        };
        let line = serde_json::to_string(&a).unwrap();
        assert!(line.contains("\"do\":\"new\""));
        assert_eq!(serde_json::from_str::<Ask>(&line).unwrap(), a);
    }
}
