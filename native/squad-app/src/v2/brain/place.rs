//! Where a job runs: on this computer or on the person's server, and on which
//! engine.
//!
//! The rules, in order:
//! 1. No server set up, or it doesn't answer: here.
//! 2. Run where the project lives. A project only one side has runs there.
//! 3. Run where the bot's agent is signed in. If only one side has it, that
//!    side; if neither, the side with another engine ready.
//! 4. Long work (races, goals, big builds) goes to the server when both sides
//!    could run it, so this computer stays free and the work carries on with
//!    the lid closed.
//! 5. Otherwise here, where it's quickest to watch.
//!
//! Every answer says why in one sentence, for the job's feed, and names the
//! other side as the fallback when it could run the job too.
//!
//! [`place`] gathers the facts (is the folder here, is the agent installed,
//! the folder's git remote) and [`decide`] applies the rules to them, so the
//! rules are tested without a computer or a server.

use crate::agent;
use crate::settings::AppSettings;

use super::super::data::{Member, Mode};
use super::super::{engine, store};

/// A job about to start, as the layer sees it.
#[derive(Clone, Debug)]
pub struct JobAsk {
    pub bot: Member,
    pub mode: Mode,
    pub text: String,
    pub folder: store::Folder,
    /// How many engines it races on; 0 or 1 is no race.
    pub race: usize,
    /// It's one step of a goal.
    pub goal: bool,
}

/// The person's server, as `squad serve` last described it.
#[derive(Clone, Debug, Default)]
pub struct Server {
    pub name: String,
    /// `GET /v1/hello` answered.
    pub reachable: bool,
    /// Agent ids signed in and ready there.
    pub engines: Vec<String>,
    /// Projects it has: git remote URLs, or folder names when there's no remote.
    pub projects: Vec<String>,
}

/// Where jobs can run besides here.
#[derive(Clone, Debug, Default)]
pub struct Sites {
    /// `None` when no server is set up.
    pub server: Option<Server>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Site {
    Here,
    Server(String),
}

/// The layer's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub site: Site,
    /// An agent index into `agent::PROVIDERS` and a model ("" for its default).
    pub engine: (usize, String),
    /// One sentence for the job's feed.
    pub why: String,
    /// The other side, when it could run the job too.
    pub fallback: Option<Site>,
}

/// What [`decide`] works from.
#[derive(Clone, Debug)]
pub struct Facts {
    /// The bot's own engine.
    pub engine: (usize, String),
    /// Its agent id, such as "claude".
    pub agent: String,
    /// The bot's agent can run here.
    pub agent_here: bool,
    /// Another engine that can run here, when the bot's own can't.
    pub other_here: Option<usize>,
    /// The project folder exists on this computer.
    pub project_here: bool,
    /// The project's git remote, "" when it has none.
    pub remote: String,
    pub long: bool,
}

/// Whether a job is long enough to be worth sending away.
pub fn is_long(ask: &JobAsk) -> bool {
    if ask.race > 1 || ask.goal {
        return true;
    }
    if !ask.mode.writes() {
        return false;
    }
    let lower = ask.text.to_lowercase();
    let big = [
        "migrate",
        "migration",
        "rewrite",
        "whole",
        "entire",
        "every ",
        "across the",
        "all the",
        "port ",
        "overnight",
    ];
    ask.text.len() > 600 || big.iter().any(|w| lower.contains(w))
}

/// "github.com/acme/app" from any spelling of a git remote, for comparing.
pub fn same_project(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.trim().to_lowercase();
        let s = s
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("ssh://")
            .trim_start_matches("git@");
        let s = s.replacen(':', "/", 1);
        s.trim_end_matches('/').trim_end_matches(".git").to_string()
    }
    !a.trim().is_empty() && norm(a) == norm(b)
}

