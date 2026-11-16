//! Peek's scored eval: a fixed set of moments, each with the decisions that
//! are right for it, read by Peek's real engine and scored.
//!
//! The cases live in `evals/peek/` as TOML, so a new one is a few lines:
//! `front.toml` for a message the person sends ([`think`]: which bot gets
//! the work, when to ask back, when to make a bot, analysis to Guard, the
//! whole ask in a hand-off) and `steer.toml` for one bot's message to another
//! ([`talk`]: pass, redirect, drop or stop).
//!
//! Run it before and after any change to Peek's rules:
//!
//! ```text
//! cargo test -p squadcode peek_eval -- --ignored --nocapture
//! ```
//!
//! `ENGINE` (claude), `MODEL` (haiku), `RUNS` per case (3), `JOBS` at once (6),
//! `ONLY` (cases whose id holds it) and `PEEK_EVAL_OUT` (where the report goes,
//! `target/peek-eval`) change the run. It fails when a case that was right in
//! the committed baseline (`baseline-<engine>-<model>.json`) is now mostly
//! wrong. `UPDATE_BASELINE=1` writes the baseline from this run.
//!
//! The tests below the runner check the cases themselves, with no engine:
//! every case loads, every right outcome can be reached through Peek's own
//! parsing, and a wrong one scores wrong.

use std::collections::BTreeMap;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use super::front::Front;
use super::talk::{self, Moment, Steer};
use super::think::{self, Seen};
use crate::v2::data::{set_made, Made, Member};

const FRONT: &str = include_str!("../../../evals/peek/front.toml");
const STEER: &str = include_str!("../../../evals/peek/steer.toml");

/// The made bot some cases have on the crew, like one Peek made from a thread.
const STRIPE: u16 = 41;

#[derive(Deserialize)]
struct File {
    case: Vec<Case>,
}

/// One moment and the decisions that are right for it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Case {
    id: String,
    from: String,
    gap: String,
    crew: String,
    thread: String,
    // A message from the person.
    open: Vec<String>,
    said: Vec<[String; 2]>,
    picked: String,
    wrote: String,
    // A bot's message to another.
    asked: String,
    since: Vec<[String; 2]>,
    sender: String,
    to: String,
    says: String,
    hops: u32,
    last: bool,
    // What's right.
    ok: Vec<Want>,
    never: Vec<String>,
}

/// One right outcome.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Want {
    #[serde(rename = "do")]
    act: String,
    bot: Vec<String>,
    to: Vec<String>,
    start: Option<bool>,
    name: Vec<String>,
    task_has: Vec<Vec<String>>,
    title_not: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Suite {
    Front,
    Steer,
}

/// What Peek decided, in the cases' words.
#[derive(Clone, Debug, Default, PartialEq)]
struct Got {
    act: String,
    bot: String,
    start: Option<bool>,
    name: String,
    task: String,
    title: String,
}

impl Got {
    fn line(&self) -> String {
        let mut s = self.act.clone();
        for (k, v) in [("bot", &self.bot), ("name", &self.name)] {
            if !v.is_empty() {
                s += &format!(" {k}={v}");
            }
        }
        if let Some(start) = self.start {
            s += &format!(" start={start}");
        }
        if !self.title.is_empty() {
            s += &format!(" title={:?}", self.title);
        }
        if !self.task.is_empty() {
            let t: String = self.task.chars().take(140).collect();
            s += &format!(" task={t:?}");
        }
        s
    }
}

fn cases(suite: Suite) -> Vec<Case> {
    let (name, text) = match suite {
        Suite::Front => ("front.toml", FRONT),
        Suite::Steer => ("steer.toml", STEER),
    };
    toml::from_str::<File>(text)
        .unwrap_or_else(|e| panic!("evals/peek/{name}: {e}"))
        .case
}

fn all_cases() -> Vec<(Suite, Case)> {
    cases(Suite::Front)
        .into_iter()
        .map(|c| (Suite::Front, c))
        .chain(cases(Suite::Steer).into_iter().map(|c| (Suite::Steer, c)))
        .collect()
}

fn area(id: &str) -> &str {
    id.split('.').next().unwrap_or(id)
}

fn stripe() {
    set_made(&[Made {
        id: STRIPE,
        name: "Stripe".into(),
        role: "Payments: Stripe checkout, webhooks and billing bugs".into(),
        like: Member::Fixer,
        look: [0; 4],
        outfit: String::new(),
        skills: Vec::new(),
        rules: Vec::new(),
        project: String::new(),
    }]);
}

