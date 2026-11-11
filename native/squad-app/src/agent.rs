//! Launches provider CLIs and shell commands, streaming their output as events.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::settings::AgentOptions;

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub bin: &'static str,
    pub mark: &'static str,
    pub color: u32,
}

/// Every agent Squad Code can run, from the manifest (see `registry`): the built-in ones in
/// a fixed order, then any the person added.
pub static PROVIDERS: Providers = Providers;

pub struct Providers;

impl std::ops::Deref for Providers {
    type Target = [Provider];
    fn deref(&self) -> &[Provider] {
        crate::registry::providers()
    }
}

/// Whether a fact of the manifest holds for the installed CLI.
fn holds(
    provider: &str,
    fact: impl Fn(&crate::registry::AgentSpec) -> &crate::registry::Gate,
) -> bool {
    crate::registry::spec(provider).is_some_and(|s| fact(s).holds(provider))
}

/// Whether the CLI can resume the conversation Squad Code kept for a session. Grok (xAI's
/// Grok Build) and Antigravity both resume only by UUID; a session from the community
/// Grok CLI Squad Code ran before keeps a shorter id, so it starts a new conversation with the
/// history in the prompt instead.
pub fn can_resume(provider: &str, conversation: &str) -> bool {
    !matches!(provider, "grok" | "antigravity") || conversation.is_empty() || is_uuid(conversation)
}

fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// `text` without terminal colour codes (`ESC [ ... letter`).
pub(crate) fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// CLIs that take a conversation id chosen by Squad Code on the first run. Copilot and Gemini
/// do once their help lists `--session-id`; otherwise the id comes from their output.
pub fn names_conversation(provider: &str) -> bool {
    holds(provider, |s| &s.names_conversation)
}

pub fn provider(id: &str) -> &'static Provider {
    PROVIDERS
        .iter()
        .find(|p| p.id == id)
        .unwrap_or(&PROVIDERS[0])
}

/// Whether the agent can run: its configured executable exists, or with none
/// configured, its default command is on PATH.
pub fn available(bin: &str, configured: &str) -> bool {
    let configured = configured.trim();
    if configured.is_empty() {
        installed(bin)
    } else if configured.contains(std::path::MAIN_SEPARATOR) {
        std::path::Path::new(configured).is_file()
    } else {
        installed(configured)
    }
}

pub fn installed(bin: &str) -> bool {
    let suffixes: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                suffixes
                    .iter()
                    .any(|s| dir.join(format!("{bin}{s}")).is_file())
            })
        })
        .unwrap_or(false)
}

/// Shell-style splitting with single/double quotes, for the "extra arguments" setting.
pub fn split_args(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut quote, mut any) = (String::new(), None::<char>, false);
    for c in input.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

pub struct RunSpec<'a> {
    pub provider: &'a str,
    pub prompt: &'a str,
    pub resume: bool,
    /// The session's selections: model, effort, tier, mode, approval, agent.
    pub sel: &'a std::collections::HashMap<String, String>,
    pub opts: &'a AgentOptions,
    /// Whether the working directory is a git repository.
    pub in_git: bool,
    /// Claude Code conversation id for this session; empty until it has one.
    pub session_id: &'a str,
    /// Image files attached to the prompt.
    pub images: &'a [String],
    /// Squad Code's memory server for this folder: (Squad Code's executable, the session folder).
    pub memory: Option<(&'a str, &'a str)>,
    /// The run is on this machine, so MCP servers that are local programs can start.
    pub local: bool,
    /// The session's id when it is a Crew thread, whose memory server adds the tools
    /// threads report through; 0 otherwise.
    pub thread: u64,
    /// Squad Code may trust the folder for CLIs that ask (Cursor, Gemini) and approve its MCP
    /// servers up front: the person trusts the project's agent settings, or the folder
    /// holds none that start programs (see `folder_trusted`).
    pub trusted: bool,
}

/// A random version-4 UUID, for naming a Claude Code conversation up front.
pub fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = [0u8; 16];
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        hasher.write_usize(i);
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// How a CLI reports a run: Claude Code's stream-json (which Amp and Qwen Code also
/// speak), its own JSON lines, Pi's events, Antigravity's NDJSON, or plain text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Output {
    Claude,
    Json,
    Pi,
    Antigravity,
    Text,
}

/// The output format Squad Code asks the CLI for. Formats only newer releases have are used
/// once the installed CLI's help lists them.
pub fn output(provider: &str) -> Output {
    match crate::registry::spec(provider).map(|s| s.format()) {
        Some("claude") => Output::Claude,
        Some("json") => Output::Json,
        Some("pi") => Output::Pi,
        Some("antigravity") => Output::Antigravity,
        _ => Output::Text,
    }
}

/// Squad Code's permission levels a CLI can hold to, given its `--help`. Amp has no read-only
/// mode; Cursor's print mode either only proposes changes or, with `--force`, also runs
/// commands, so it has no edit-only level.
pub fn modes(provider: &str) -> Vec<&'static str> {
    match crate::registry::spec(provider) {
        Some(spec) => spec.modes.iter().map(String::as_str).collect(),
        None => vec!["edit", "plan", "full"],
    }
}

/// The level a CLI really runs at for the session's mode, for agents that have no exact
/// match: one without Auto edit (Cursor, whose edits come with commands) runs at Full
/// access.
pub fn effective_mode<'a>(provider: &str, mode: &'a str) -> &'a str {
    match mode {
        "" | "edit" if !modes(provider).contains(&"edit") => "full",
        _ => mode,
    }
}

/// Why the agent can't run in this mode, when it has no way to stay read-only.
pub fn mode_error(provider: &str, mode: &str) -> Option<String> {
    if mode != "plan" || modes(provider).contains(&"plan") {
        return None;
    }
    Some(format!(
        "{} has no read-only mode, so Squad Code won't run it in Plan only. Switch to Auto edit or Full access, or plan with another agent.",
        self::provider(provider).name
    ))
}

/// MCP servers for one run: the ones the person shares, then Cove's and Squad Code's memory
/// server when the run is on this machine.
fn mcp_servers(spec: &RunSpec) -> Vec<(String, crate::mcp::McpSpec)> {
    use crate::mcp::McpSpec;
    let mut out = crate::mcp::servers_for(spec.provider, spec.local, &spec.opts.bot_mcp);
    // A bot's own servers; a program can only start when the run is on this machine.
    for (name, server) in &spec.opts.bot_servers {
        if (spec.local || server.command.is_empty()) && !out.iter().any(|(n, _)| n == name) {
            out.push((name.clone(), server.clone()));
        }
    }
    if let Some((exe, dir)) = spec.memory {
        if let Some(cove) = cove_server() {
            out.push((
                "Cove".into(),
                McpSpec {
                    command: cove,
                    args: vec!["mcp".into()],
                    ..Default::default()
                },
            ));
        }
        let mut args = if spec.thread == 0 {
            vec!["--mcp-memory".into(), dir.to_string()]
        } else {
            vec![
                "--mcp-memory".into(),
                dir.to_string(),
                "--thread".into(),
                spec.thread.to_string(),
            ]
        };
        // So it can tell the person's answers from ones anyone else left in the folder.
        // A connection's relay answers in Squad Code itself, which has the key.
        if let Some(file) = spec.local.then(crate::approvals::key_file).flatten() {
            args.extend([crate::approvals::KEY_FILE_ARG.into(), file]);
        }
        out.push((
            MEMORY_SERVER.into(),
            McpSpec {
                command: exe.to_string(),
                args,
                ..Default::default()
            },
        ));
    }
    out
}

/// Squad Code's own tools an agent may call without being asked: the job's own
/// headless browser, inside Squad (nothing outside it changes), and a thread's reports
/// back.
fn unasked_tools(spec: &RunSpec) -> Vec<&'static str> {
    if spec.memory.is_none() {
        return Vec::new();
    }
    let mut tools: Vec<&'static str> = crate::browse::tools::TOOLS.to_vec();
    tools.push(crate::browse::design::THEME);
    if spec.thread != 0 {
        tools.extend(crate::memory_mcp::THREAD_TOOLS);
    } else {
        // A job's pictures and directions: they only show in its own feed.
        tools.extend(crate::v2::shown::TOOLS);
    }
    tools
}

/// Whether `provider` takes MCP servers for a run: by flag, project file or ACP.
/// Grok reads them only from a trusted folder, and Pi and Antigravity's print mode
/// have no way in, so those use Squad's tools as shell commands instead.
pub fn takes_mcp(provider: &str) -> bool {
    !matches!(provider, "grok" | "pi" | "antigravity") || acp_args(provider).is_some()
}

/// MCP servers for a CLI that reads them from a project file. That file is in the
/// session folder, which a run elsewhere sees too, so only servers reached by URL go
/// along there.
fn file_servers(spec: &RunSpec) -> Vec<(String, crate::mcp::McpSpec)> {
    if crate::mcp::project_file(spec.provider).is_none() {
        return Vec::new();
    }
    mcp_servers(spec)
        .into_iter()
        .filter(|(_, server)| spec.local || server.command.is_empty())
        .collect()
}

