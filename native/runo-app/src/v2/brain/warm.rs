//! Peek's engine kept warm, so reading a message takes a second or two.
//!
//! Starting Claude Code cold for every message costs most of the 6 seconds
//! a read takes. So Peek keeps one Claude Code process open per project, in
//! its stream mode (`--input-format stream-json`), and sends each message in
//! as a new turn. It's started before the first message, restarted every
//! [`TURNS`] reads so its conversation stays small, and dropped after a
//! failed read, so a late reply is never taken for the next message's. Other
//! engines, and any time the warm one can't take a message, read cold.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::agent::Event;

/// Reads before the warm engine starts afresh.
pub const TURNS: u32 = 12;
/// The least time between tries to start an engine that wouldn't start.
const RETRY: Duration = Duration::from_secs(60);

struct Warm {
    key: String,
    child: Child,
    stdin: ChildStdin,
    lines: Arc<Mutex<Receiver<String>>>,
    /// End lines still to come from turns whose reply was already taken.
    owed: Arc<AtomicU32>,
    turns: u32,
}

impl Warm {
    fn stop(mut self) {
        let _ = self.child.kill();
        std::thread::spawn(move || {
            let _ = self.child.wait();
        });
    }
}

static WARM: Mutex<Option<Warm>> = Mutex::new(None);
/// The last engine that wouldn't start, and when.
static FAILED: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Whether `provider` can be kept warm.
pub fn can(provider: &str) -> bool {
    provider == "claude"
}

/// The command for a warm Claude Code on `model`, which reads turns on
/// stdin and changes nothing.
pub fn command(bin: &str, model: &str) -> Command {
    let mut c = Command::new(bin);
    c.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--permission-mode",
        "plan",
        // No tools: Peek only reads and decides, which keeps each read cheap.
        "--tools",
        "",
        // Peek's reads stay out of the person's own Claude Code sessions.
        "--no-session-persistence",
    ]);
    if !model.trim().is_empty() {
        c.args(["--model", model.trim()]);
    }
    c
}

/// One turn, as a line on the warm engine's stdin.
pub fn message(prompt: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": prompt},
    })
    .to_string()
        + "\n"
}

/// What a line the warm engine printed says: the turn's reply, its error,
/// or None for a line that isn't a turn's end.
pub fn result_of(line: &str) -> Option<Result<String, String>> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v["type"].as_str() != Some("result") {
        return None;
    }
    let text = v["result"].as_str().unwrap_or("").trim().to_string();
    let failed = v["is_error"].as_bool().unwrap_or(false)
        || v["subtype"].as_str().is_some_and(|s| s != "success");
    Some(if failed {
        Err(if text.is_empty() {
            "the engine couldn't answer".to_string()
        } else {
            text
        })
    } else {
        Ok(text)
    })
}

/// The words of a reply as soon as the engine has written them, which is a
/// few seconds before the turn's end line.
pub fn reply_of(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v["type"].as_str() != Some("assistant") || !v["parent_tool_use_id"].is_null() {
        return None;
    }
    let text: String = v["message"]["content"]
        .as_array()?
        .iter()
        .filter(|b| b["type"].as_str() == Some("text"))
        .filter_map(|b| b["text"].as_str())
        .collect();
    Some(text.trim().to_string()).filter(|t| !t.is_empty())
}

fn key(bin: &str, model: &str, dir: &str) -> String {
    format!("{bin}\n{model}\n{dir}")
}

fn spawn(k: &str, bin: &str, model: &str, dir: &str) -> Option<Warm> {
    if let Ok(f) = FAILED.lock() {
        if let Some((fk, at)) = f.as_ref() {
            if fk == k && at.elapsed() < RETRY {
                return None;
            }
        }
    }
    let mut c = command(bin, model);
    c.current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let started = c.spawn().ok().and_then(|mut child| {
        let stdin = child.stdin.take()?;
        let out = child.stdout.take()?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Some(Warm {
            key: k.to_string(),
            child,
            stdin,
            lines: Arc::new(Mutex::new(rx)),
            owed: Arc::new(AtomicU32::new(0)),
            turns: 0,
        })
    });
    if started.is_none() {
        if let Ok(mut f) = FAILED.lock() {
            *f = Some((k.to_string(), Instant::now()));
        }
    }
    started
}