/// A bot by its name, any case; "you" and "Peek" are not bots.
fn bot(name: &str) -> Option<Member> {
    let name = name.trim();
    if name.eq_ignore_ascii_case("stripe") {
        return Some(Member::Made(STRIPE));
    }
    Member::CREW
        .into_iter()
        .find(|m| m.name().eq_ignore_ascii_case(name))
}

/// The made bots the app knows for this case: Stripe only when it's on the
/// case's crew, so a case without it can't hand work to it.
fn made_for(crew: &[Member]) {
    if crew.contains(&Member::Made(STRIPE)) {
        stripe();
    } else {
        set_made(&[]);
    }
}

fn crew(case: &Case, suite: Suite) -> Vec<Member> {
    let preset = match (case.crew.as_str(), suite) {
        ("", Suite::Front) => "all",
        ("", Suite::Steer) => "all+stripe",
        (c, _) => c,
    };
    let mut crew = Member::CREW.to_vec();
    match preset {
        "all" => {}
        "all+stripe" => crew.push(Member::Made(STRIPE)),
        "no-guard" => crew.retain(|m| *m != Member::Guard),
        "starter" => crew = crate::v2::data::STARTER_CREW.to_vec(),
        other => panic!("{}: no crew called {other:?}", case.id),
    }
    crew
}

fn pairs(said: &[[String; 2]]) -> Vec<(String, String)> {
    said.iter()
        .map(|[who, words]| (who.clone(), words.clone()))
        .collect()
}

/// What the engine reads for the case: the same prompt the app builds.
fn prompt(case: &Case, suite: Suite) -> String {
    let crew = crew(case, suite);
    made_for(&crew);
    match suite {
        Suite::Front => {
            let recent = pairs(&case.said);
            let open: Vec<Member> = case.open.iter().filter_map(|b| bot(b)).collect();
            think::prompt(
                &case.wrote,
                &Seen {
                    project: "shop",
                    thread: if case.thread.is_empty() {
                        "general"
                    } else {
                        &case.thread
                    },
                    summary: "",
                    recent: &recent,
                    crew: &crew,
                    running: &[],
                    in_thread: &open,
                    server: "",
                    picked: &case.picked,
                },
            )
        }
        Suite::Steer => {
            let since = pairs(&case.since);
            let risk = super::guard::risk(&case.says).unwrap_or_default();
            talk::prompt(&moment(case, &crew, &since, &risk))
        }
    }
}

fn moment<'a>(
    case: &'a Case,
    crew: &'a [Member],
    since: &'a [(String, String)],
    risk: &'a str,
) -> Moment<'a> {
    Moment {
        project: "shop",
        thread: if case.thread.is_empty() {
            "general"
        } else {
            &case.thread
        },
        summary: "",
        asked: &case.asked,
        since,
        from: bot(&case.sender).expect("sender"),
        to: bot(&case.to).expect("to"),
        said: &case.says,
        crew,
        hops: case.hops,
        last: case.last,
        risk,
    }
}