/// Environment for one run, for CLIs that take settings that way.
fn provider_env(spec: &RunSpec) -> Vec<(String, String)> {
    let mut env = Vec::new();
    // The person's own environment lines come after these and win.
    if spec.provider == "claude" && !spec.opts.subagent_model.trim().is_empty() {
        env.push((
            "CLAUDE_CODE_SUBAGENT_MODEL".into(),
            spec.opts.subagent_model.trim().to_string(),
        ));
    }
    if spec.provider == "opencode" {
        let servers: serde_json::Map<String, serde_json::Value> = mcp_servers(spec)
            .into_iter()
            .map(|(name, server)| (name, server.opencode_json()))
            .collect();
        if !servers.is_empty() {
            let config = serde_json::json!({ "mcp": servers });
            env.push(("OPENCODE_CONFIG_CONTENT".into(), config.to_string()));
        }
    }
    env
}

fn provider_args(spec: &RunSpec) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let opts = spec.opts;
    let get = |id: &str| spec.sel.get(id).cloned().unwrap_or_default();
    let or = |a: String, b: &str| if a.is_empty() { b.to_string() } else { a };
    let model = or(get("model"), &opts.model_override);
    let effort = or(get("effort"), &opts.effort);
    let mode = get("mode");
    let mode = effective_mode(spec.provider, &mode).to_string();
    let knows = |flag: &str| crate::caps::mentions(spec.provider, flag);
    let mut args: Vec<String> = Vec::new();
    // An agent Squad Code knows only from the manifest: its print command, if it gave one.
    if let Some(agent) = crate::registry::spec(spec.provider).filter(|a| a.driver == "acp") {
        let print = agent.print.clone().unwrap_or_default();
        args.extend(print.args);
        if let (Some(flag), false) = (print.model, model.is_empty()) {
            args.extend([flag, model]);
        }
        args.extend(split_args(&opts.extra_args));
        args.push(with_images(spec));
        return args;
    }
    match spec.provider {
        "pi" => {
            args.extend([s("--mode"), s("json")]);
            args.extend(pi_args(spec, &model, &effort, &mode));
            args.push(prompt_arg(spec.prompt));
        }
        "codex" => {
            args.extend([s("exec"), s("--json")]);
            // `-i` takes several files; the flag after it ends the list, so the prompt is
            // never read as an image.
            if !spec.resume {
                for image in spec.images {
                    args.extend([s("-i"), image.clone()]);
                }
            }
            let sandbox = match mode.as_str() {
                "" | "edit" => "workspace-write".to_string(),
                "plan" => s("read-only"),
                "full" => s("danger-full-access"),
                other => other.to_string(),
            };
            args.extend([s("--sandbox"), sandbox]);
            if !model.is_empty() {
                args.extend([s("-m"), model]);
            }
            if !effort.is_empty() {
                args.extend([s("-c"), format!("model_reasoning_effort=\"{effort}\"")]);
            }
            let tier = get("tier");
            if !tier.is_empty() {
                args.extend([s("-c"), format!("service_tier=\"{tier}\"")]);
            }
            if !opts.profile.is_empty() {
                args.extend([s("-p"), opts.profile.clone()]);
            }
            for dir in list(&opts.add_dirs) {
                args.extend([s("--add-dir"), dir]);
            }
            if !spec.in_git {
                args.push(s("--skip-git-repo-check"));
            }
            for (name, server) in mcp_servers(spec) {
                for over in server.codex_overrides(&name) {
                    args.extend([s("-c"), over]);
                }
            }
            // `exec` can't ask, so a tool call that would ask is cancelled instead.
            for tool in unasked_tools(spec) {
                args.extend([
                    s("-c"),
                    format!("mcp_servers.{MEMORY_SERVER}.tools.{tool}.approval_mode=\"approve\""),
                ]);
            }
            args.extend(split_args(&opts.extra_args));
            if spec.resume {
                // A known conversation id resumes exactly that one; `--last` is the
                // newest in the folder, which may belong to another session.
                if spec.session_id.is_empty() {
                    args.extend([s("resume"), s("--last")]);
                } else {
                    args.extend([s("resume"), spec.session_id.to_string()]);
                }
                // `resume` takes one file per `-i`.
                for image in spec.images {
                    args.extend([s("-i"), image.clone()]);
                }
            }
            args.push(prompt_arg(spec.prompt));
        }
        "cursor" => {
            args.extend([s("-p"), s("--output-format"), s("stream-json")]);
            // Without trust, Cursor stops at its trust prompt rather than load settings
            // the repository brought.
            if spec.trusted {
                args.extend(cursor_trust());
            }
            if spec.resume && !spec.session_id.is_empty() {
                args.extend([s("--resume"), spec.session_id.to_string()]);
            }
            // Plan only runs in Cursor's plan mode once the CLI has one; before that,
            // print mode without `--force` only proposes changes.
            if mode == "plan" {
                args.extend(cursor_mode("plan"));
            } else {
                args.push(s("--force"));
            }
            // Servers from the project file wait for approval unless approved up front,
            // which happens only when they are all Squad Code's or the person trusts the repo.
            if spec.trusted && knows("--approve-mcps") && !file_servers(spec).is_empty() {
                args.push(s("--approve-mcps"));
            }
            if !model.is_empty() {
                args.extend([s("-m"), model]);
            }
            args.extend(split_args(&opts.extra_args));
            args.push(with_images(spec));
        }
        "gemini" | "qwen" => {
            let qwen = spec.provider == "qwen";
            let approval = match mode.as_str() {
                "plan" if knows("\"plan\"") => "plan",
                "plan" => "default",
                "full" => "yolo",
                // Qwen Code spells it with a hyphen.
                _ if qwen && knows("auto-edit") => "auto-edit",
                _ => "auto_edit",
            };
            args.extend([s("--approval-mode"), s(approval)]);
            if output(spec.provider) != Output::Text {
                args.extend([s("--output-format"), s("stream-json")]);
            }
            if !model.is_empty() {
                args.extend([s("-m"), model]);
            }
            for dir in list(&opts.add_dirs) {
                args.extend([s("--include-directories"), dir]);
            }
            args.extend(conversation_args(spec));
            if qwen && knows("--mcp-config") {
                let servers: serde_json::Map<String, serde_json::Value> = mcp_servers(spec)
                    .into_iter()
                    .map(|(name, server)| (name, server.claude_json()))
                    .collect();
                if !servers.is_empty() {
                    let config = serde_json::json!({ "mcpServers": servers });
                    args.extend([s("--mcp-config"), config.to_string()]);
                }
            }
            args.extend(split_args(&opts.extra_args));
            args.extend([s("-p"), with_images(spec)]);
        }
        "copilot" => {
            // Non-interactive runs need tools allowed up front; a denial outranks it, so
            // Auto edit keeps the shell off and Plan only also keeps file writes off.
            args.push(s("--allow-all-tools"));
            match mode.as_str() {
                "full" => {}
                "plan" => args.extend([s("--deny-tool"), s("shell"), s("--deny-tool"), s("write")]),
                _ => args.extend([s("--deny-tool"), s("shell")]),
            }
            if output("copilot") == Output::Json {
                args.extend([s("--output-format"), s("json")]);
            }
            if !model.is_empty() {
                args.extend([s("--model"), model]);
            }
            for dir in list(&opts.add_dirs) {
                args.extend([s("--add-dir"), dir]);
            }
            if spec.resume && !spec.session_id.is_empty() {
                args.extend([s("--resume"), spec.session_id.to_string()]);
            } else if !spec.resume && names_conversation("copilot") && !spec.session_id.is_empty() {
                args.extend([s("--session-id"), spec.session_id.to_string()]);
            }
            if knows("--additional-mcp-config") {
                let servers: serde_json::Map<String, serde_json::Value> = mcp_servers(spec)
                    .into_iter()
                    .map(|(name, server)| (name, server.copilot_json()))
                    .collect();
                if !servers.is_empty() {
                    let config = serde_json::json!({ "mcpServers": servers });
                    args.extend([s("--additional-mcp-config"), config.to_string()]);
                }
            }
            args.extend(split_args(&opts.extra_args));
            if knows("--attachment") {
                for image in spec.images {
                    args.extend([s("--attachment"), image.clone()]);
                }
                args.extend([s("-p"), prompt_arg(spec.prompt)]);
            } else {
                args.extend([s("-p"), with_images(spec)]);
            }
        }
        "amp" => {
            if spec.resume && !spec.session_id.is_empty() {
                args.extend([s("threads"), s("continue"), spec.session_id.to_string()]);
            }
            // Amp edits files on its own and asks before most commands, which execute mode
            // turns into a refusal; Full access allows everything.
            if mode == "full" {
                args.push(s("--dangerously-allow-all"));
            }
            args.push(s("--stream-json"));
            let servers: serde_json::Map<String, serde_json::Value> = mcp_servers(spec)
                .into_iter()
                .map(|(name, server)| (name, server.amp_json()))
                .collect();
            if !servers.is_empty() {
                args.extend([
                    s("--mcp-config"),
                    serde_json::Value::Object(servers).to_string(),
                ]);
            }
            args.extend(split_args(&opts.extra_args));
            args.extend([s("-x"), with_images(spec)]);
        }
        "droid" => {
            args.extend([s("exec"), s("-o"), s("stream-json")]);
            if spec.resume && !spec.session_id.is_empty() {
                args.extend([s("-s"), spec.session_id.to_string()]);
            }
            // Droid is read-only unless given an autonomy level.
            match mode.as_str() {
                "plan" => {}
                "full" => args.extend([s("--auto"), s("high")]),
                _ => args.extend([s("--auto"), s("medium")]),
            }
            if !model.is_empty() {
                args.extend([s("-m"), model]);
            }
            args.extend(split_args(&opts.extra_args));
            args.push(with_images(spec));
        }
        // Google's Antigravity CLI prints its own NDJSON stream (see `AntigravityStream`).
        // Resume takes a UUID; a headless run can't ask, so Full access skips every prompt
        // and Auto edit only accepts edits.
        "antigravity" => {
            args.extend([s("-p"), s("--output-format"), s("stream-json")]);
            match mode.as_str() {
                "plan" => args.extend([s("--mode"), s("plan")]),
                "full" => args.push(s("--dangerously-skip-permissions")),
                _ => args.extend([s("--mode"), s("accept-edits")]),
            }
            if !model.is_empty() {
                args.extend([s("--model"), model]);
            }
            if !effort.is_empty() {
                args.extend([s("--effort"), effort]);
            }
            let id = spec.session_id;
            match (spec.resume, id.is_empty()) {
                (true, false) if is_uuid(id) => args.extend([s("--conversation"), id.to_string()]),
                (true, true) => args.push(s("-c")),
                _ => {}
            }
            // A fresh or stale-id resume takes its workspace directories up front; a
            // resumed session keeps the folders it was created with.
            if !spec.resume || !is_uuid(id) {
                for dir in list(&opts.add_dirs) {
                    args.extend([s("--add-dir"), dir]);
                }
            }
            args.extend(split_args(&opts.extra_args));
            args.push(with_images(spec));
        }
        // xAI's Grok Build speaks Claude Code's stream-json and permission modes. A
        // headless run can't ask, so a tool call its mode doesn't allow is refused.
        "grok" => {
            let permission = match mode.as_str() {
                "plan" => "plan",
                "full" => "bypassPermissions",
                _ => "acceptEdits",
            };
            args.extend([s("--output-format"), s("streaming-messages-json")]);
            if knows("--include-partial-messages") {
                args.push(s("--include-partial-messages"));
            }
            args.extend([s("--permission-mode"), s(permission)]);
            if !model.is_empty() {
                args.extend([s("-m"), model]);
            }
            if !effort.is_empty() {
                args.extend([s("--reasoning-effort"), effort]);
            }
            if !opts.append_prompt.trim().is_empty() {
                args.extend([s("--rules"), opts.append_prompt.clone()]);
            }
            // It takes Claude Code's `Bash(git *)` style rules.
            for rule in list(&opts.allowed_tools) {
                args.extend([s("--allow"), rule]);
            }
            for rule in list(&opts.disallowed_tools) {
                args.extend([s("--deny"), rule]);
            }
            let id = spec.session_id;
            match (spec.resume, id.is_empty()) {
                // A resumed session keeps the folders it was created with.
                (true, false) if is_uuid(id) => args.extend([s("-r"), id.to_string()]),
                (true, true) => args.push(s("-c")),
                _ => {
                    // `-s` takes only a UUID, for a new session.
                    if is_uuid(id) {
                        args.extend([s("-s"), id.to_string()]);
                    }
                    for dir in list(&opts.add_dirs) {
                        args.extend([s("--add-dir"), dir]);
                    }
                }
            }
            args.extend(split_args(&opts.extra_args));
            args.extend([s("-p"), with_images(spec)]);
        }
        "opencode" => {
            args.extend([s("run"), s("--format"), s("json")]);
            if spec.resume {
                // A known id resumes exactly this session's conversation; `--continue` is the
                // newest in the folder, which may belong to another session.
                if spec.session_id.is_empty() {
                    args.push(s("--continue"));
                } else {
                    args.extend([s("-s"), spec.session_id.to_string()]);
                }
            }
            if !model.is_empty() {
                args.extend([s("-m"), model]);
            }
            // Plan only always runs OpenCode's read-only plan agent, whatever agent the
            // session or Settings picked.
            let agent = if mode == "plan" {
                Some(s("plan"))
            } else if !get("agent").is_empty() {
                Some(get("agent"))
            } else if !opts.sub_agent.is_empty() {
                Some(opts.sub_agent.clone())
            } else {
                None
            };
            if let Some(agent) = agent {
                args.extend([s("--agent"), agent]);
            }
            let variant = or(effort, &opts.variant);
            if !variant.is_empty() {
                args.extend([s("--variant"), variant]);
            }
            if opts.thinking {
                args.push(s("--thinking"));
            }
            if mode == "full" || mode == "auto" {
                args.push(s("--auto"));
            }
            args.extend(split_args(&opts.extra_args));
            // `-f` takes several files, so it goes after the prompt.
            args.push(prompt_arg(spec.prompt));
            for image in spec.images {
                args.extend([s("-f"), image.clone()]);
            }
        }
        _ => {
            let permission = match mode.as_str() {
                "" | "edit" => "acceptEdits",
                "full" => "bypassPermissions",
                other => other,
            };
            // Structured output lets the app show each tool step live instead of a wall of text.
            args.extend([
                s("-p"),
                s("--output-format"),
                s("stream-json"),
                s("--verbose"),
                s("--include-partial-messages"),
                s("--permission-mode"),
                s(permission),
            ]);
            if !model.is_empty() {
                args.extend([s("--model"), model]);
            }
            if !effort.is_empty() {
                args.extend([s("--effort"), effort]);
            }
            if !opts.append_prompt.trim().is_empty() {
                args.extend([s("--append-system-prompt"), opts.append_prompt.clone()]);
            }
            let mut allowed = list(&opts.allowed_tools);
            let servers: serde_json::Map<String, serde_json::Value> = mcp_servers(spec)
                .into_iter()
                .map(|(name, server)| (name, server.claude_json()))
                .collect();
            for tool in unasked_tools(spec) {
                allowed.push(format!("mcp__{MEMORY_SERVER}__{tool}"));
            }
            if !servers.is_empty() {
                let config = serde_json::json!({ "mcpServers": servers });
                args.extend([s("--mcp-config"), config.to_string()]);
            }
            // A tool it isn't allowed is asked about in Squad Code while the turn waits, instead
            // of being refused and shown only once the turn has ended.
            if spec.memory.is_some()
                && !matches!(permission, "bypassPermissions" | "plan" | "dontAsk")
            {
                args.extend([
                    s("--permission-prompt-tool"),
                    format!("mcp__{MEMORY_SERVER}__{}", crate::approvals::TOOL),
                ]);
            }
            if !allowed.is_empty() {
                args.extend([s("--allowed-tools"), allowed.join(",")]);
            }
            let denied = list(&opts.disallowed_tools);
            if !denied.is_empty() {
                args.extend([s("--disallowed-tools"), denied.join(",")]);
            }
            for dir in list(&opts.add_dirs) {
                args.extend([s("--add-dir"), dir]);
            }
            if !opts.fallback_model.is_empty() {
                args.extend([s("--fallback-model"), opts.fallback_model.clone()]);
            }
            args.extend(split_args(&opts.extra_args));
            match (spec.resume, spec.session_id.is_empty()) {
                // Pin the conversation to this session: `--continue` would pick up whatever
                // conversation ran last in the folder, which may belong to another session.
                (true, false) => args.extend([s("--resume"), spec.session_id.to_string()]),
                (false, false) => args.extend([s("--session-id"), spec.session_id.to_string()]),
                (true, true) => args.push(s("--continue")),
                (false, true) => {}
            }
            // Several Claude flags are variadic; `--` keeps them from swallowing the prompt.
            args.push(s("--"));
            args.push(with_images(spec));
        }
    }
    args
}

