//! `squad do` and `squad brain …`: the intelligence layer from a terminal.

use super::super::data::Member;
use super::super::store::{self, Store};
use super::{helper, ideas, route, tasks, voice};

/// Where `squad do <words>` sends a job, as the person's engine reads it.
/// Err when the engine can't decide, or reads it as something other than work.
pub fn route_do(text: &str) -> Result<route::Route, String> {
    let _ = Store::load(); // knows the person's crew and made bots
    let st = crate::settings::AppSettings::load();
    match decide_here(&st, text)? {
        (super::front::Front::Hand(r), _) | (super::front::Front::Confirm { route: r, .. }, _) => {
            Ok(r)
        }
        (_, say) => Err(format!(
            "That doesn't read as a job. {say} To start one anyway: squad new --bot <name> <words...>"
        )
        .trim()
        .to_string()),
    }
}

/// What the person's engine decides about `text`, for this folder.
fn decide_here(
    st: &crate::settings::AppSettings,
    text: &str,
) -> Result<(super::front::Front, String), String> {
    let f = here();
    let crew = Member::all();
    let seen = super::think::Seen {
        project: &f.name,
        recent: &[],
        crew: &crew,
        running: &[],
        worked: &[],
        in_thread: &[],
        server: "",
    };
    super::think::decide(st, &f.path, text, &seen)
}

fn here() -> store::Folder {
    let dir = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    store::Folder {
        name: std::path::Path::new(&dir)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: dir,
        ship: String::new(),
    }
}