/// The engine's reply, read the way the app reads it.
fn read(reply: &str, case: &Case, suite: Suite) -> Got {
    let crew = crew(case, suite);
    made_for(&crew);
    let mut got = Got::default();
    match suite {
        Suite::Front => {
            // The whole ask, when the rules ask the engine to write one.
            if let (Some(a), Some(b)) = (reply.find('{'), reply.rfind('}')) {
                if let Some(Ok(v)) = reply.get(a..=b).map(serde_json::from_str::<Value>) {
                    got.task = v["task"].as_str().unwrap_or("").trim().to_string();
                    got.title = v["title"].as_str().unwrap_or("").trim().to_string();
                }
            }
            let Some((front, _)) = think::parse(reply, &case.wrote, &crew) else {
                got.act = "unparsed".into();
                return got;
            };
            let name = |m: Member| m.name().to_string();
            match front {
                Front::Chat { .. } => got.act = "reply".into(),
                Front::AskBack { .. } => got.act = "ask".into(),
                Front::Stop => got.act = "stop".into(),
                Front::RunApp => got.act = "run".into(),
                Front::Answer { by, .. } => {
                    got.act = "answer".into();
                    got.bot = by.map(name).unwrap_or_else(|| "Peek".into());
                }
                Front::Hand(r) => {
                    got.act = "hand".into();
                    got.bot = name(r.bot);
                }
                Front::Confirm { route, .. } => {
                    got.act = "confirm".into();
                    got.bot = name(route.bot);
                }
                Front::Make { name: n, start, .. } => {
                    got.act = "make".into();
                    got.name = n;
                    got.start = Some(start);
                }
            }
        }
        Suite::Steer => {
            let since = pairs(&case.since);
            let risk = super::guard::risk(&case.says).unwrap_or_default();
            match talk::parse(reply, &moment(case, &crew, &since, &risk)) {
                Some(Steer::Go { to, .. }) => {
                    got.act = "pass".into();
                    got.bot = to.name().to_string();
                }
                Some(Steer::Stop { .. }) => got.act = "stop".into(),
                Some(Steer::Drop) => got.act = "drop".into(),
                None => got.act = "unparsed".into(),
            }
        }
    }
    got
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Whether the decision is one of the right ones, and why not when it isn't.
fn score(got: &Got, case: &Case) -> Result<(), String> {
    if case.never.iter().any(|b| same(b, &got.bot)) {
        return Err(format!("{} must never get it", got.bot));
    }
    let mut why = Vec::new();
    for want in &case.ok {
        match fits(want, got) {
            Ok(()) => return Ok(()),
            Err(e) => why.push(e),
        }
    }
    Err(why.join("; or "))
}

fn fits(want: &Want, got: &Got) -> Result<(), String> {
    if !same(&want.act, &got.act) {
        return Err(format!("wanted {}", want.act));
    }
    let bots: Vec<&String> = want.bot.iter().chain(&want.to).collect();
    if !bots.is_empty() && !bots.iter().any(|b| same(b, &got.bot)) {
        return Err(format!(
            "wanted {} {}",
            want.act,
            bots.iter()
                .map(|b| b.as_str())
                .collect::<Vec<_>>()
                .join("/")
        ));
    }
    if let Some(start) = want.start {
        if got.start != Some(start) {
            return Err(format!("wanted make with start={start}"));
        }
    }
    if !want.name.is_empty() && !want.name.iter().any(|n| same(n, &got.name)) {
        return Err(format!("wanted make named {}", want.name.join("/")));
    }
    let task = got.task.to_lowercase();
    for group in &want.task_has {
        if !group.iter().any(|w| task.contains(&w.to_lowercase())) {
            return Err(format!("the whole ask doesn't name {}", group.join("/")));
        }
    }
    if want.title_not.iter().any(|t| same(t, &got.title))
        || got.title.is_empty() && !want.title_not.is_empty()
    {
        return Err(format!("the title is {:?}", got.title));
    }
    Ok(())
}

/// One case's runs, as the report keeps them.
struct Row {
    id: String,
    gap: String,
    right: usize,
    runs: usize,
    errors: usize,
    wrong: Vec<(String, String)>,
}

/// The committed scores a run is held to.
fn baseline_path(engine: &str, model: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("evals/peek")
        .join(format!("baseline-{engine}-{model}.json"))
}

/// Peek's real decisions on every case, scored:
/// `cargo test -p squadcode peek_eval -- --ignored --nocapture`.
#[test]
#[ignore = "runs Peek's real engine: cargo test -p squadcode peek_eval -- --ignored --nocapture"]
fn peek_eval() {
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let engine = env("ENGINE", "claude");
    let model = env("MODEL", "haiku");
    let runs: usize = env("RUNS", "3").parse().expect("RUNS");
    let jobs: usize = env("JOBS", "6").parse().expect("JOBS");
    let only = env("ONLY", "");
    let out = std::path::PathBuf::from(env(
        "PEEK_EVAL_OUT",
        &format!("{}/../../target/peek-eval", env!("CARGO_MANIFEST_DIR")),
    ));

    let _g = crate::v2::data::tests::CREW_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let picked: Vec<(Suite, Case)> = all_cases()
        .into_iter()
        .filter(|(_, c)| c.id.contains(only.as_str()))
        .collect();
    assert!(!picked.is_empty(), "no case id holds {only:?}");
    let prompts: Vec<String> = picked.iter().map(|(s, c)| prompt(c, *s)).collect();
    println!(
        "Peek eval: {} cases x {runs} runs on {engine} {model}, {jobs} at once",
        picked.len()
    );

    // Every run of every case, a few at once.
    let work: Vec<(usize, usize)> = (0..picked.len())
        .flat_map(|i| (0..runs).map(move |r| (i, r)))
        .collect();
    let queue = Arc::new(Mutex::new(work.into_iter()));
    let replies = Arc::new(Mutex::new(vec![Vec::new(); picked.len()]));
    let dir = std::env::temp_dir().to_string_lossy().into_owned();
    let workers: Vec<_> = (0..jobs.max(1))
        .map(|_| {
            let (queue, replies, prompts) = (queue.clone(), replies.clone(), prompts.clone());
            let (engine, model, dir) = (engine.clone(), model.clone(), dir.clone());
            std::thread::spawn(move || loop {
                let Some((i, _)) = queue.lock().unwrap().next() else {
                    break;
                };
                let command = crate::agent::ask_command_on(&engine, "", &prompts[i], &model);
                let (tx, rx) = channel();
                crate::agent::oneshot_within(command, &dir, think::LIMIT * 2, tx);
                let reply = match rx.recv_timeout(think::LIMIT * 2 + Duration::from_secs(10)) {
                    Ok(crate::agent::Event::Output(Ok(r))) if !r.trim().is_empty() => {
                        Ok(crate::agent::strip_ansi(&r))
                    }
                    Ok(crate::agent::Event::Output(Ok(_))) => Err("empty reply".to_string()),
                    Ok(crate::agent::Event::Output(Err(e))) => Err(e),
                    _ => Err("no answer".to_string()),
                };
                replies.lock().unwrap()[i].push(reply);
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let replies = std::mem::take(&mut *replies.lock().unwrap());

    let mut rows = Vec::new();
    for ((suite, case), replies) in picked.iter().zip(replies) {
        let mut row = Row {
            id: case.id.clone(),
            gap: case.gap.clone(),
            right: 0,
            runs: 0,
            errors: 0,
            wrong: Vec::new(),
        };
        for reply in replies {
            let Ok(reply) = reply else {
                row.errors += 1;
                if let Err(e) = reply {
                    row.wrong
                        .push(("engine error".into(), e.chars().take(200).collect()));
                }
                continue;
            };
            row.runs += 1;
            let got = read(&reply, case, *suite);
            match score(&got, case) {
                Ok(()) => row.right += 1,
                Err(why) => row.wrong.push((got.line(), why)),
            }
        }
        rows.push(row);
    }
    set_made(&[]);

    // The report.
    let errors: usize = rows.iter().map(|r| r.errors).sum();
    let attempted = rows.iter().map(|r| r.runs + r.errors).sum::<usize>();
    let right: usize = rows.iter().map(|r| r.right).sum();
    let ran: usize = rows.iter().map(|r| r.runs).sum();
    let pct = |a: usize, b: usize| {
        if b == 0 {
            0.0
        } else {
            100.0 * a as f64 / b as f64
        }
    };
    let mut by_area: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for r in &rows {
        let e = by_area.entry(area(&r.id)).or_default();
        e.0 += r.right;
        e.1 += r.runs;
    }
    let base: Option<Value> = std::fs::read_to_string(baseline_path(&engine, &model))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let was = |id: &str| -> Option<(u64, u64)> {
        let v = &base.as_ref()?["cases"][id];
        Some((v[0].as_u64()?, v[1].as_u64()?))
    };
    // A case that was right nearly every time, now wrong more often than not.
    let regressed: Vec<&Row> = rows
        .iter()
        .filter(|r| r.runs > 0)
        .filter(|r| match was(&r.id) {
            Some((a, b)) if b > 0 => {
                a as f64 / b as f64 >= 0.8 && (r.right as f64) / (r.runs as f64) < 0.5
            }
            _ => false,
        })
        .collect();

    let mut md = format!(
        "# Peek eval\n\n**{right} / {ran} right ({:.1}%)** on {engine} {model}, {} cases x {runs} runs.",
        pct(right, ran),
        rows.len()
    );
    if let Some(b) = &base {
        let (a, n) = b["cases"]
            .as_object()
            .map(|cs| {
                cs.iter()
                    .filter(|(id, _)| rows.iter().any(|r| &r.id == *id))
                    .fold((0, 0), |(a, n), (_, v)| {
                        (
                            a + v[0].as_u64().unwrap_or(0),
                            n + v[1].as_u64().unwrap_or(0),
                        )
                    })
            })
            .unwrap_or((0, 0));
        md += &format!(
            " Baseline on the same cases: {a} / {n} ({:.1}%).",
            pct(a as usize, n as usize)
        );
    }
    if errors > 0 {
        md += &format!(" {errors} runs failed in the engine and aren't scored.");
    }
    md += "\n\n| Area | Right | Score |\n|---|---|---|\n";
    for (a, (r, n)) in &by_area {
        md += &format!("| {a} | {r} / {n} | {:.0}% |\n", pct(*r, *n));
    }
    if !regressed.is_empty() {
        md += "\n## Regressions\n\nRight in the baseline, wrong more often than not now:\n\n";
        for r in &regressed {
            let (a, b) = was(&r.id).unwrap_or_default();
            md += &format!(
                "- `{}`: {} / {} now, {a} / {b} before\n",
                r.id, r.right, r.runs
            );
        }
    }
    let misses: Vec<&Row> = rows
        .iter()
        .filter(|r| r.right < r.runs || r.errors > 0)
        .collect();
    if !misses.is_empty() {
        md += "\n## Not always right\n\n";
        for r in misses {
            md += &format!("- `{}`: {} / {}", r.id, r.right, r.runs);
            if !r.gap.is_empty() {
                md += &format!(" (known gap: {})", r.gap);
            }
            md += "\n";
            for (got, why) in r.wrong.iter().take(3) {
                md += &format!("  - got {got}: {why}\n");
            }
        }
    }
    println!("\n{md}");
    std::fs::create_dir_all(&out).expect("report folder");
    std::fs::write(out.join("report.md"), &md).expect("report");
    let cases: serde_json::Map<String, Value> = rows
        .iter()
        .map(|r| (r.id.clone(), json!([r.right, r.runs])))
        .collect();
    let results = json!({ "engine": engine, "model": model, "runs": runs, "right": right, "ran": ran, "cases": cases });
    std::fs::write(
        out.join("results.json"),
        serde_json::to_string_pretty(&results).unwrap(),
    )
    .expect("results");
    println!("Report: {}", out.join("report.md").display());

    if std::env::var("UPDATE_BASELINE").is_ok_and(|v| v == "1") {
        assert!(
            only.is_empty(),
            "write the baseline from a run of every case"
        );
        let path = baseline_path(&engine, &model);
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&results).unwrap() + "\n",
        )
        .expect("baseline");
        println!("Baseline written: {}", path.display());
    }
    assert!(
        attempted > 0 && errors * 10 <= attempted,
        "{errors} of {attempted} runs failed in the engine; is {engine} installed and signed in?"
    );
    assert!(
        regressed.is_empty(),
        "{} cases regressed: {}",
        regressed.len(),
        regressed
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRONT_ACTS: [&str; 8] = [
        "reply", "ask", "stop", "run", "answer", "hand", "confirm", "make",
    ];
    const STEER_ACTS: [&str; 3] = ["pass", "drop", "stop"];

    #[test]
    fn every_case_is_well_formed() {
        let _g = crate::v2::data::tests::CREW_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let all = all_cases();
        assert!(all.len() >= 60, "{} cases", all.len());
        let mut ids = std::collections::HashSet::new();
        for (suite, c) in &all {
            assert!(ids.insert(c.id.clone()), "{} is there twice", c.id);
            assert!(c.id.contains('.'), "{}: name it area.what", c.id);
            assert!(!c.from.is_empty(), "{}: say where it came from", c.id);
            assert!(!c.ok.is_empty(), "{}: no right outcome", c.id);
            let acts: &[&str] = match suite {
                Suite::Front => &FRONT_ACTS,
                Suite::Steer => &STEER_ACTS,
            };
            for w in &c.ok {
                assert!(acts.contains(&w.act.as_str()), "{}: no {:?}", c.id, w.act);
                for b in w.bot.iter().chain(&w.to).chain(&c.never) {
                    assert!(b == "Peek" || bot(b).is_some(), "{}: no bot {b}", c.id);
                }
            }
            match suite {
                Suite::Front => {
                    assert!(!c.wrote.trim().is_empty(), "{}", c.id);
                    for b in &c.open {
                        assert!(bot(b).is_some(), "{}: no bot {b}", c.id);
                    }
                }
                Suite::Steer => {
                    assert!(!c.says.trim().is_empty() && !c.asked.is_empty(), "{}", c.id);
                    assert!(bot(&c.sender).is_some(), "{}: sender", c.id);
                    assert!(bot(&c.to).is_some(), "{}: to", c.id);
                }
            }
            let p = prompt(c, *suite);
            let words = if *suite == Suite::Front {
                &c.wrote
            } else {
                &c.says
            };
            let start: String = words.chars().take(20).collect();
            assert!(
                p.contains(start.replace('"', "'").as_str()),
                "{}: the prompt doesn't carry the message",
                c.id
            );
        }
        set_made(&[]);
    }

    /// A reply that makes the engine's decision `want`, as the engine writes it.
    fn reply_for(want: &Want, case: &Case, suite: Suite) -> String {
        let bot = want
            .bot
            .iter()
            .chain(&want.to)
            .next()
            .cloned()
            .unwrap_or_default();
        let task: Vec<String> = want.task_has.iter().map(|g| g[0].clone()).collect();
        let v = match (suite, want.act.as_str()) {
            (Suite::Steer, act) => json!({
                "do": act, "to": bot, "note": "", "say": "Here's where the bots got to.", "choices": []
            }),
            // A risky ask reaches the person as a yes-or-no, whoever decided.
            (Suite::Front, "confirm") => json!({
                "do": "hand", "say": "On it.", "bot": if bot.is_empty() { "Builder".to_string() } else { bot }, "kind": "build"
            }),
            (Suite::Front, "answer") => json!({
                "do": "answer", "say": "", "bot": if bot == "Peek" { String::new() } else { bot }
            }),
            (Suite::Front, "make") => json!({
                "do": "make", "say": "Want me to make it?",
                "name": want.name.first().cloned().unwrap_or_else(|| "Quill".into()),
                "for": "the work in this case", "kind": "build",
                "start": want.start.unwrap_or(false)
            }),
            (Suite::Front, act) => json!({
                "do": act, "say": "Sounds good.", "choices": [], "bot": bot, "kind": "",
                "task": task.join(", "), "title": if want.title_not.is_empty() { "" } else { "Do the whole ask" }
            }),
        };
        let _ = case;
        format!("Sure.\n{v}")
    }

    #[test]
    fn every_right_outcome_can_be_reached_through_peeks_parsing() {
        let _g = crate::v2::data::tests::CREW_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (suite, c) in all_cases() {
            for want in &c.ok {
                let reply = reply_for(want, &c, suite);
                let got = read(&reply, &c, suite);
                assert_eq!(
                    score(&got, &c),
                    Ok(()),
                    "{}: {reply} read as {}",
                    c.id,
                    got.line()
                );
            }
        }
        set_made(&[]);
    }

    #[test]
    fn a_wrong_decision_scores_wrong() {
        let _g = crate::v2::data::tests::CREW_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let find = |id: &str| {
            all_cases()
                .into_iter()
                .find(|(_, c)| c.id == id)
                .unwrap_or_else(|| panic!("{id}"))
        };
        // Analysis handed to Builder: the #optimisation bug.
        let (s, c) = find("analysis.optimisation-all-together");
        let got = read(
            r#"{"do":"hand","say":"On it.","bot":"Builder","kind":"build"}"#,
            &c,
            s,
        );
        assert_eq!(got.bot, "Builder");
        assert!(score(&got, &c).is_err());
        // Guard, but only "all togther" as the ask.
        let got = read(
            r#"{"do":"hand","say":"On it.","bot":"Guard","kind":"review","task":"all togther","title":"all togther"}"#,
            &c,
            s,
        );
        let why = score(&got, &c).unwrap_err();
        assert!(why.contains("main-develop"), "{why}");
        // A redirect that should have been a stop.
        let (s, c) = find("steer.risky-deploy-nobody-asked-for");
        let got = read(r#"{"do":"pass","to":"Shipper","note":""}"#, &c, s);
        assert!(score(&got, &c).is_err());
        // A reply that isn't a decision.
        let (s, c) = find("talk.small-talk");
        let got = read("I'm Peek!", &c, s);
        assert_eq!(got.act, "unparsed");
        assert!(score(&got, &c).is_err());
        // Making a bot for work the crew does.
        let (s, c) = find("make.not-for-work-the-crew-does");
        let got = read(
            r#"{"do":"make","say":"Want a typo bot?","name":"Typo","for":"typos","kind":"fix"}"#,
            &c,
            s,
        );
        assert!(score(&got, &c).is_err());
        set_made(&[]);
    }
}