/// Pi's flags for a turn, in JSON or RPC mode: the conversation (named by Squad Code, which
/// Pi creates when it doesn't have it yet), the model and thinking level, and the tools
/// Squad Code's mode allows. Pi asks nothing before it acts, so Plan only gives it only tools
/// that read and Auto edit adds the ones that edit, but no shell.
fn pi_args(spec: &RunSpec, model: &str, effort: &str, mode: &str) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let mut args = Vec::new();
    if !spec.session_id.is_empty() {
        args.extend([s("--session-id"), spec.session_id.to_string()]);
    }
    if !model.is_empty() {
        args.extend([s("--model"), model.to_string()]);
    }
    if !effort.is_empty() {
        args.extend([s("--thinking"), effort.to_string()]);
    }
    match mode {
        "plan" => args.extend([s("--tools"), s("read,grep,find,ls")]),
        "full" => {}
        _ => args.extend([s("--tools"), s("read,edit,write,grep,find,ls")]),
    }
    if !spec.opts.append_prompt.trim().is_empty() {
        args.extend([s("--append-system-prompt"), spec.opts.append_prompt.clone()]);
    }
    args.extend(split_args(&spec.opts.extra_args));
    args
}

/// Gemini's and Qwen Code's conversation flags: `--resume <id>` to continue, or a new
/// conversation's id up front. Gemini names a new one only when it has `--session-id`.
fn conversation_args(spec: &RunSpec) -> Vec<String> {
    let id = spec.session_id;
    if id.is_empty() {
        return Vec::new();
    }
    let gemini = spec.provider == "gemini";
    if spec.resume {
        vec!["--resume".into(), id.to_string()]
    } else if !gemini || names_conversation("gemini") {
        vec!["--session-id".into(), id.to_string()]
    } else {
        Vec::new()
    }
}

