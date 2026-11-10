//! What a bot can do, beyond its name and kind of work: its own instructions,
//! the skill files and connected apps (MCP servers) it uses, what it may do on
//! its own, what it reads first and what it starts out knowing.
//!
//! Every bot can have one, built-in or made. A bot made on Crew, or by Peek,
//! gets one drafted by the engine from what it's for and the project (see
//! `brain::maker`); the person edits it any time from the bot's Setup.
//!
//! The setup reaches the engine two ways: as words in the bot's brief, which
//! every engine reads, and as tool rules and MCP servers for the engines that
//! take them (see [`Setup::tool_rules`] and `agent::RunSpec`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use super::data::Member;

/// A skill file (a folder with a SKILL.md) the bot uses.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillRef {
    /// Its name, as in the SKILL.md front matter ("stripe-webhooks").
    pub name: String,
    /// One line on what it's for.
    pub description: String,
    /// The SKILL.md itself.
    pub path: PathBuf,
}

/// A connected app (MCP server) the person added to one bot: a command that
/// starts it, or a URL that reaches it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BotServer {
    pub name: String,
    pub command: String,
    pub url: String,
}

impl BotServer {
    /// Read from what the person typed: a URL, or a command line.
    pub fn parse(name: &str, text: &str) -> Result<BotServer, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Paste a URL or a command that starts the server.".into());
        }
        let is_url = text.starts_with("http://") || text.starts_with("https://");
        let name = if name.trim().is_empty() {
            // "npx -y @stripe/mcp" → stripe-mcp; "https://mcp.linear.app/sse" → mcp-linear-app
            let guess = if is_url {
                text.split("//")
                    .nth(1)
                    .and_then(|h| h.split('/').next())
                    .unwrap_or(text)
            } else {
                text.split_whitespace()
                    .rfind(|w| !w.starts_with('-'))
                    .unwrap_or(text)
            };
            slug(guess)
        } else {
            slug(name)
        };
        Ok(if is_url {
            BotServer {
                name,
                url: text.to_string(),
                ..Default::default()
            }
        } else {
            BotServer {
                name,
                command: text.to_string(),
                ..Default::default()
            }
        })
    }

    pub fn spec(&self) -> crate::mcp::McpSpec {
        let mut words = crate::agent::split_args(&self.command).into_iter();
        crate::mcp::McpSpec {
            command: words.next().unwrap_or_default(),
            args: words.collect(),
            url: self.url.trim().to_string(),
            // An address ending in /sse speaks the older transport.
            sse: self.url.trim_end_matches('/').ends_with("/sse"),
            ..Default::default()
        }
    }
}

/// What a bot may do on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Perms {
    /// It may change files in its own copy of the project; off, it only reads.
    pub edit: bool,
    /// It may add or install packages.
    pub install: bool,
    /// It may fetch web pages and search the web.
    pub web: bool,
    /// It may push, deploy or publish, asking every time; off, it never does.
    pub push: bool,
}

impl Default for Perms {
    fn default() -> Self {
        Perms {
            edit: true,
            install: true,
            web: true,
            push: true,
        }
    }
}

/// Package commands "install" covers, Claude Code style.
pub(super) const INSTALLS: &[&str] = &[
    "npm install",
    "npm i",
    "npm add",
    "pnpm add",
    "pnpm install",
    "yarn add",
    "bun add",
    "pip install",
    "uv add",
    "poetry add",
    "cargo add",
    "cargo install",
    "go get",
    "gem install",
    "brew install",
];