/// Makes sure the warm engine for `dir` is running; false when it can't be.
fn ensure(slot: &mut Option<Warm>, bin: &str, model: &str, dir: &str) -> bool {
    let k = key(bin, model, dir);
    let alive = slot
        .as_mut()
        .is_some_and(|w| w.key == k && w.turns < TURNS && matches!(w.child.try_wait(), Ok(None)));
    if !alive {
        if let Some(old) = slot.take() {
            old.stop();
        }
        *slot = spawn(&k, bin, model, dir);
    }
    slot.is_some()
}

/// Starts the warm engine for `dir` ahead of the next message. Cheap when it's
/// already running.
pub fn prepare(bin: &str, model: &str, dir: &str) {
    if let Ok(mut g) = WARM.lock() {
        // Not while a read is under way: it holds the lines.
        let busy = g.as_ref().is_some_and(|w| w.lines.try_lock().is_err());
        if !busy {
            ensure(&mut g, bin, model, dir);
        }
    }
}

/// Counts off one end line owed by an earlier turn; false when none is.
fn take_owed(owed: &AtomicU32) -> bool {
    loop {
        let n = owed.load(Ordering::SeqCst);
        if n == 0 {
            return false;
        }
        if owed
            .compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return true;
        }
    }
}

/// Stops the warm engine whose key is `k`, if it's still the one running.
fn drop_if(k: &str) {
    if let Ok(mut g) = WARM.lock() {
        if g.as_ref().is_some_and(|w| w.key == k) {
            if let Some(w) = g.take() {
                w.stop();
            }
        }
    }
}