/// Why Squad Code won't trust `dir` for `spec`'s CLI, when the person hasn't trusted the
/// project's agent settings and the folder has settings that start programs (from the
/// repository, a pull request, or a VM sharing the folder). None when it may. What an
/// earlier run added to the folder's settings is taken out first, so only the folder's
/// own are judged.
pub fn untrusted_config(spec: &RunSpec, dir: &str) -> Option<String> {
    if spec.trusted || crate::mcp::project_file(spec.provider).is_none() {
        return None;
    }
    crate::mcp::clean_project(std::path::Path::new(dir));
    let found = crate::mcp::foreign_config(
        std::path::Path::new(dir),
        spec.provider,
        &file_servers(spec),
    )?;
    let name = provider(spec.provider).name;
    crate::applog::warn_once(
        "agent",
        format!("not trusting {dir} for {name}: {found} starts programs Squad Code didn't add"),
    );
    Some(format!(
        "Squad Code didn't trust this folder for {name}, because {found} can start programs. \
         If this project's agent settings are yours, turn on \"Trust agent settings in this \
         repository\" in the project's settings."
    ))
}

/// Cursor's headless runs stop at a workspace trust prompt in a folder it hasn't seen,
/// such as a new worktree, unless the CLI is told to trust it.
pub(crate) fn cursor_trust() -> Vec<String> {
    if crate::caps::mentions("cursor", "--trust") {
        vec!["--trust".into()]
    } else {
        Vec::new()
    }
}

/// Cursor's `--mode plan` or `--mode ask`, once its help lists the flag (`--model` also
/// starts with `--mode`, so the flag is matched with its argument).
pub(crate) fn cursor_mode(mode: &str) -> Vec<String> {
    if crate::caps::mentions("cursor", "--mode <") {
        vec!["--mode".into(), mode.into()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod interactive_tests {
    use super::*;

    #[test]
    fn a_prompt_starting_with_a_dash_is_never_a_flag() {
        assert_eq!(
            prompt_arg("--dangerously-bypass-approvals-and-sandbox"),
            " --dangerously-bypass-approvals-and-sandbox"
        );
        assert_eq!(prompt_arg("fix the -x flag"), "fix the -x flag");
    }
}

/// Cove's MCP server for agents on this machine, when Cove runs here and the person
/// lets agents use it: scratch VMs, commands, files and ports of their own.
fn cove_server() -> Option<String> {
    (crate::settings::cove_tools() && crate::cove::available())
        .then(crate::cove::binary)
        .flatten()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Name of Squad Code's memory MCP server in the agents' config.
pub const MEMORY_SERVER: &str = "squadcode";

/// The prompt with attached image paths listed, for CLIs that read images from disk.
/// The prompt as a command-line argument. Most agent CLIs take it as a positional
/// argument without a `--` before it, so a prompt starting with `-` (a Crew message that
/// is exactly `--dangerously-bypass-approvals-and-sandbox`, say) would be read as a flag.
/// A leading space keeps it text and changes nothing for the agent.
fn prompt_arg(prompt: &str) -> String {
    if prompt.starts_with('-') {
        format!(" {prompt}")
    } else {
        prompt.to_string()
    }
}

fn with_images(spec: &RunSpec) -> String {
    if spec.images.is_empty() {
        return prompt_arg(spec.prompt);
    }
    let mut prompt = prompt_arg(spec.prompt);
    prompt.push_str("\n\nAttached images (read them):");
    for image in spec.images {
        prompt.push_str(&format!("\n- {image}"));
    }
    prompt
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Session(u64),
    Terminal(u64),
}

pub enum Event {
    Line(String),
    /// A line of an agent's output that is JSON, parsed on the thread that read it so the
    /// main thread only has to act on it.
    Json(String, serde_json::Value),
    /// stderr of an agent run: kept out of the conversation unless the run fails.
    Err(String),
    Done(bool),
    /// Result of a one-off background command.
    Output(Result<String, String>),
}

/// Run a command to completion on a background thread and send its stdout back.
pub fn oneshot(mut command: Command, dir: &str, tx: Sender<Event>) {
    command.current_dir(dir).stdin(Stdio::null());
    std::thread::spawn(move || {
        let result = match command.output() {
            Ok(out) if out.status.success() => {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                Err(if err.is_empty() {
                    String::from_utf8_lossy(&out.stdout).trim().to_string()
                } else {
                    err
                })
            }
            Err(error) => Err(format!(
                "could not start {:?}: {error}",
                command.get_program()
            )),
        };
        let _ = tx.send(Event::Output(result));
    });
}

/// Like `oneshot`, but the command is killed after `limit` and reported as an error.
pub fn oneshot_within(
    mut command: Command,
    dir: &str,
    limit: std::time::Duration,
    tx: Sender<Event>,
) {
    command
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    std::thread::spawn(move || {
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let program = command.get_program().to_owned();
                let _ = tx.send(Event::Output(Err(format!(
                    "could not start {program:?}: {error}"
                ))));
                return;
            }
        };
        let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_string(&mut text);
                }
                text
            })
        };
        let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
        let err = drain(child.stderr.take().map(|p| Box::new(p) as _));
        let deadline = std::time::Instant::now() + limit;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        // After a timeout a grandchild may still hold the pipes open, so they are not awaited.
        let result = match status {
            Some(status) => {
                let stdout = out.join().unwrap_or_default();
                let stderr = err.join().unwrap_or_default();
                if status.success() {
                    Ok(stdout.trim().to_string())
                } else if stderr.trim().is_empty() {
                    Err(stdout.trim().to_string())
                } else {
                    Err(stderr.trim().to_string())
                }
            }
            None => Err(format!("timed out after {}s", limit.as_secs())),
        };
        let _ = tx.send(Event::Output(result));
    });
}

/// What to do to sign an agent in, when its output says it isn't. Only lines from the
/// CLI itself are checked (its errors and stderr), never the agent's reply.
pub fn sign_in_hint(provider: &str, log: &str) -> Option<String> {
    if !log.lines().any(signs_out) {
        return None;
    }
    Some(sign_in_help(provider))
}

/// Whether one line of a CLI's error output says it isn't signed in. Broad words such as
/// "unauthorized" or "401" count only on the agent's own errors: an MCP server or a tool
/// refused for its own login doesn't sign the agent out.
fn signs_out(line: &str) -> bool {
    let lower = line.to_lowercase();
    let plain = [
        "not logged in",
        "please log in",
        "please login",
        "please run /login",
        "login required",
        "log in with",
        "not signed in",
        "please sign in",
        "not authenticated",
        "authentication required",
        "invalid api key",
        "no api key",
        "api key not found",
        "missing api key",
        "starting login flow",
        // Gemini, signed out.
        "set an auth method",
        "must specify the gemini_api_key",
    ];
    if plain.iter().any(|s| lower.contains(s)) {
        return true;
    }
    // Words any refused request uses: an MCP server's, a plugin's or a hook's as much as
    // the agent's own.
    let broad = [
        "authentication failed",
        "unauthorized",
        "token expired",
        "could not be validated",
        "status 401",
        "(401)",
        "code 401",
        // OpenCode, when its saved sign-in has expired.
        "token refresh failed",
        "providerautherror",
    ];
    let elsewhere = ["mcp", "plugin", "hook "].iter().any(|s| lower.contains(s));
    !elsewhere && broad.iter().any(|s| lower.contains(s))
}

/// The command that signs an agent in. Squad runs it itself, as a card you
/// can type to, never in your terminal.
pub fn sign_in_cmd(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "claude" => "claude",
        "codex" => "codex login",
        "gemini" => "gemini",
        "qwen" => "qwen",
        "copilot" => "copilot",
        "amp" => "amp login",
        "cursor" => "cursor-agent login",
        "droid" => "droid",
        "opencode" => "opencode auth login",
        "grok" => "grok login",
        "antigravity" => "agy",
        _ => return None,
    })
}

/// How to sign an agent in, as "Codex isn't signed in. Press Sign in …".
pub fn sign_in_help(provider: &str) -> String {
    let name = self::provider(provider).name;
    let key = match provider {
        "claude" => " Or set ANTHROPIC_API_KEY under Settings > Claude Code > Environment.",
        "codex" => " Or set OPENAI_API_KEY under Settings > Codex > Environment.",
        "gemini" => " Or set GEMINI_API_KEY under Settings > Gemini > Environment.",
        "qwen" => " Or set an API key under Settings > Qwen Code > Environment.",
        "copilot" => " Or set GH_TOKEN under Settings > Copilot > Environment.",
        "amp" => " Or set AMP_API_KEY under Settings > Amp > Environment.",
        "cursor" => " Or set CURSOR_API_KEY under Settings > Cursor > Environment.",
        "droid" => " Or set FACTORY_API_KEY under Settings > Droid > Environment.",
        "grok" => " Or set XAI_API_KEY under Settings > Grok > Environment.",
        "antigravity" => " Or set GEMINI_API_KEY under Settings > Antigravity > Environment.",
        _ => "",
    };
    match sign_in_cmd(provider) {
        Some(cmd) => {
            let then = if matches!(provider, "claude" | "copilot") {
                " and type /login"
            } else {
                ""
            };
            format!(
                "{name} isn't signed in. Press Sign in and Squad runs {cmd} for you, right here{then}; follow what it asks.{key}"
            )
        }
        None => format!("{name} isn't signed in. Sign in to it, then try again.{key}"),
    }
}

/// The flag an agent's one-shot ask takes a model with, if it takes one.
pub fn ask_model_flag(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" | "copilot" | "antigravity" => Some("--model"),
        "amp" => None,
        _ => Some("-m"),
    }
}

