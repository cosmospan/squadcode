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
    /// A job that starts at `at` (seconds since 1970), on the night shift.
    Later {
        text: String,
        mode: Mode,
        bot: String,
        dir: String,
        at: u64,
    },
    /// Takes a waiting job out of the line.
    Cancel {
        id: u64,
    },
    /// Rewinds a job to before its turn number `to` (1 is its first follow-up).
    Rewind {
        id: u64,
        to: usize,
    },
    /// Forgets lesson `n` (from 1) that `bot` keeps in the project at `dir`.
    Forget {
        bot: String,
        dir: String,
        n: usize,
    },
    /// A new trigger for the project at `dir`.
    #[serde(rename = "add-trigger")]
    AddTrigger {
        trigger: Box<super::triggers::Trigger>,
        dir: String,
    },
    /// Turns trigger `id` on or off, runs it now or removes it.
    Trigger {
        id: u64,
        action: String,
    },
    /// A trigger's link called from this computer: its secret and what came with it.
    Fire {
        token: String,
        text: String,
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
  squad later [<when> <words...>]
                             jobs waiting to start, or one for the night
                             shift: tonight, 2am, at 14:30, in 2h, tomorrow
  squad later cancel <id>     take a waiting job out of the line
  squad issue <n|KEY|link> [words...]
                             a job for a GitHub or Linear issue; its pull
                             request closes the issue when it merges
  squad linear key <key>      connect Linear with a personal API key
  squad rewind <id> [<n>]     the points a job can go back to, or go back
                             to before its turn n
  squad lessons [forget <bot> <n>]
                             what the bots learned from your send-backs here
  squad project [init]        what squad.toml says here, or write a starter
  squad triggers              jobs that start by themselves, and their runs
  squad trigger add <when> [--bot <name>] [--name <name>] <words...>
                             a job that starts by itself in this folder.
                             <when> is schedule \"weekdays at 9am\" (or a
                             cron expression), label <name> [--author <login>],
                             ci [<branch>] [--workflow <name>] or link
  squad trigger on|off|run|rm <id>
                             turn a trigger on or off, run it now, remove it
  squad fire <secret> [words...]
                             call a link trigger from a script or git hook
  squad browser <command>     use the browser of the job working in this
                             folder: open, look, click, type, press, back,
                             screenshot, eval (squad browser help)

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