/// Push, deploy and publish commands.
const PUSHES: &[&str] = &[
    "git push",
    "npm publish",
    "pnpm publish",
    "yarn publish",
    "cargo publish",
    "gh pr merge",
    "vercel",
    "fly deploy",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Setup {
    /// Who it is and how it works, in Markdown, told to its engine on every job.
    pub instructions: String,
    /// Skill files it uses.
    pub skill_files: Vec<SkillRef>,
    /// Connected apps found on this computer that it uses, by `mcp::key`
    /// ("Claude Code/github").
    pub mcp: Vec<String>,
    /// Connected apps the person added to this bot alone.
    pub servers: Vec<BotServer>,
    pub perms: Perms,
    /// Files, folders or links it reads before it starts.
    pub reads: Vec<String>,
    /// Facts about the project it starts out knowing.
    pub memory: Vec<String>,
    /// The engine drafted it (the person may have changed it since).
    pub drafted: bool,
}

impl Setup {
    pub fn is_empty(&self) -> bool {
        *self == Setup::default()
    }

    /// The setup as words for the bot's brief: empty when there's nothing
    /// beyond its built-in persona.
    pub fn brief(&self) -> String {
        let mut out = String::new();
        let text = self.instructions.trim();
        if !text.is_empty() {
            out.push_str(text);
            out.push_str("\n\n");
        }
        if !self.skill_files.is_empty() {
            out.push_str(
                "Your skills. Before work one of these covers, read its SKILL.md and follow it:\n",
            );
            for s in &self.skill_files {
                out.push_str(&format!("- {}", s.name));
                if !s.description.trim().is_empty() {
                    out.push_str(&format!(": {}", s.description.trim()));
                }
                out.push_str(&format!(" ({})\n", s.path.display()));
            }
            out.push('\n');
        }
        if !self.reads.is_empty() {
            out.push_str("Read these first:\n");
            for r in &self.reads {
                out.push_str(&format!("- {}\n", r.trim()));
            }
            out.push('\n');
        }
        if !self.memory.is_empty() {
            out.push_str("What you already know about this project:\n");
            for m in &self.memory {
                out.push_str(&format!("- {}\n", m.trim()));
            }
            out.push('\n');
        }
        let mut limits = Vec::new();
        if !self.perms.edit {
            limits.push("Never change files: read and report.");
        }
        if !self.perms.install {
            limits.push("Never add or install packages; say which one you'd need instead.");
        }
        if !self.perms.web {
            limits.push("Don't use the web: work from the code and what you were given.");
        }
        if !self.perms.push {
            limits.push("Never push, deploy or publish.");
        }
        if !limits.is_empty() {
            out.push_str("Limits:\n");
            for l in limits {
                out.push_str(&format!("- {l}\n"));
            }
        }
        out.trim_end().to_string()
    }

    /// Tool rules for Claude Code style engines: the ones the bot is denied.
    pub fn tool_rules(&self) -> Vec<String> {
        let mut deny = Vec::new();
        let bash = |list: &[&str]| -> Vec<String> {
            list.iter().map(|c| format!("Bash({c}:*)")).collect()
        };
        if !self.perms.install {
            deny.extend(bash(INSTALLS));
        }
        // Allowed is how every bot works already (web steps are asked about as
        // usual), so only a "no" adds a rule.
        if !self.perms.web {
            deny.extend(["WebFetch".to_string(), "WebSearch".to_string()]);
        }
        if !self.perms.push {
            deny.extend(bash(PUSHES));
        }
        if !self.perms.edit {
            deny.extend(
                ["Edit", "Write", "MultiEdit", "NotebookEdit"]
                    .iter()
                    .map(|t| t.to_string()),
            );
        }
        deny
    }
}

static SETUPS: RwLock<Vec<(String, Setup)>> = RwLock::new(Vec::new());

/// Makes `setups` the ones the app knows about (on load, and after a change).
pub fn set_all(setups: &HashMap<String, Setup>) {
    *SETUPS.write().unwrap_or_else(|e| e.into_inner()) =
        setups.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
}

/// The setup of `who`; empty when it has none.
pub fn of(who: Member) -> Setup {
    let key = super::engine::bot_key(who);
    SETUPS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, s)| s.clone())
        .unwrap_or_default()
}