/// Sends `prompt` to the warm engine for `dir`; its reply comes back on the
/// receiver as `Event::Output(id, …)` within `limit`. None when the warm
/// engine can't take it now; read cold then.
pub fn ask(
    bin: &str,
    model: &str,
    dir: &str,
    prompt: &str,
    limit: Duration,
    id: String,
) -> Option<Receiver<Event>> {
    let mut g = WARM.lock().ok()?;
    if !ensure(&mut g, bin, model, dir) {
        return None;
    }
    let w = g.as_mut()?;
    // A read still under way holds the lines; this one reads cold.
    {
        let lines = w.lines.try_lock().ok()?;
        while let Ok(line) = lines.try_recv() {
            if result_of(&line).is_some() {
                take_owed(&w.owed);
            }
        }
    }
    let sent = w
        .stdin
        .write_all(message(prompt).as_bytes())
        .and_then(|_| w.stdin.flush());
    if sent.is_err() {
        if let Some(w) = g.take() {
            w.stop();
        }
        return None;
    }
    w.turns += 1;
    let lines = w.lines.clone();
    let owed = w.owed.clone();
    let k = w.key.clone();
    drop(g);
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let deadline = Instant::now() + limit;
        // The reply goes back as soon as it's written, a few seconds before
        // the turn's end line; the next read skips that line.
        let mut sent = false;
        let result = match lines.lock() {
            Ok(lines) => loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match lines.recv_timeout(left) {
                    Ok(line) => {
                        if let Some(r) = result_of(&line) {
                            // An earlier turn's end, not this one's.
                            if take_owed(&owed) {
                                continue;
                            }
                            break r;
                        }
                        if let Some(text) = reply_of(&line) {
                            let _ = tx.send(Event::Output(id.clone(), Ok(text.clone())));
                            sent = true;
                            owed.fetch_add(1, Ordering::SeqCst);
                            break Ok(text);
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        break Err(format!("timed out after {}s", limit.as_secs()))
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        break Err("the engine stopped".to_string())
                    }
                }
            },
            Err(_) => Err("the engine stopped".to_string()),
        };
        // A failed read leaves the engine mid-turn: start afresh next time.
        if result.is_err() {
            drop_if(&k);
        }
        if !sent {
            let _ = tx.send(Event::Output(id, result));
        }
    });
    Some(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_goes_in_as_one_line() {
        let m = message("hi \"there\"\nsecond line");
        assert!(m.ends_with('\n'));
        assert_eq!(m.matches('\n').count(), 1);
        let v: Value = serde_json::from_str(m.trim()).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["content"], "hi \"there\"\nsecond line");
    }

    #[test]
    fn only_a_turns_end_is_its_reply() {
        assert_eq!(result_of(r#"{"type":"system","subtype":"init"}"#), None);
        assert_eq!(result_of(r#"{"type":"assistant","message":{}}"#), None);
        assert_eq!(result_of("not json"), None);
        assert_eq!(
            result_of(
                r#"{"type":"result","subtype":"success","is_error":false,"result":" {\"do\":\"reply\"} "}"#
            ),
            Some(Ok("{\"do\":\"reply\"}".into()))
        );
        assert!(matches!(
            result_of(r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#),
            Some(Err(_))
        ));
    }

    #[test]
    fn a_reply_is_taken_as_soon_as_it_is_written() {
        let line = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"{\"do\":\"stop\"}"}]}}"#;
        assert_eq!(reply_of(line), Some("{\"do\":\"stop\"}".into()));
        let sub = r#"{"type":"assistant","parent_tool_use_id":"t1","message":{"content":[{"type":"text","text":"x"}]}}"#;
        assert_eq!(reply_of(sub), None);
        assert_eq!(reply_of(r#"{"type":"result","result":"x"}"#), None);
    }

    #[test]
    fn the_warm_command_reads_turns_and_changes_nothing() {
        let c = command("claude", "haiku");
        let args: Vec<String> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        for want in [
            "--input-format",
            "stream-json",
            "plan",
            "haiku",
            "--tools",
            "--no-session-persistence",
        ] {
            assert!(args.iter().any(|a| a == want), "{want}");
        }
    }

    /// A stand-in engine: a shell loop that answers each line with a result.
    #[cfg(unix)]
    #[test]
    fn a_warm_engine_answers_turn_after_turn() {
        let dir = std::env::temp_dir();
        let script = dir.join(format!("peek-warm-{}.sh", std::process::id()));
        std::fs::write(
            &script,
            "#!/bin/sh\nwhile read line; do echo '{\"type\":\"system\"}'; echo '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\"}'; done\n",
        )
        .unwrap();
        let _ = Command::new("chmod").arg("+x").arg(&script).status();
        let bin = script.to_string_lossy().into_owned();
        let d = dir.to_string_lossy().into_owned();
        for i in 0..3 {
            let rx = ask(
                &bin,
                "",
                &d,
                "hello",
                Duration::from_secs(10),
                format!("t{i}"),
            )
            .expect("warm");
            match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                Event::Output(id, Ok(r)) => {
                    assert_eq!(id, format!("t{i}"));
                    assert_eq!(r, "ok");
                }
                _ => panic!("no reply"),
            }
        }
        drop_if(&key(&bin, "", &d));
        let _ = std::fs::remove_file(script);
    }
}

/// Times real reads on the installed Claude Code: `cargo test real_reads -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn real_reads() {
    let dir = std::env::temp_dir().to_string_lossy().into_owned();
    prepare("claude", "haiku", &dir);
    std::thread::sleep(Duration::from_secs(4));
    for (i, text) in [
        "make a snake and ladder game and do it on the vps",
        "runthe app",
        "the export button does nothing on safari",
        "stp the server",
        "thanks",
    ]
    .iter()
    .enumerate()
    {
        let crew = crate::v2::data::Member::CREW.to_vec();
        let p = super::think::prompt(
            text,
            &super::think::Seen {
                project: "shop",
                recent: &[],
                crew: &crew,
                running: &[],
                worked: &[],
                in_thread: &[],
                server: "this computer, or the person's server vps (connected, with claude signed in). It doesn't have this project yet; Squad Code copies it there first, by itself. Squad Code picks where each job runs.",
            },
        );
        let at = Instant::now();
        let rx = ask(
            "claude",
            "haiku",
            &dir,
            &p,
            Duration::from_secs(40),
            format!("r{i}"),
        )
        .unwrap();
        let got = rx.recv_timeout(Duration::from_secs(45)).ok();
        let reply = match got {
            Some(Event::Output(_, r)) => format!("{r:?}"),
            _ => "nothing".into(),
        };
        println!("{:.1}s {text}: {reply}", at.elapsed().as_secs_f32());
        std::thread::sleep(Duration::from_secs(4));
    }
}