/// `ask_command` on `model`; empty leaves the agent on its own default.
pub fn ask_command_on(provider: &str, configured_bin: &str, prompt: &str, model: &str) -> Command {
    let bin = if configured_bin.trim().is_empty() {
        self::provider(provider).bin
    } else {
        configured_bin.trim()
    };
    let mut command = Command::new(bin);
    let model_args: Vec<&str> = match ask_model_flag(provider) {
        Some(flag) if !model.trim().is_empty() => vec![flag, model.trim()],
        _ => Vec::new(),
    };
    match provider {
        "codex" => {
            command.args(["exec", "--sandbox", "read-only", "--skip-git-repo-check"]);
            command.args(&model_args).arg(prompt);
        }
        "opencode" => {
            command.arg("run").args(&model_args).arg(prompt);
        }
        "cursor" => {
            command.args(["-p", "--output-format", "text"]);
            command.args(cursor_trust());
            command.args(cursor_mode("ask"));
            command.args(&model_args).arg(prompt);
        }
        "amp" => {
            command.args(["-x", prompt]);
        }
        "droid" => {
            command.arg("exec").args(&model_args).arg(prompt);
        }
        "claude" => {
            command.args(["-p", "--permission-mode", "plan"]);
            command.args(&model_args).args(["--", prompt]);
        }
        "grok" => {
            command.args(["--permission-mode", "plan"]);
            command.args(&model_args).args(["-p", prompt]);
        }
        "antigravity" => {
            command.args(["--mode", "plan"]);
            command.args(&model_args).args(["-p", prompt]);
        }
        _ => {
            command.args(&model_args).args(["-p", prompt]);
        }
    }
    command
}

#[derive(Clone)]
/// A running process, which leads its own process group so stopping it also stops what
/// it started (dev servers, test runs, the ssh or cove process of a connection run).
/// `runner` marks Squad Code's own connection runner.
pub struct Handle(Option<Arc<Mutex<Child>>>, bool, Option<u32>);

impl Handle {
    /// A local turn running detached from Squad Code in process group `group`.
    pub(crate) fn detached(group: u32) -> Self {
        Self(None, false, Some(group))
    }

    pub fn kill(&self) {
        if let Some(group) = self.2 {
            crate::detach::stop(group);
            return;
        }
        if let Some(Ok(mut child)) = self.0.as_ref().map(|c| c.lock()) {
            // A connection runner stops the turn on the connection and brings back what
            // it did, then exits on its own.
            if self.1 && cfg!(unix) {
                let _ = Command::new("kill")
                    .args(["-USR1", &child.id().to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                return;
            }
            #[cfg(unix)]
            {
                // Ask the whole group to stop, then make sure of it a moment later.
                let group = child.id() as i32;
                // SAFETY: plain signal sends to a process group this handle started.
                unsafe {
                    libc::kill(-group, libc::SIGTERM);
                }
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(2));
                    // SAFETY: as above; a group that already ended just returns an error.
                    unsafe {
                        libc::kill(-group, libc::SIGKILL);
                    }
                });
            }
            let _ = child.kill();
        }
    }
}

/// The program to run for a turn: the agent's Settings binary, else its usual name.
fn program<'a>(spec: &RunSpec<'a>) -> &'a str {
    if spec.opts.binary.trim().is_empty() {
        self::provider(spec.provider).bin
    } else {
        spec.opts.binary.trim()
    }
}

/// The command for one turn: the agent's program with the arguments and environment
/// its settings and the session's selections call for.
pub fn turn_command(spec: &RunSpec) -> Command {
    let mut command = Command::new(program(spec));
    command.args(provider_args(spec));
    command.envs(provider_env(spec));
    for line in spec.opts.env.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if !key.trim().is_empty() {
                command.env(key.trim(), value.trim());
            }
        }
    }
    command
}

/// What the folder needs before a turn: Gemini's trust, so it holds to the session's
/// mode, and MCP servers for CLIs that read them from a project file.
pub fn prepare_folder(spec: &RunSpec, dir: &str, sandboxed: bool) {
    if spec.provider == "gemini" && spec.local && !sandboxed && spec.trusted {
        crate::gemini::trust(dir, &spec.opts.env);
    }
    if crate::mcp::project_file(spec.provider).is_some() {
        crate::mcp::write_project(
            std::path::Path::new(dir),
            spec.provider,
            &file_servers(spec),
        );
    }
}

/// Whether Squad Code talks to the agent while it works: Claude Code takes its prompt, and
/// any message sent while it runs, on stdin (`--input-format stream-json`), and so does
/// Amp, in the same format, once its help lists `--stream-json-input`.
pub fn takes_live_input(provider: &str) -> bool {
    live_form(provider).is_some()
}

/// The manifest's way for the installed CLI to take messages on stdin.
fn live_form(provider: &str) -> Option<&'static crate::registry::Live> {
    crate::registry::spec(provider)?.live.iter().find(|l| {
        l.when
            .as_deref()
            .is_none_or(|w| crate::caps::mentions(provider, w))
    })
}

/// A turn's command made live: the prompt at the end of its arguments (after `--` for
/// Claude Code, `-x` for Amp) goes instead as the first message on stdin. Returns the
/// command and that prompt, or `None` when the arguments don't end that way.
pub fn live_command(provider: &str, command: &Command) -> Option<(Command, String)> {
    let args: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let [rest @ .., flag, prompt] = args.as_slice() else {
        return None;
    };
    // The form that matches how the command ends; whether the installed CLI takes
    // messages this way at all is `takes_live_input`.
    let form = crate::registry::spec(provider)?
        .live
        .iter()
        .find(|form| form.flag == *flag)?;
    let mut live = Command::new(command.get_program());
    live.args(rest).args(&form.args);
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => live.env(key, value),
            None => live.env_remove(key),
        };
    }
    Some((live, prompt.clone()))
}

/// The arguments that run `provider` as an ACP agent, once the installed CLI's help
/// lists them. `SQUADCODE_NO_ACP` set in Squad Code's environment keeps every agent in print mode.
pub fn acp_args(provider: &str) -> Option<Vec<String>> {
    if std::env::var_os("SQUADCODE_NO_ACP").is_some() {
        return None;
    }
    let spec = crate::registry::spec(provider)?;
    spec.pick(&spec.acp).map(<[String]>::to_vec)
}

/// An ACP turn's settings, with `fallback` (the print-mode command) to run if the agent
/// can't open a session. Returns them and the prompt, which goes as the first message.
pub fn acp_config(
    spec: &RunSpec,
    dir: &str,
    fallback: &Command,
) -> Option<(crate::harness::acp::Config, String)> {
    let args = acp_args(spec.provider)?;
    let get = |id: &str| spec.sel.get(id).cloned().unwrap_or_default();
    let model = match get("model") {
        m if m.is_empty() => spec.opts.model_override.clone(),
        m => m,
    };
    let argv = |c: &Command| -> Vec<String> {
        std::iter::once(c.get_program())
            .chain(c.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    };
    let mut command = vec![program(spec).to_string()];
    command.extend(args);
    let config = crate::harness::acp::Config {
        provider: spec.provider.to_string(),
        command,
        fallback: argv(fallback),
        cwd: dir.to_string(),
        session: if spec.resume {
            spec.session_id.to_string()
        } else {
            String::new()
        },
        model,
        mode: get("mode"),
        servers: mcp_servers(spec)
            .into_iter()
            .map(|(name, server)| server.acp_json(&name))
            .collect(),
    };
    Some((config, with_images(spec)))
}

/// A Codex turn's settings for its app server, once the installed Codex has one, with
/// `fallback` (the `exec` command) to run if it can't start. The `-c` settings of the
/// `exec` command (MCP servers, effort, service tier) go along.
pub fn codex_app_config(
    spec: &RunSpec,
    dir: &str,
    fallback: &Command,
) -> Option<crate::harness::codex::Config> {
    if spec.provider != "codex" || std::env::var_os("SQUADCODE_NO_APP_SERVER").is_some() {
        return None;
    }
    let agent = crate::registry::spec("codex")?;
    let server = agent.pick(&agent.rpc)?;
    let args: Vec<String> = fallback
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let mut command = vec![program(spec).to_string()];
    command.extend(server.iter().cloned());
    for pair in args.windows(2) {
        match pair[0].as_str() {
            "-c" => command.extend(["-c".to_string(), pair[1].clone()]),
            "-p" => command.extend(["-c".to_string(), format!("profile={:?}", pair[1])]),
            _ => {}
        }
    }
    let get = |id: &str| spec.sel.get(id).cloned().unwrap_or_default();
    let or = |a: String, b: &str| if a.is_empty() { b.to_string() } else { a };
    Some(crate::harness::codex::Config {
        command,
        fallback: std::iter::once(fallback.get_program().to_string_lossy().into_owned())
            .chain(args)
            .collect(),
        cwd: dir.to_string(),
        session: if spec.resume {
            spec.session_id.to_string()
        } else {
            String::new()
        },
        model: or(get("model"), &spec.opts.model_override),
        effort: or(get("effort"), &spec.opts.effort),
        mode: get("mode"),
    })
}

/// A Pi turn's settings for its RPC mode, once the installed Pi has it, with `fallback`
/// (the `--mode json` command) to run if it can't start. Returns them and the prompt,
/// which goes as the first message.
pub fn pi_config(
    spec: &RunSpec,
    dir: &str,
    fallback: &Command,
) -> Option<(crate::harness::pi::Config, String)> {
    let agent = crate::registry::spec(spec.provider).filter(|a| a.driver == "pi")?;
    if std::env::var_os("SQUADCODE_NO_RPC").is_some() {
        return None;
    }
    let rpc = agent.pick(&agent.rpc)?;
    let args: Vec<String> = fallback
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    // `--mode json … <prompt>` becomes `--mode rpc …`: the prompt goes as a message.
    let [rest @ .., _prompt] = args.as_slice() else {
        return None;
    };
    let mut command = vec![program(spec).to_string()];
    command.extend(rpc.iter().cloned());
    let mut flags = rest.iter();
    while let Some(arg) = flags.next() {
        if arg == "--mode" {
            flags.next();
        } else {
            command.push(arg.clone());
        }
    }
    let config = crate::harness::pi::Config {
        command,
        fallback: std::iter::once(fallback.get_program().to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .collect(),
        cwd: dir.to_string(),
    };
    Some((config, with_images(spec)))
}

/// Run an agent turn.
pub fn run_agent(
    target: Target,
    spec: &RunSpec,
    dir: &str,
    tx: Sender<Event>,
) -> Result<Handle, String> {
    let foreign = untrusted_config(spec, dir);
    if let Some(why) = &foreign {
        // Shown with the error when the CLI then stops at its trust prompt.
        let _ = tx.send(Event::Err(why.clone()));
    }
    let spec = &RunSpec {
        trusted: foreign.is_none(),
        ..*spec
    };
    let bin = program(spec);
    let command = turn_command(spec);
    let acp = match (target, crate::detach::root()) {
        (Target::Session(_), Some(_)) => acp_config(spec, dir, &command),
        _ => None,
    };
    if acp.is_some() {
        // The servers go in the session instead; nothing is merged into project files.
        if spec.provider == "gemini" && spec.trusted {
            crate::gemini::trust(dir, &spec.opts.env);
        }
        crate::mcp::clean_project(std::path::Path::new(dir));
    } else {
        prepare_folder(spec, dir, false);
    }
    if let (Target::Session(id), Some(root)) = (target, crate::detach::root()) {
        // Local turns outlive Squad Code, so closing or a crash doesn't stop them.
        if crate::updater::resolve(bin).is_some() {
            if let Some((config, prompt)) = acp {
                let setup = |turn: &std::path::Path| config.write(turn);
                return crate::detach::spawn_driver(
                    root, id, "--acp", setup, command, &prompt, dir, tx,
                );
            }
            if let Some((config, prompt)) = pi_config(spec, dir, &command) {
                let setup = |turn: &std::path::Path| config.write(turn);
                return crate::detach::spawn_driver(
                    root, id, "--pi", setup, command, &prompt, dir, tx,
                );
            }
            if let Some(config) = codex_app_config(spec, dir, &command) {
                let setup = |turn: &std::path::Path| config.write(turn);
                return crate::detach::spawn_driver(
                    root,
                    id,
                    "--codex",
                    setup,
                    command,
                    &with_images(spec),
                    dir,
                    tx,
                );
            }
            if takes_live_input(spec.provider) {
                if let Some((live, prompt)) = live_command(spec.provider, &command) {
                    return crate::detach::spawn_live(root, id, live, &prompt, dir, tx);
                }
            }
            return crate::detach::spawn(root, id, command, dir, tx);
        }
    }
    spawn(target, command, dir, tx)
}

/// Run a shell command.
#[cfg(test)]
pub fn run_shell(
    target: Target,
    line: &str,
    dir: &str,
    tx: Sender<Event>,
) -> Result<Handle, String> {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", line]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", line]);
        c
    };
    command.env("TERM", "dumb");
    spawn(target, command, dir, tx)
}