/// The rules, applied to facts gathered elsewhere.
pub fn decide(ask: &JobAsk, f: &Facts, sites: &Sites) -> Placement {
    let here = |engine: (usize, String), why: String, fallback: Option<Site>| Placement {
        site: Site::Here,
        engine,
        why,
        fallback,
    };
    let fallback_engine = |f: &Facts| -> (usize, String) {
        match (f.agent_here, f.other_here) {
            (false, Some(i)) => (i, String::new()),
            _ => f.engine.clone(),
        }
    };
    let server = match &sites.server {
        None => {
            return here(fallback_engine(f), String::new(), None);
        }
        Some(s) if !s.reachable => {
            return here(
                fallback_engine(f),
                format!("Running here: {} isn't answering right now.", s.name),
                None,
            );
        }
        Some(s) => s,
    };
    let on_server = Site::Server(server.name.clone());
    let project_there = server.projects.iter().any(|p| {
        same_project(p, &f.remote) || (!p.contains('/') && p.eq_ignore_ascii_case(&ask.folder.name))
    });
    let agent_there = server.engines.contains(&f.agent);
    let any_there = !server.engines.is_empty();
    let name = agent::PROVIDERS[f.engine.0].name;

    // 2. Where the project lives.
    match (f.project_here, project_there) {
        (true, false) => {
            return here(
                fallback_engine(f),
                format!(
                    "Running here: {} doesn't have {} yet.",
                    server.name, ask.folder.name
                ),
                None,
            );
        }
        (false, true) => {
            return Placement {
                site: on_server,
                engine: f.engine.clone(),
                why: format!(
                    "Running on {}: {} isn't on this computer.",
                    server.name, ask.folder.name
                ),
                fallback: None,
            };
        }
        (false, false) => {
            return here(
                fallback_engine(f),
                format!(
                    "Neither this computer nor {} has {}.",
                    server.name, ask.folder.name
                ),
                None,
            );
        }
        (true, true) => {}
    }
    let can_here = f.agent_here || f.other_here.is_some();
    // 3. Where the bot's agent is signed in.
    match (f.agent_here, agent_there) {
        (true, false) => {
            return here(
                f.engine.clone(),
                format!("Running here: {name} isn't signed in on {}.", server.name),
                any_there.then(|| on_server.clone()),
            );
        }
        (false, true) => {
            return Placement {
                site: on_server,
                engine: f.engine.clone(),
                why: format!(
                    "Running on {}: {name} is signed in there, not here.",
                    server.name
                ),
                fallback: can_here.then_some(Site::Here),
            };
        }
        (false, false) => {
            return here(
                fallback_engine(f),
                format!(
                    "{name} isn't set up here or on {}, so another engine runs it.",
                    server.name
                ),
                None,
            );
        }
        (true, true) => {}
    }
    // 4. Long work goes to the server.
    if f.long {
        let what = if ask.race > 1 {
            "a race"
        } else if ask.goal {
            "part of a goal"
        } else {
            "a long job"
        };
        return Placement {
            site: on_server,
            engine: f.engine.clone(),
            why: format!(
                "Running on {}: it's {what}, so this computer stays free.",
                server.name
            ),
            fallback: Some(Site::Here),
        };
    }
    // 5. Here.
    here(
        f.engine.clone(),
        "Running here, where it's quickest to watch.".into(),
        Some(on_server),
    )
}

/// The folder's git remote, or "". Asked of git at most every few minutes
/// per folder: this runs as each job starts, on the window's thread.
pub fn remote_of(path: &str) -> String {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static KNOWN: Mutex<Option<HashMap<String, (String, Instant)>>> = Mutex::new(None);
    const FRESH: Duration = Duration::from_secs(300);
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    let known = known.get_or_insert_with(HashMap::new);
    if let Some((remote, at)) = known.get(path) {
        if at.elapsed() < FRESH {
            return remote.clone();
        }
    }
    let remote = ask_git_remote(path);
    known.insert(path.to_string(), (remote.clone(), Instant::now()));
    remote
}