fn cwd() -> String {
    std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `squad triggers`: each trigger, what starts it and how its last run went.
fn triggers_list(s: &Store) -> String {
    if s.triggers.is_empty() {
        return "No triggers yet. squad trigger add schedule \"weekdays at 9am\" <words...> makes one.".into();
    }
    s.triggers
        .iter()
        .map(|t| {
            let last = match t.runs.last() {
                None => "never ran".to_string(),
                Some(r) if !r.skipped.is_empty() => format!("last skipped: {}", r.skipped),
                Some(r) if r.job != 0 => format!(
                    "last #{} {}",
                    r.job,
                    s.job(r.job).map(|j| j.status.label()).unwrap_or("gone")
                ),
                Some(_) => "last waiting to start".to_string(),
            };
            let mut line = format!(
                "{:<4} {:<4} {:<22} {:<30} {:<10} {}",
                t.id,
                if t.on { "on" } else { "off" },
                t.name,
                t.summary(),
                t.bot.name(),
                last
            );
            if let Some(token) = t.token() {
                line.push_str(&format!("\n     squad fire {token} [words...]"));
            }
            if !t.trouble.is_empty() {
                line.push_str(&format!("\n     {}", t.trouble));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `squad trigger add …` as a trigger, before the app checks it.
pub fn parse_trigger(args: &[String]) -> Result<super::triggers::Trigger, String> {
    use super::triggers::{Trigger, When};
    let usage = "usage: squad trigger add schedule \"weekdays at 9am\"|label <name>|ci [<branch>]|link [--bot <name>] [--name <name>] <words...>";
    let kind = args.first().ok_or(usage)?.as_str();
    let mut rest = args[1..].iter().peekable();
    let mut arg = String::new();
    let takes_arg = matches!(kind, "schedule" | "label")
        || (kind == "ci" && rest.peek().is_some_and(|a| !a.starts_with("--")));
    if takes_arg {
        arg = rest.next().cloned().ok_or(usage)?;
    }
    let (mut bot, mut name, mut author, mut workflow, mut words) = (
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        Vec::new(),
    );
    while let Some(a) = rest.next() {
        let mut val = || rest.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--bot" => bot = val()?,
            "--name" => name = val()?,
            "--author" => author = val()?,
            "--workflow" => workflow = val()?,
            "--" => {}
            w => words.push(w.to_string()),
        }
    }
    let when = match kind {
        "schedule" => When::Schedule {
            cron: crate::schedule::from_words(&arg)?,
        },
        "label" => When::Label { label: arg, author },
        "ci" => When::CiFails {
            branch: if arg.is_empty() { "main".into() } else { arg },
            workflow,
        },
        "link" => When::Link {
            token: super::triggers::new_token(),
        },
        _ => return Err(usage.into()),
    };
    let who = if bot.is_empty() {
        match &when {
            When::CiFails { .. } => Member::Fixer,
            _ => Member::Builder,
        }
    } else {
        Member::all()
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(&bot))
            .ok_or_else(|| format!("No bot called {bot} on the crew."))?
    };
    let text = words.join(" ").trim().to_string();
    if text.is_empty() {
        return Err("Say what the job is each time it starts.".into());
    }
    Ok(Trigger {
        name,
        when,
        text,
        bot: who,
        mode: who.mode(),
        ..Default::default()
    })
}

/// `squad trigger …`: add one, or turn one on or off, run it now or remove it.
fn trigger(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("add") => {
            let t = parse_trigger(&args[1..])?;
            let mut out = format!("Adding \u{201c}{}\u{201d}: {}.", t.text, t.summary());
            if let Some(token) = t.token() {
                out.push_str(&format!(
                    "\nCall it with: squad fire {token} [words...]\nor POST to http://127.0.0.1:{}/v1/hooks/{token} while the app is open (port {} under squad serve).",
                    super::triggers::HOOK_PORT,
                    super::server::PORT
                ));
            }
            push(&Ask::AddTrigger {
                trigger: Box::new(t),
                dir: cwd(),
            })?;
            Ok(out)
        }
        Some(action @ ("on" | "off" | "run" | "rm")) => {
            let id = args
                .get(1)
                .and_then(|s| s.parse().ok())
                .ok_or(format!("usage: squad trigger {action} <id>"))?;
            push(&Ask::Trigger {
                id,
                action: action.to_string(),
            })?;
            Ok(format!("Trigger {id}: {action}."))
        }
        _ => Err("usage: squad trigger add|on|off|run|rm …  (squad help)".into()),
    }
}

/// `squad later`: the waiting jobs, one for later, or cancelling one.
fn later(args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        let s = Store::load_view();
        if s.later.is_empty() {
            return Ok(
                "Nothing is waiting. squad later tonight <words...> puts a job on the night shift."
                    .into(),
            );
        }
        let now = super::store::now();
        return Ok(s
            .later
            .iter()
            .map(|l| {
                let when = if l.reading {
                    "reading its issue".to_string()
                } else if l.at == 0 {
                    format!("when {} is free", l.bot.name())
                } else {
                    super::shift::when_words(l.at, now)
                };
                format!(
                    "#{:<4} {:<10} {:<24} {}",
                    l.id,
                    l.bot.name(),
                    when,
                    l.label()
                )
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }
    if args[0] == "cancel" {
        let id = args
            .get(1)
            .map(|s| s.trim_start_matches('#'))
            .and_then(|s| s.parse().ok())
            .ok_or("usage: squad later cancel <id>")?;
        return push(&Ask::Cancel { id }).map(|_| format!("Taking #{id} out of the line."));
    }
    let typed = args.join(" ");
    let (at, text) = super::shift::parse_when(&typed, chrono::Local::now())
        .ok_or("Say when first: squad later tonight|2am|at 14:30|in 2h|tomorrow <words...>")?;
    let r = super::brain::cli::route_do(&text)?;
    push(&Ask::Later {
        text,
        mode: r.mode,
        bot: r.bot.name().to_string(),
        dir: cwd(),
        at,
    })?;
    Ok(format!(
        "{} starts it {}.",
        r.bot.name(),
        super::shift::when_words(at, super::store::now())
    ))
}

/// `squad issue`: a job for an issue.
fn issue(args: &[String]) -> Result<String, String> {
    let what = args.first().cloned().unwrap_or_default();
    let words = args.get(1..).unwrap_or_default().join(" ");
    let r = super::issues::find(&format!("issue {what}"))
        .or_else(|| super::issues::find(&what))
        .ok_or_else(|| {
            "usage: squad issue <number, Linear key like ENG-42, or link> [words...]".to_string()
        })?;
    let ask = match &r {
        super::issues::Ref::GitHub(n, Some(url)) => format!("Fix issue {n} ({url})"),
        super::issues::Ref::GitHub(n, None) => format!("Fix issue {n}"),
        super::issues::Ref::Linear(k) => format!("Fix {k}"),
    };
    let text = format!("{ask}. {words}")
        .trim()
        .trim_end_matches('.')
        .to_string();
    let r2 = super::brain::cli::route_do(&text)?;
    push(&Ask::New {
        text,
        mode: r2.mode,
        bot: r2.bot.name().to_string(),
        dir: cwd(),
    })?;
    Ok(format!(
        "{} reads {} and starts on it in Squad Code.",
        r2.bot.name(),
        r.label()
    ))
}

/// `squad lessons`: what the bots learned in this project, or forgetting one.
fn lessons(args: &[String]) -> Result<String, String> {
    let dir = cwd();
    let s = Store::load_view();
    let here = s
        .folders
        .iter()
        .filter(|f| !f.path.is_empty() && std::path::Path::new(&dir).starts_with(&f.path))
        .max_by_key(|f| f.path.len())
        .map(|f| f.path.clone())
        .unwrap_or(dir.clone());
    if args.first().map(String::as_str) == Some("forget") {
        let (Some(bot), Some(n)) = (args.get(1), args.get(2).and_then(|n| n.parse().ok())) else {
            return Err("usage: squad lessons forget <bot> <n>".into());
        };
        return push(&Ask::Forget {
            bot: bot.clone(),
            dir: here,
            n,
        })
        .map(|_| "Forgotten.".into());
    }
    let mut out = Vec::new();
    for m in Member::all() {
        let mine: Vec<&super::learn::Lesson> = s
            .lessons
            .iter()
            .filter(|l| l.bot == m && l.folder == here)
            .collect();
        if mine.is_empty() {
            continue;
        }
        out.push(format!("{}:", m.name()));
        for (i, l) in mine.iter().enumerate() {
            out.push(format!("  {}. {}", i + 1, l.text));
        }
    }
    Ok(if out.is_empty() {
        "No lessons here yet. When you send work back with a reason, the bot keeps what's worth keeping.".into()
    } else {
        out.join("\n")
    })
}

/// `squad project`: what the project file says, or writing a starter one.
fn project(args: &[String]) -> Result<String, String> {
    let dir = cwd();
    let path = std::path::Path::new(&dir).join(super::project::FILE);
    if args.first().map(String::as_str) == Some("init") {
        if path.exists() {
            return Err(format!("{} is already here.", super::project::FILE));
        }
        std::fs::write(&path, super::project::starter(&dir)).map_err(|e| e.to_string())?;
        return Ok(format!(
            "Wrote {}. Check it in so every job (and everyone's crew) sets up the same way.",
            super::project::FILE
        ));
    }
    match super::project::read(&dir)? {
        None => Ok(format!(
            "No {} here, so Squad works it out. squad project init writes one to start from.",
            super::project::FILE
        )),
        Some(p) => Ok(format!(
            "setup:  {}\ncopy:   {}\nrun:    {}\nurl:    {}\nport:   {}\nchecks: {}",
            p.setup.join(", "),
            p.copy.join(", "),
            p.run,
            p.url,
            p.port,
            p.checks.join(", ")
        )),
    }
}

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
        "later" => later(&args[1..]),
        "issue" => issue(&args[1..]),
        "linear" => match (args.get(1).map(String::as_str), args.get(2)) {
            (Some("key"), Some(key)) => super::issues::save_linear_key(key)
                .map(|_| "Linear is connected. Name an issue like ENG-42 in a job.".into()),
            _ => Err("usage: squad linear key <personal API key from Linear's settings>".into()),
        },
        "rewind" => id().and_then(|id| match args.get(2) {
            None => {
                let s = Store::load_view();
                let j = s.job(id).ok_or(format!("No job #{id}."))?;
                let points = super::rewind::points(j);
                if points.is_empty() {
                    return Ok(format!("#{id} has no turns to go back to."));
                }
                Ok(points
                    .iter()
                    .map(|k| format!("  {k}  before \u{201c}{}\u{201d}", j.marks[*k].said))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some(n) => {
                let to: usize = n.parse().map_err(|_| "usage: squad rewind <id> [<n>]")?;
                push(&Ask::Rewind { id, to }).map(|_| format!("Rewinding #{id}."))
            }
        }),
        "lessons" => lessons(&args[1..]),
        "triggers" => Ok(triggers_list(&Store::load_view())),
        "trigger" => trigger(&args[1..]),
        "fire" => match args.get(1) {
            Some(token) => push(&Ask::Fire {
                token: token.clone(),
                text: args[2..].join(" "),
            })
            .map(|_| "Handed to the crew.".into()),
            None => Err("usage: squad fire <secret> [words...]".into()),
        },
        "project" => project(&args[1..]),
        "brain" => super::brain::cli::main(&args[1..]),
        // The job's browser, from a shell: what engines without MCP use, and how the
        // desktop app finds a server job's browser (`browser port --dir …`).
        "browser" => {
            let cwd = std::env::current_dir()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default();
            crate::browse::cli::main(&args[1..], &cwd)
        }
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
    fn triggers_read_their_words() {
        use super::super::data::Member;
        use super::super::triggers::When;
        let mut args = words("schedule");
        args.push("weekdays at 9am".into());
        args.extend(words("--bot keeper tidy the flaky tests"));
        let t = super::parse_trigger(&args).unwrap();
        assert_eq!(
            t.when,
            When::Schedule {
                cron: "0 9 * * 1-5".into()
            }
        );
        assert_eq!(
            (t.bot, t.text.as_str()),
            (Member::Keeper, "tidy the flaky tests")
        );
        let t = super::parse_trigger(&words("ci --workflow tests fix it")).unwrap();
        assert_eq!(
            t.when,
            When::CiFails {
                branch: "main".into(),
                workflow: "tests".into()
            }
        );
        assert_eq!(t.bot, Member::Fixer);
        let t = super::parse_trigger(&words("label squad --author ana -- work on it")).unwrap();
        assert!(
            matches!(t.when, When::Label { ref label, ref author } if label == "squad" && author == "ana")
        );
        assert_eq!(
            super::parse_trigger(&words("link look at it"))
                .unwrap()
                .token()
                .map(str::len),
            Some(32)
        );
        assert!(super::parse_trigger(&words("label squad")).is_err());
        assert!(super::parse_trigger(&words("sometimes do it")).is_err());
        // It goes through the queue and back.
        let ask = Ask::AddTrigger {
            trigger: Box::new(t),
            dir: "/p".into(),
        };
        let line = serde_json::to_string(&ask).unwrap();
        assert_eq!(serde_json::from_str::<Ask>(&line).unwrap(), ask);
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
        let r = Ask::Rewind { id: 4, to: 2 };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Ask>(&line).unwrap(), r);
    }
}