/// Everything `who` is told about itself on every job and question: its
/// persona, then its setup.
pub fn brief(who: Member) -> String {
    let extra = of(who).brief();
    if extra.is_empty() {
        who.persona().to_string()
    } else {
        format!("{}\n\n{extra}", who.persona())
    }
}

/// Where a bot's own files go: `~/.squadcode/bots/<key>/`.
pub fn folder(who: Member) -> PathBuf {
    crate::store::data_dir()
        .join("bots")
        .join(super::engine::bot_key(who))
}

/// A short name every agent accepts for a skill's folder: "Stripe webhooks!"
/// → "stripe-webhooks".
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "skill".into()
    } else {
        out
    }
}

/// Writes a skill the engine drafted for `who` as its own SKILL.md, in the
/// bot's folder so no agent's own skills change. Returns where it went.
pub fn write_skill(
    who: Member,
    name: &str,
    description: &str,
    body: &str,
) -> std::io::Result<SkillRef> {
    let name = slug(name);
    let dir = folder(who).join("skills").join(&name);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("SKILL.md");
    let description = description.trim().replace('\n', " ");
    let text = format!(
        "---\nname: {name}\ndescription: {description}\n---\n\n{}\n",
        body.trim()
    );
    std::fs::write(&path, text)?;
    Ok(SkillRef {
        name,
        description,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_setup_adds_nothing() {
        let s = Setup::default();
        assert!(s.is_empty());
        assert_eq!(s.brief(), "");
        assert!(s.tool_rules().is_empty());
    }

    #[test]
    fn the_brief_holds_skills_memory_and_limits() {
        let s = Setup {
            instructions: "# Ledger\nYou own billing.".into(),
            skill_files: vec![SkillRef {
                name: "stripe-webhooks".into(),
                description: "How events flow".into(),
                path: PathBuf::from("/b/SKILL.md"),
            }],
            reads: vec!["docs/payments.md".into()],
            memory: vec!["Money is integer cents".into()],
            perms: Perms {
                install: false,
                push: false,
                ..Perms::default()
            },
            ..Setup::default()
        };
        let b = s.brief();
        assert!(b.starts_with("# Ledger"));
        assert!(b.contains("- stripe-webhooks: How events flow (/b/SKILL.md)"));
        assert!(b.contains("- docs/payments.md"));
        assert!(b.contains("- Money is integer cents"));
        assert!(b.contains("Never push"));
        let deny = s.tool_rules();
        assert!(deny.contains(&"Bash(npm install:*)".to_string()));
        assert!(deny.contains(&"Bash(git push:*)".to_string()));
    }

    #[test]
    fn a_read_only_bot_cannot_edit() {
        let s = Setup {
            perms: Perms {
                edit: false,
                ..Perms::default()
            },
            ..Setup::default()
        };
        assert!(s.tool_rules().contains(&"Edit".to_string()));
        assert!(s.brief().contains("Never change files"));
    }

    #[test]
    fn a_server_is_a_url_or_a_command() {
        let s = BotServer::parse("", "https://mcp.linear.app/sse").unwrap();
        assert_eq!(s.name, "mcp-linear-app");
        assert!(s.spec().sse && s.spec().command.is_empty());
        let s = BotServer::parse("", "npx -y @stripe/mcp --tools=all").unwrap();
        assert_eq!(s.name, "stripe-mcp");
        let spec = s.spec();
        assert_eq!(spec.command, "npx");
        assert_eq!(spec.args, vec!["-y", "@stripe/mcp", "--tools=all"]);
        assert_eq!(BotServer::parse("Docs", "docs-mcp").unwrap().name, "docs");
        assert!(BotServer::parse("x", " ").is_err());
    }

    #[test]
    fn skill_names_are_slugs() {
        assert_eq!(slug("Stripe webhooks!"), "stripe-webhooks");
        assert_eq!(slug("  "), "skill");
    }
}