/// `squad brain why|may|ideas|install|update|remove`.
pub fn main(args: &[String]) -> Result<String, String> {
    let sub = args.first().map(String::as_str).unwrap_or("");
    let rest = args.get(1..).unwrap_or(&[]).join(" ");
    let store = Store::load();
    let st = crate::settings::AppSettings::load();
    let crew = Member::all();
    match sub {
        // How a bot's step would be handled: `squad brain may npm install -g x`.
        "may" => {
            if rest.trim().is_empty() {
                return Err("usage: squad brain may <command...>".into());
            }
            let here = std::env::current_dir()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default();
            let j = super::permit::judge(
                "The bot",
                "Bash",
                &serde_json::json!({ "command": rest }),
                &here,
            );
            let tier = match j.tier {
                super::permit::Tier::Auto => "Just does it.",
                super::permit::Tier::Ask if j.can_always => "Asks: Allow once, Always allow, Deny.",
                super::permit::Tier::Ask => "Asks: Allow once, Deny.",
                super::permit::Tier::EveryTime => "Asks every time: Allow once, Deny.",
                super::permit::Tier::Never => "Refused.",
            };
            Ok(format!("{tier}\n{} {}", j.headline, j.why)
                .trim_end()
                .to_string())
        }
        "why" => {
            if rest.trim().is_empty() {
                return Err("usage: squad brain why <words...>".into());
            }
            // What Peek does with it, before any bot is picked.
            let (front, say) = decide_here(&st, &rest)?;
            let r = match front {
                super::front::Front::Chat { reply } => {
                    return Ok(format!("Peek replies, no job:\n{reply}"))
                }
                super::front::Front::AskBack { question, choices } => {
                    let mut out = format!("Peek asks first, no job yet:\n{question}");
                    if !choices.is_empty() {
                        out.push_str(&format!("\n[{}]", choices.join("] [")));
                    }
                    return Ok(out);
                }
                super::front::Front::Answer { code, .. } => {
                    return Ok(format!(
                        "{} answers it, read-only, with no job{}.",
                        super::front::answerer(&crew).name(),
                        if code { ", reading the project" } else { "" }
                    ))
                }
                super::front::Front::Confirm { why, route } => {
                    return Ok(format!(
                        "{} would take it as a {} job, after your yes.\n{why}",
                        route.bot.name(),
                        route.mode.label().to_lowercase()
                    ))
                }
                super::front::Front::Hand(r) => r,
                super::front::Front::Stop => {
                    return Ok("Stops what's running here, with no new job.".into())
                }
                super::front::Front::RunApp => {
                    return Ok(
                        "Starts the app in the terminal and keeps it up, with no job.".into(),
                    )
                }
            };
            let p = super::place(
                &st,
                &super::JobAsk {
                    bot: r.bot,
                    mode: r.mode,
                    text: rest.clone(),
                    folder: here(),
                    race: 0,
                    goal: false,
                },
                &super::Sites::default(),
            );
            let site = match &p.site {
                super::Site::Here => "this computer".to_string(),
                super::Site::Server(name) => name.clone(),
            };
            let out = format!(
                "Peek: {}\n{} \u{b7} {} job (decided by {})\nRuns on {site}, on {}. {}",
                if say.is_empty() {
                    r.handoff(super::route::Joins::New)
                } else {
                    format!("{say} {}", super::route::Joins::New.line(r.bot))
                },
                r.bot.name(),
                r.mode.label(),
                super::think::engine_name(&st),
                super::super::engine::label_of(p.engine.0, &p.engine.1),
                p.why
            );
            let mut out = out.trim_end().to_string();
            // Smart Peek's other jobs, when it's on.
            if let Some(t) = tasks::title(&st, &rest) {
                out.push_str(&format!("\nCard title: {t}"));
            }
            if let Some(q) = tasks::clarify(&st, &rest) {
                out.push_str(&format!("\nPeek would ask first: {q}"));
            }
            Ok(out)
        }
        "ideas" if rest.trim() == "make" => {
            let Some(i) = ideas::next(&store, &crew, &[], store::now()) else {
                return Err("No idea to make yet.".into());
            };
            let mut rt = super::super::runtime::Runtime::new(store);
            rt.make_idea(&st, i.key)
                .ok_or("That idea no longer fits.")?;
            Ok(format!(
                "{} joined the crew. It knows {}, with the standard rules for its kind of work.",
                i.name,
                i.words.join(", ")
            ))
        }
        "ideas" => Ok(match ideas::next(&store, &crew, &[], store::now()) {
            Some(i) => format!(
                "{}\n{}: {} (works like {}). Make it with: squad brain ideas make",
                voice::line(&voice::Moment::Idea {
                    name: i.name.to_string(),
                    why: i.why.clone(),
                }),
                i.name,
                i.role,
                i.like.name()
            ),
            None => {
                "No ideas yet. Peek suggests a bot once the same kind of job comes up a few times."
                    .into()
            }
        }),
        "install" | "update" => {
            // squad brain install [file.gguf | https://…]: Squad's own model,
            // then Smart Peek on. With nothing named, the latest release.
            // squad brain update: the latest release, if it's newer.
            let base = st.brain_model_url.trim().to_string();
            let msg = match args.get(1) {
                Some(from) if sub == "install" => {
                    helper::install_file(from)?;
                    format!("Installed {from}.")
                }
                _ => match helper::install_latest(&base, sub == "update")? {
                    Some(r) => format!(
                        "Installed Peek v{}{}.",
                        r.version,
                        if r.notes.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", r.notes)
                        }
                    ),
                    None => format!(
                        "Peek v{} is already the latest.",
                        helper::installed().map(|r| r.version).unwrap_or(0)
                    ),
                },
            };
            let mut st = st.clone();
            st.brain_model = helper::MODEL.to_string();
            st.brain_url.clear();
            st.save();
            Ok(format!(
                "{msg} Smart Peek is on, with Squad's own model at {}.",
                helper::model_path().display()
            ))
        }
        "remove" => {
            helper::remove();
            let mut st = st.clone();
            if helper::is_ours(&st.brain_model) {
                st.brain_model.clear();
            }
            st.save();
            Ok("Smart Peek is off and its model is deleted.".into())
        }
        _ => Err("usage: squad brain why|may|ideas|install|update|remove".into()),
    }
}