fn ask_git_remote(path: &str) -> String {
    std::process::Command::new("git")
        .args(["-C", path, "remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Where a job runs, and on which engine. Call it right before `make_job`.
pub fn place(st: &AppSettings, ask: &JobAsk, sites: &Sites) -> Placement {
    let (i, model) = engine::engine(st, ask.bot);
    let agent_here = engine::ready(st, i);
    let other_here = if agent_here {
        None
    } else {
        engine::race_engines(st, ask.bot, 8)
            .into_iter()
            .find(|j| *j != i && engine::ready(st, *j))
    };
    // Git is only asked when there's a server to compare with.
    let remote = if sites.server.as_ref().is_some_and(|s| s.reachable) {
        remote_of(&ask.folder.path)
    } else {
        String::new()
    };
    let facts = Facts {
        engine: (i, model),
        agent: agent::PROVIDERS[i].id.to_string(),
        agent_here,
        other_here,
        project_here: std::path::Path::new(&ask.folder.path).is_dir(),
        remote,
        long: is_long(ask),
    };
    decide(ask, &facts, sites)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(mode: Mode, text: &str) -> JobAsk {
        JobAsk {
            bot: Member::Builder,
            mode,
            text: text.into(),
            folder: store::Folder {
                name: "app".into(),
                path: "/work/app".into(),
                ship: String::new(),
                ..Default::default()
            },
            race: 0,
            goal: false,
        }
    }

    fn facts() -> Facts {
        Facts {
            engine: (0, String::new()),
            agent: agent::PROVIDERS[0].id.to_string(),
            agent_here: true,
            other_here: None,
            project_here: true,
            remote: "git@github.com:acme/app.git".into(),
            long: false,
        }
    }

    fn server() -> Sites {
        Sites {
            server: Some(Server {
                name: "my-vps".into(),
                reachable: true,
                engines: vec![agent::PROVIDERS[0].id.to_string()],
                projects: vec!["https://github.com/acme/app".into()],
            }),
        }
    }

    #[test]
    fn no_server_means_here_and_says_nothing() {
        let p = decide(&ask(Mode::Build, "add a page"), &facts(), &Sites::default());
        assert_eq!(p.site, Site::Here);
        assert!(p.why.is_empty());
    }

    #[test]
    fn an_unreachable_server_falls_back_here() {
        let mut s = server();
        s.server.as_mut().unwrap().reachable = false;
        let p = decide(&ask(Mode::Build, "add a page"), &facts(), &s);
        assert_eq!(p.site, Site::Here);
        assert!(p.why.contains("isn't answering"));
    }

    #[test]
    fn a_project_only_the_server_has_runs_there() {
        let mut f = facts();
        f.project_here = false;
        let p = decide(&ask(Mode::Build, "add a page"), &f, &server());
        assert_eq!(p.site, Site::Server("my-vps".into()));
    }

    #[test]
    fn a_project_the_server_lacks_runs_here() {
        let mut f = facts();
        f.remote = "git@github.com:acme/other.git".into();
        let p = decide(&ask(Mode::Build, "add a page"), &f, &server());
        assert_eq!(p.site, Site::Here);
        assert!(p.why.contains("doesn't have app"));
    }

    #[test]
    fn runs_where_the_agent_is_signed_in() {
        let mut f = facts();
        f.agent_here = false;
        let p = decide(&ask(Mode::Fix, "fix it"), &f, &server());
        assert_eq!(p.site, Site::Server("my-vps".into()));
        let mut s = server();
        s.server.as_mut().unwrap().engines = vec!["someone-else".into()];
        let p = decide(&ask(Mode::Fix, "fix it"), &facts(), &s);
        assert_eq!(p.site, Site::Here);
        assert_eq!(p.fallback, Some(Site::Server("my-vps".into())));
    }

    #[test]
    fn long_work_goes_to_the_server_short_work_stays() {
        let mut f = facts();
        let mut a = ask(Mode::Build, "migrate every screen to the new router");
        f.long = is_long(&a);
        assert!(f.long);
        let p = decide(&a, &f, &server());
        assert_eq!(p.site, Site::Server("my-vps".into()));
        assert_eq!(p.fallback, Some(Site::Here));

        a = ask(Mode::Ask, "what does main.rs do?");
        f.long = is_long(&a);
        let p = decide(&a, &f, &server());
        assert_eq!(p.site, Site::Here);
    }

    #[test]
    fn races_and_goals_are_long() {
        let mut a = ask(Mode::Build, "add a page");
        a.race = 3;
        assert!(is_long(&a));
        a.race = 0;
        a.goal = true;
        assert!(is_long(&a));
    }

    #[test]
    fn remotes_match_however_theyre_spelled() {
        assert!(same_project(
            "git@github.com:acme/app.git",
            "https://github.com/acme/app"
        ));
        assert!(same_project(
            "https://github.com/Acme/App/",
            "ssh://git@github.com/acme/app.git"
        ));
        assert!(!same_project("", ""));
        assert!(!same_project("github.com/acme/app", "github.com/acme/api"));
    }
}
