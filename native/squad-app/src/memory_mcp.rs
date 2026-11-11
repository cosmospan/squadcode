//! `SquadCode --mcp-memory <folder>`: a small MCP server (JSON-RPC over stdio) that lets any
//! agent read the session memory, search earlier sessions of the project and save notes,
//! instead of having the whole history pasted into every prompt. It also offers the
//! playbooks: as tools, and as MCP prompts that CLIs like Claude Code show as slash commands.

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

/// Serve the tools for the session in `dir`. `thread` is the session's id when it is one
/// of Crew's threads, which adds the tools threads report through.
pub fn serve(dir: &str, thread: Option<u64>) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) = handle_thread(dir, thread, &request) {
            let _ = writeln!(out, "{response}");
            let _ = out.flush();
        }
    }
}

/// The reply to one request from the session in `dir`, a Crew thread when `thread` is set.
pub fn handle_thread(dir: &str, thread: Option<u64>, request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let params = &request["params"];
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => {
            if let Some(name) = params["clientInfo"]["name"].as_str() {
                let _ = CLIENT.set(name.to_string());
            }
            Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "squadcode", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Squad Code's tools for this job, and a browser. To try a web page (such as the app you're building on localhost), use browser_open and the other browser_ tools: they drive this job's own headless browser inside Squad Code, where the person can watch, and read pages as short text outlines; don't open an outside browser or install Playwright for it. For design work, design_theme gives a starting theme from a mood and design_check runs design checks (contrast, tap targets, overflow, values off the theme) on the page in that browser. To show the person a screenshot, mockup or diagram, save it as a file and call show_picture; before designing new screens, show two to four directions with show_directions."
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let mut all = tools();
            if crate::designdraw::agentops::offered(dir) {
                if let Some(list) = all.as_array_mut() {
                    list.extend(crate::designdraw::agentops::tools());
                }
            }
            if let Some(list) = all.as_array_mut() {
                match thread {
                    Some(_) => list.extend(thread_tools()),
                    // A job shows pictures and directions in its own feed.
                    None => list.extend(crate::v2::shown::list()),
                }
            }
            if crate::browse::tools::offered() {
                if let Some(list) = all.as_array_mut() {
                    list.extend(crate::browse::tools::list());
                }
            }
            Ok(json!({ "tools": all }))
        }
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            match thread.filter(|_| THREAD_TOOLS.contains(&name)) {
                Some(thread) => Ok(thread_call(
                    &crate::crew::events_dir(),
                    thread,
                    name,
                    &params["arguments"],
                    &MESSAGES_SENT,
                )),
                None if thread.is_none() && crate::v2::shown::TOOLS.contains(&name) => {
                    Ok(crate::v2::shown::call(dir, name, &params["arguments"]))
                }
                None if crate::browse::tools::TOOLS.contains(&name) => {
                    Ok(crate::browse::tools::call(dir, name, &params["arguments"]))
                }
                None => Ok(call(dir, name, &params["arguments"])),
            }
        }
        method => Err(json!({ "code": -32601, "message": format!("Unknown method {method}") })),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    })
}

fn tools() -> Value {
    json!([
        {
            "name": crate::approvals::TOOL,
            "description": "Used by Claude Code to ask the person, in Squad Code, for permission to use a tool. Don't call it yourself.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tool_name": { "type": "string" },
                    "input": { "type": "object" },
                    "tool_use_id": { "type": "string" }
                },
                "required": ["tool_name", "input"]
            }
        },
        crate::browse::design::theme_tool()
    ])
}

/// Tools only Crew's threads get; every other session's list is unchanged.
pub(crate) const THREAD_TOOLS: [&str; 4] =
    ["set_checklist", "report", "ask_decision", "message_crew"];