fn pump<R: Read + Send + 'static>(reader: R, target: Target, is_err: bool, tx: Sender<Event>) {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            let event = line_event(target, is_err, line);
            if tx.send(event).is_err() {
                break;
            }
        }
    });
}

/// The event for a line read from a run's stdout or stderr (`is_err`).
pub(crate) fn line_event(target: Target, is_err: bool, line: String) -> Event {
    match target {
        Target::Session(_) if is_err => Event::Err(line),
        Target::Session(_) => match crate::stream::parse_line(&line) {
            Some(json) => Event::Json(line, json),
            None => Event::Line(line),
        },
        Target::Terminal(_) => Event::Line(line),
    }
}

pub(crate) fn spawn(
    target: Target,
    mut command: Command,
    dir: &str,
    tx: Sender<Event>,
) -> Result<Handle, String> {
    crate::shellenv::ready();
    let runner = command
        .get_args()
        .next()
        .is_some_and(|a| a == "--connection-run");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start {:?}: {error}", command.get_program()))?;
    if let Some(out) = child.stdout.take() {
        pump(out, target, false, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        pump(err, target, true, tx.clone());
    }
    // Only the program's file name: its arguments carry the prompt.
    let program = std::path::Path::new(command.get_program())
        .file_name()
        .unwrap_or(command.get_program())
        .to_owned();
    let child = Arc::new(Mutex::new(child));
    let waiter = child.clone();
    std::thread::spawn(move || loop {
        let finished = waiter
            .lock()
            .ok()
            .and_then(|mut c| c.try_wait().ok().flatten());
        if let Some(status) = finished {
            // Let the reader threads flush the final lines first.
            std::thread::sleep(Duration::from_millis(150));
            if !status.success() {
                let program = program.to_string_lossy();
                crate::applog::warn(
                    "agent",
                    format!(
                        "{target:?}: {program} exited with {}",
                        match status.code() {
                            Some(code) => format!("code {code}"),
                            None => "no code (stopped by a signal)".into(),
                        }
                    ),
                );
            }
            let _ = tx.send(Event::Done(status.success()));
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    });
    Ok(Handle(Some(child), runner, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn bounded_oneshots_report_output_and_time_out() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut echo = Command::new("sh");
        echo.args(["-c", "echo recap"]);
        oneshot_within(echo, ".", Duration::from_secs(5), tx.clone());
        let mut slow = Command::new("sh");
        slow.args(["-c", "sleep 10"]);
        oneshot_within(slow, ".", Duration::from_millis(300), tx);
        // The quick one answers first; the slow one is stopped at its limit.
        let got: Vec<Result<String, String>> = (0..2)
            .map(|_| match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Output(result) => result,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(got[0], Ok("recap".into()));
        assert!(got[1].as_ref().unwrap_err().contains("timed out"));
    }

    #[test]
    fn an_agents_json_lines_arrive_parsed() {
        let session = Target::Session(1);
        let json = r#"{"type":"result"}"#.to_string();
        match line_event(session, false, json.clone()) {
            Event::Json(line, value) => {
                assert!(line == json);
                assert_eq!(value["type"], "result");
            }
            _ => panic!("not parsed"),
        }
        assert!(matches!(
            line_event(session, false, "plain words".into()),
            Event::Line(l) if l == "plain words"
        ));
        assert!(matches!(
            line_event(session, true, json.clone()),
            Event::Err(..)
        ));
        // A terminal's output is shown as it is.
        assert!(matches!(
            line_event(Target::Terminal(1), false, json),
            Event::Line(..)
        ));
    }

    #[test]
    fn shell_streams_lines_then_done() {
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = std::env::temp_dir();
        run_shell(
            Target::Terminal(1),
            "echo hi && echo err 1>&2",
            dir.to_str().unwrap(),
            tx,
        )
        .unwrap();
        let mut lines = vec![];
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Line(l) | Event::Json(l, _) | Event::Err(l) => lines.push(l),
                Event::Done(ok) => {
                    assert!(ok);
                    break;
                }
                Event::Output(..) => {}
            }
        }
        lines.sort();
        assert_eq!(lines, ["err", "hi"]);
    }
}

#[cfg(test)]
mod arg_tests {
    use super::*;
    use std::collections::HashMap;

    fn sel(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn args(
        provider: &str,
        resume: bool,
        selections: &[(&str, &str)],
        opts: &AgentOptions,
        in_git: bool,
    ) -> Vec<String> {
        let sel = sel(selections);
        provider_args(&RunSpec {
            provider,
            prompt: "hi",
            resume,
            sel: &sel,
            opts,
            in_git,
            session_id: "",
            images: &[],
            memory: None,
            local: true,
            thread: 0,
            trusted: false,
        })
    }

    #[test]
    fn the_subagent_model_goes_to_claude_only_when_set_and_the_persons_env_wins() {
        let sel = HashMap::new();
        let env_of = |provider, opts: &AgentOptions| {
            let spec = RunSpec {
                provider,
                prompt: "hi",
                resume: false,
                sel: &sel,
                opts,
                in_git: true,
                session_id: "",
                images: &[],
                memory: None,
                local: true,
                thread: 0,
                trusted: false,
            };
            turn_command(&spec)
                .get_envs()
                .filter(|(k, _)| *k == "CLAUDE_CODE_SUBAGENT_MODEL")
                .map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
                .last()
                .flatten()
        };
        assert_eq!(env_of("claude", &AgentOptions::default()), None);
        let mut opts = AgentOptions {
            subagent_model: " sonnet ".into(),
            ..AgentOptions::default()
        };
        assert_eq!(env_of("claude", &opts).as_deref(), Some("sonnet"));
        assert_eq!(env_of("codex", &opts), None);
        opts.env = "CLAUDE_CODE_SUBAGENT_MODEL=haiku".into();
        assert_eq!(env_of("claude", &opts).as_deref(), Some("haiku"));
    }

    #[test]
    fn codex_resumes_a_known_conversation_by_id() {
        let sel = HashMap::new();
        let o = AgentOptions::default();
        let run = |session_id| {
            provider_args(&RunSpec {
                provider: "codex",
                prompt: "hi",
                resume: true,
                sel: &sel,
                opts: &o,
                in_git: true,
                session_id,
                images: &[],
                memory: None,
                local: true,
                thread: 0,
                trusted: false,
            })
        };
        assert!(run("0199-id").ends_with(&["resume".into(), "0199-id".into(), "hi".into()]));
        assert!(run("").ends_with(&["resume".into(), "--last".into(), "hi".into()]));
    }

    #[test]
    fn claude_pins_follow_ups_to_its_own_conversation() {
        let sel = HashMap::new();
        let o = AgentOptions::default();
        let spec = |resume, session_id| RunSpec {
            provider: "claude",
            prompt: "hi",
            resume,
            sel: &sel,
            opts: &o,
            in_git: true,
            session_id,
            images: &[],
            memory: None,
            local: true,
            thread: 0,
            trusted: false,
        };
        let id = "0b7c6a52-3f7e-4f3a-9d61-2a1c9b3e8f10";
        let first = provider_args(&spec(false, id));
        assert!(first.windows(2).any(|w| w == ["--session-id", id]));
        assert!(!first.contains(&"--continue".to_string()));
        let next = provider_args(&spec(true, id));
        assert!(next.windows(2).any(|w| w == ["--resume", id]));
        assert!(!next.contains(&"--continue".to_string()));
        // The prompt always comes last, after `--`.
        assert_eq!(&next[next.len() - 2..], ["--", "hi"]);
    }

    #[test]
    fn codex_runs_its_app_server_with_the_exec_settings() {
        let sel: std::collections::HashMap<String, String> = [
            ("mode".to_string(), "plan".to_string()),
            ("effort".to_string(), "high".to_string()),
        ]
        .into();
        let o = AgentOptions {
            profile: "work".into(),
            ..AgentOptions::default()
        };
        let spec = RunSpec {
            provider: "codex",
            prompt: "Fix it",
            resume: true,
            sel: &sel,
            opts: &o,
            in_git: true,
            session_id: "01a1",
            images: &[],
            memory: None,
            local: true,
            thread: 0,
            trusted: false,
        };
        let exec = turn_command(&spec);
        // Without an app server in the installed Codex's help, `exec` runs as before.
        assert!(codex_app_config(&spec, "/w", &exec).is_none());
        crate::caps::set(
            "codex",
            crate::caps::Caps {
                help: "Commands:\n  exec  Run non-interactively\n  app-server  Run the app server"
                    .into(),
                ..Default::default()
            },
        );
        let config = codex_app_config(&spec, "/w", &exec).unwrap();
        assert_eq!(config.command[1], "app-server");
        assert!(config
            .command
            .windows(2)
            .any(|w| w == ["-c", "model_reasoning_effort=\"high\""]));
        assert!(config
            .command
            .windows(2)
            .any(|w| w == ["-c", "profile=\"work\""]));
        assert_eq!(config.fallback.last().map(String::as_str), Some("Fix it"));
        assert_eq!(
            (config.session.as_str(), config.mode.as_str()),
            ("01a1", "plan")
        );
        assert_eq!(config.effort, "high");
    }

    #[test]
    fn modes_come_from_the_manifest() {
        assert_eq!(effective_mode("cursor", ""), "full");
        assert_eq!(effective_mode("amp", "edit"), "edit");
        assert!(mode_error("amp", "plan").is_some());
        assert!(mode_error("cursor", "plan").is_none());
        assert!(mode_error("claude", "plan").is_none());
    }

    #[test]
    fn a_live_claude_turn_sends_its_prompt_on_stdin() {
        let mut command = Command::new("claude");
        command
            .args(["-p", "--output-format", "stream-json", "--", "Fix it"])
            .env("SQUAD_TEST_X", "1");
        let (live, prompt) = live_command("claude", &command).unwrap();
        assert_eq!(prompt, "Fix it");
        let args: Vec<_> = live
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-p",
                "--output-format",
                "stream-json",
                "--input-format",
                "stream-json"
            ]
        );
        assert!(live
            .get_envs()
            .any(|(k, v)| k == "SQUAD_TEST_X" && v.is_some()));
        // Without a trailing prompt there's nothing to move.
        let mut bare = Command::new("claude");
        bare.args(["-p", "hi"]);
        assert!(live_command("claude", &bare).is_none());
        assert!(takes_live_input("claude") && !takes_live_input("codex"));
        // Amp takes its messages the same way once its help lists the flag.
        let mut amp = Command::new("amp");
        amp.args(["--stream-json", "-x", "Fix it"]);
        let (live, prompt) = live_command("amp", &amp).unwrap();
        assert_eq!(prompt, "Fix it");
        let args: Vec<_> = live
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["--stream-json", "-x", "--stream-json-input"]);
        assert!(live_command("claude", &amp).is_none());
    }

    #[test]
    fn uuids_are_v4_and_distinct() {
        let a = new_uuid();
        let b = new_uuid();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(matches!(
            parts[3].chars().next(),
            Some('8' | '9' | 'a' | 'b')
        ));
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }

    #[test]
    fn provider_lookup_falls_back_to_claude() {
        assert_eq!(provider("codex").name, "Codex");
        assert_eq!(provider("nope").id, "claude");
    }

    #[cfg(unix)]
    #[test]
    fn agent_gets_env_args_and_reports_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("squadcode-agent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake-agent");
        std::fs::write(
            &script,
            "#!/bin/sh\necho \"env=$SQUAD_TEST_T\"\necho \"args=$*\"\necho oops 1>&2\nexit 3\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let sel = sel(&[("model", "opus")]);
        let o = AgentOptions {
            binary: script.to_string_lossy().into_owned(),
            env: " SQUAD_TEST_T = from-env \nNOT_A_PAIR\n=ignored".into(),
            ..Default::default()
        };
        let spec = RunSpec {
            provider: "claude",
            prompt: "do it",
            resume: false,
            sel: &sel,
            opts: &o,
            in_git: true,
            session_id: "",
            images: &[],
            memory: None,
            local: true,
            thread: 0,
            trusted: false,
        };
        run_agent(Target::Session(5), &spec, dir.to_str().unwrap(), tx).unwrap();
        let (mut out, mut err) = (vec![], vec![]);
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Line(l) | Event::Json(l, _) => out.push(l),
                Event::Err(l) => err.push(l),
                Event::Done(ok) => {
                    assert!(!ok);
                    break;
                }
                Event::Output(..) => unreachable!(),
            }
        }
        assert_eq!(out[0], "env=from-env");
        assert_eq!(
            out[1],
            "args=-p --output-format stream-json --verbose --include-partial-messages --permission-mode acceptEdits --model opus -- do it"
        );
        // stderr from an agent stays out of the conversation stream.
        assert_eq!(err, ["oops"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_binary_is_an_error_not_a_panic() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let sel = HashMap::new();
        let o = AgentOptions {
            binary: "/definitely/not/a/squadcode/binary".into(),
            ..Default::default()
        };
        let spec = RunSpec {
            provider: "claude",
            prompt: "hi",
            resume: false,
            sel: &sel,
            opts: &o,
            in_git: false,
            session_id: "",
            images: &[],
            memory: None,
            local: true,
            thread: 0,
            trusted: false,
        };
        let error = run_agent(Target::Session(1), &spec, "/", tx).err().unwrap();
        assert!(error.contains("could not start"));
    }

    #[test]
    fn kill_stops_a_running_command() {
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = std::env::temp_dir();
        let handle = run_shell(Target::Terminal(7), "sleep 30", dir.to_str().unwrap(), tx).unwrap();
        handle.kill();
        loop {
            if let Event::Done(ok) = rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                assert!(!ok);
                break;
            }
        }
    }

    #[test]
    fn configured_binaries_count_as_available() {
        assert!(available("definitely-not-a-real-agent", "/bin/sh"));
        assert!(!available("sh", "/no/such/agent"));
        assert!(available("definitely-not-a-real-agent", "sh"));
        assert!(!available("definitely-not-a-real-agent", " "));
    }

    #[test]
    fn claude_args() {
        let o = AgentOptions::default();
        assert_eq!(
            args(
                "claude",
                true,
                &[("mode", "plan"), ("model", "opus")],
                &o,
                true
            ),
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--permission-mode",
                "plan",
                "--model",
                "opus",
                "--continue",
                "--",
                "hi"
            ]
        );
        assert_eq!(
            args("claude", false, &[], &o, true),
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--permission-mode",
                "acceptEdits",
                "--",
                "hi"
            ]
        );
        let o = AgentOptions {
            effort: "high".into(),
            allowed_tools: "Read, Edit".into(),
            add_dirs: "/a,/b".into(),
            extra_args: "--verbose".into(),
            ..Default::default()
        };
        assert_eq!(
            args(
                "claude",
                false,
                &[("mode", "dontAsk"), ("effort", "max")],
                &o,
                true
            ),
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--permission-mode",
                "dontAsk",
                "--effort",
                "max",
                "--allowed-tools",
                "Read,Edit",
                "--add-dir",
                "/a",
                "--add-dir",
                "/b",
                "--verbose",
                "--",
                "hi"
            ]
        );
    }

    #[test]
    fn codex_args() {
        let o = AgentOptions::default();
        assert_eq!(
            args("codex", false, &[], &o, true),
            ["exec", "--json", "--sandbox", "workspace-write", "hi"]
        );
        assert_eq!(
            args("codex", false, &[("mode", "full")], &o, true),
            ["exec", "--json", "--sandbox", "danger-full-access", "hi"]
        );
        let o = AgentOptions {
            profile: "work".into(),
            ..Default::default()
        };
        assert_eq!(
            args(
                "codex",
                true,
                &[
                    ("mode", "read-only"),
                    ("model", "gpt-5.6-sol"),
                    ("effort", "high"),
                    ("tier", "priority"),
                    ("approval", "never")
                ],
                &o,
                false
            ),
            [
                "exec",
                "--json",
                "--sandbox",
                "read-only",
                "-m",
                "gpt-5.6-sol",
                "-c",
                "model_reasoning_effort=\"high\"",
                "-c",
                "service_tier=\"priority\"",
                "-p",
                "work",
                "--skip-git-repo-check",
                "resume",
                "--last",
                "hi"
            ]
        );
    }

    #[test]
    fn opencode_args() {
        let o = AgentOptions::default();
        assert_eq!(
            args("opencode", false, &[], &o, true),
            ["run", "--format", "json", "hi"]
        );
        assert_eq!(
            args("opencode", true, &[("mode", "plan")], &o, true),
            [
                "run",
                "--format",
                "json",
                "--continue",
                "--agent",
                "plan",
                "hi"
            ]
        );
        let o = AgentOptions {
            variant: "high".into(),
            thinking: true,
            ..Default::default()
        };
        assert_eq!(
            args(
                "opencode",
                false,
                &[
                    ("mode", "auto"),
                    ("model", "opencode/big-pickle"),
                    ("agent", "build")
                ],
                &o,
                true
            ),
            [
                "run",
                "--format",
                "json",
                "-m",
                "opencode/big-pickle",
                "--agent",
                "build",
                "--variant",
                "high",
                "--thinking",
                "--auto",
                "hi"
            ]
        );
    }

    #[test]
    fn each_cli_continues_its_own_conversation() {
        // Each CLI is given the conversation it should continue.
        let o = AgentOptions::default();
        let sel = std::collections::HashMap::new();
        let spec = |provider, resume| RunSpec {
            provider,
            prompt: "hi",
            resume,
            sel: &sel,
            opts: &o,
            in_git: true,
            session_id: "id-1",
            images: &[],
            memory: None,
            local: true,
            thread: 0,
            trusted: false,
        };
        let has = |p, resume, pair: [&str; 2]| {
            provider_args(&spec(p, resume))
                .windows(2)
                .any(|w| w == pair)
        };
        assert!(has("gemini", true, ["--resume", "id-1"]));
        assert!(has("qwen", true, ["--resume", "id-1"]));
        assert!(has("cursor", true, ["--resume", "id-1"]));
        assert!(has("copilot", true, ["--resume", "id-1"]));
        assert!(has("droid", true, ["-s", "id-1"]));
        assert!(has("opencode", true, ["-s", "id-1"]));
        assert_eq!(
            &provider_args(&spec("amp", true))[..3],
            ["threads", "continue", "id-1"]
        );
    }

    fn run_args(provider: &str, mode: &str, resume: bool, images: &[String]) -> Vec<String> {
        let sel = sel(&[("mode", mode)]);
        let o = AgentOptions::default();
        provider_args(&RunSpec {
            provider,
            prompt: "hi",
            resume,
            sel: &sel,
            opts: &o,
            in_git: true,
            session_id: "id-1",
            images,
            memory: None,
            local: true,
            thread: 0,
            trusted: true,
        })
    }

    #[test]
    fn images_never_swallow_the_prompt() {
        let images = ["a.png".to_string(), "b.png".to_string()];
        // Codex's `-i` takes several files, so a flag always follows it.
        let fresh = run_args("codex", "edit", false, &images);
        assert_eq!(
            &fresh[..6],
            ["exec", "--json", "-i", "a.png", "-i", "b.png"]
        );
        assert_eq!(fresh[6], "--sandbox");
        assert_eq!(fresh.last().unwrap(), "hi");
        // `resume` takes one file per flag, after the conversation id.
        let resumed = run_args("codex", "edit", true, &images);
        assert!(resumed.ends_with(&[
            "resume".into(),
            "id-1".into(),
            "-i".into(),
            "a.png".into(),
            "-i".into(),
            "b.png".into(),
            "hi".into()
        ]));
        // OpenCode's `-f` is variadic too, so it comes after the prompt.
        let opencode = run_args("opencode", "edit", false, &images);
        assert!(opencode.ends_with(&[
            "hi".into(),
            "-f".into(),
            "a.png".into(),
            "-f".into(),
            "b.png".into()
        ]));
        // Others read the files from the paths listed in the prompt.
        let claude = run_args("claude", "edit", false, &images);
        assert!(claude.last().unwrap().contains("- a.png"));
    }

    #[test]
    fn permission_levels_map_to_each_cli() {
        let has = |provider: &str, mode: &str, flag: &[&str]| {
            let args = run_args(provider, mode, false, &[]);
            args.windows(flag.len()).any(|w| w == flag)
        };
        // Copilot: Auto edit keeps the shell off, Plan only file writes too.
        assert!(has("copilot", "edit", &["--deny-tool", "shell"]));
        assert!(!has("copilot", "edit", &["--deny-tool", "write"]));
        assert!(has("copilot", "plan", &["--deny-tool", "write"]));
        assert!(has("copilot", "plan", &["--deny-tool", "shell"]));
        assert!(!has("copilot", "full", &["--deny-tool"]));
        assert!(has("copilot", "full", &["--allow-all-tools"]));
        // Amp: edits on its own, everything only with Full access.
        assert!(!has("amp", "edit", &["--dangerously-allow-all"]));
        assert!(has("amp", "full", &["--dangerously-allow-all"]));
        // Cursor: Plan only proposes changes; anything else is `--force`.
        assert!(!has("cursor", "plan", &["--force"]));
        assert!(has("cursor", "edit", &["--force"]));
        assert!(has("cursor", "full", &["--force"]));
        // Droid is read-only without an autonomy level.
        assert!(!has("droid", "plan", &["--auto"]));
        assert!(has("droid", "full", &["--auto", "high"]));
        // Gemini and Qwen: approval modes.
        assert!(has("gemini", "full", &["--approval-mode", "yolo"]));
        assert!(has("gemini", "edit", &["--approval-mode", "auto_edit"]));
        // OpenCode's plan agent wins over any picked sub-agent.
        let sel = sel(&[("mode", "plan"), ("agent", "build")]);
        let o = AgentOptions {
            sub_agent: "general".into(),
            ..Default::default()
        };
        let opencode = provider_args(&RunSpec {
            provider: "opencode",
            prompt: "hi",
            resume: false,
            sel: &sel,
            opts: &o,
            in_git: true,
            session_id: "",
            images: &[],
            memory: None,
            local: true,
            thread: 0,
            trusted: false,
        });
        assert!(opencode.windows(2).any(|w| w == ["--agent", "plan"]));
        assert!(!opencode.contains(&"build".to_string()));
        // Amp can't stay read-only, so it doesn't run in Plan only.
        assert!(mode_error("amp", "plan").is_some());
        assert!(mode_error("amp", "edit").is_none());
        assert!(mode_error("copilot", "plan").is_none());
        assert_eq!(modes("cursor"), ["plan", "full"]);
        assert_eq!(effective_mode("cursor", ""), "full");
        assert_eq!(effective_mode("copilot", "edit"), "edit");
    }

    #[test]
    fn structured_output_for_every_cli_that_has_it() {
        assert!(run_args("cursor", "edit", false, &[])
            .windows(2)
            .any(|w| w == ["--output-format", "stream-json"]));
        assert!(run_args("droid", "edit", false, &[])
            .windows(2)
            .any(|w| w == ["-o", "stream-json"]));
        assert!(run_args("amp", "edit", false, &[]).contains(&"--stream-json".to_string()));
        assert_eq!(output("codex"), Output::Json);
        assert_eq!(output("amp"), Output::Claude);
    }

    #[test]
    fn asks_take_a_model_before_the_prompt() {
        let args = |provider: &str, model: &str| -> Vec<String> {
            ask_command_on(provider, "", "why?", model)
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            args("claude", "haiku"),
            [
                "-p",
                "--permission-mode",
                "plan",
                "--model",
                "haiku",
                "--",
                "why?"
            ]
        );
        let codex = args("codex", "gpt-5-mini");
        assert_eq!(codex[0], "exec");
        assert!(codex.ends_with(&["-m".into(), "gpt-5-mini".into(), "why?".into()]));
        assert_eq!(args("gemini", "flash"), ["-m", "flash", "-p", "why?"]);
        // No model leaves each ask as it always was.
        assert_eq!(args("gemini", ""), ["-p", "why?"]);
        assert_eq!(args("amp", "anything"), ["-x", "why?"]);
    }

    #[test]
    fn gemini_names_a_conversation_once_its_help_lists_the_flag() {
        crate::caps::set(
            "gemini",
            crate::caps::Caps {
                help: "--session-id  Start a session with this id".into(),
                ..Default::default()
            },
        );
        assert!(names_conversation("gemini"));
        let fresh = run_args("gemini", "edit", false, &[]);
        assert!(fresh.windows(2).any(|w| w == ["--session-id", "id-1"]));
        let resumed = run_args("gemini", "edit", true, &[]);
        assert!(resumed.windows(2).any(|w| w == ["--resume", "id-1"]));
        assert!(!resumed.contains(&"--session-id".to_string()));
    }

    #[test]
    fn splits_quoted_args() {
        assert_eq!(
            split_args(r#"--foo "a b" 'c d' e"#),
            ["--foo", "a b", "c d", "e"]
        );
        assert!(split_args("   ").is_empty());
    }
}