/// Messages a thread may send Crew from one run of this server, which lasts one turn in
/// Chat. They queue and reach Crew in order; more would let two agents keep each other busy.
const MESSAGES_PER_RUN: usize = 5;
/// What this run already sent Crew, so an exact repeat isn't sent twice.
static MESSAGES_SENT: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn thread_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "set_checklist",
            "description": "Show the person your plan for this task as a checklist, and keep it current: call it again with the whole list whenever a step starts or finishes. At most 20 short steps.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "A name for your task in 2 to 5 plain words, such as \"Settings spacing pass\". Give it with your first checklist."
                    },
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "text": { "type": "string" },
                                "state": { "type": "string", "enum": ["todo", "doing", "done"] }
                            },
                            "required": ["text", "state"]
                        }
                    }
                },
                "required": ["steps"]
            }
        }),
        json!({
            "name": "report",
            "description": "Report a result to the person: what you did or found, in a few plain sentences, with links to anything they should open (a pull request, a file, a document). Call it when the task is done or you reach something they should know.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string" },
                    "links": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["summary"]
            }
        }),
        json!({
            "name": "ask_decision",
            "description": "Ask the person to choose, when the choice changes what they get or can't be undone. Give one question, two to four options with what happens if each is picked, and the index of the one you recommend. Keep working on the recommended option unless it can't be undone; their answer comes back as your next message.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": { "type": "string" },
                                "consequence": { "type": "string" }
                            },
                            "required": ["label"]
                        }
                    },
                    "recommended": { "type": "integer" }
                },
                "required": ["question", "options", "recommended"]
            }
        }),
        json!({
            "name": "message_crew",
            "description": "Send a short message to Crew, which runs this project's threads: for example to ask another thread for something your work depends on. A few per turn at most; they reach Crew in order.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }
        }),
    ]
}

/// A thread tool's call: checked, then left for the app as an event in `events`.
fn thread_call(
    events: &Path,
    thread: u64,
    name: &str,
    args: &Value,
    sent: &std::sync::Mutex<Vec<String>>,
) -> Value {
    use crate::crew::{check_decision, clean_steps, Decision, EventKind, Mark, Step};
    let kind = match name {
        "set_checklist" => {
            let Some(items) = args["steps"].as_array() else {
                return failure("Give steps: a list of {text, state}.");
            };
            let steps = items
                .iter()
                .map(|item| Step {
                    text: item["text"].as_str().unwrap_or("").to_string(),
                    mark: match item["state"].as_str() {
                        Some("done") => Mark::Done,
                        Some("doing") => Mark::Doing,
                        _ => Mark::Todo,
                    },
                })
                .collect();
            let steps = clean_steps(steps);
            if steps.is_empty() {
                return failure("The checklist has no steps.");
            }
            let title = args["title"]
                .as_str()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string);
            EventKind::Checklist { steps, title }
        }
        "report" => {
            let summary = args["summary"].as_str().unwrap_or("").trim();
            if summary.is_empty() {
                return failure("The report is empty.");
            }
            let links = args["links"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            EventKind::Report {
                summary: summary.to_string(),
                links,
            }
        }
        "ask_decision" => {
            let decision = match serde_json::from_value::<Decision>(args.clone()) {
                Ok(decision) => decision,
                Err(_) => {
                    return failure(
                        "Give a question, options as a list of {label, consequence}, and recommended as an index.",
                    )
                }
            };
            match check_decision(decision) {
                Ok(decision) => EventKind::Decision(decision),
                Err(why) => return failure(why),
            }
        }
        "message_crew" => {
            let text = args["text"].as_str().unwrap_or("").trim();
            if text.is_empty() {
                return failure("The message is empty.");
            }
            let mut sent = sent.lock().unwrap_or_else(|e| e.into_inner());
            if sent.iter().any(|t| t == text) {
                return self::text("Already sent to Crew.");
            }
            if sent.len() >= MESSAGES_PER_RUN {
                return failure(format!(
                    "You already sent Crew {MESSAGES_PER_RUN} messages this turn. Put anything else in your report."
                ));
            }
            sent.push(text.to_string());
            EventKind::Message {
                text: text.to_string(),
            }
        }
        other => return failure(format!("Unknown tool {other}")),
    };
    match crate::crew::send_event(events, &crate::crew::event(thread, kind)) {
        Ok(()) => text(match name {
            "ask_decision" => "Asked. Keep working on the recommended option unless it can't be undone; the answer comes as your next message.",
            "message_crew" => "Sent to Crew.",
            _ => "Shown to the person.",
        }),
        Err(e) => failure(format!("Squad Code could not take this: {e}")),
    }
}

fn text(t: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }] })
}

fn failure(t: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }], "isError": true })
}

fn call(dir: &str, name: &str, args: &Value) -> Value {
    match name {
        crate::approvals::TOOL => crate::approvals::call(
            &crate::approvals::folder(),
            dir,
            args,
            crate::approvals::wait(),
        ),
        crate::browse::design::THEME => crate::browse::design::call_theme(args),
        design if design.starts_with("design_") => crate::designdraw::agentops::call(
            &crate::designdraw::agentops::folder(),
            dir,
            design,
            args,
        ),
        other => failure(format!("Unknown tool {other}")),
    }
}

/// Squad Code's saved sessions, read without changing anything.
/// The name of the CLI this server talks to, from its `initialize` request.
static CLIENT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
