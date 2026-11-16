//! API keys the person brings for the agents: which providers Squad Code knows, where a
//! key's secret is kept (the OS keychain, never `settings.json`), checking a key with its
//! provider, handing it to the one run that uses it, and keeping it out of what Squad Code
//! shows and saves.
//!
//! A run's sign-in choice (`AgentOptions::key`) is one of:
//! - `""`: as before Squad Code had keys; the agent signs in however it already does.
//! - `"login"`: the agent's own login. Key variables inherited from the shell are removed,
//!   so a stray key can't bill another account.
//! - `"<provider>:<id>"`: that stored key, set in the variables the agent reads.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The sign-in choice that means the agent's own login.
pub const LOGIN: &str = "login";

/// A provider whose keys Squad Code can hold.
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    /// The variables a key is set in, for every agent that takes it.
    pub vars: &'static [&'static str],
    /// Variables that would send the agent somewhere else; removed when the key is used.
    pub clear: &'static [&'static str],
    /// The agents (ids in `agent::PROVIDERS`) that run on this provider's keys.
    pub agents: &'static [&'static str],
    /// Where the person makes a key.
    pub page: &'static str,
    /// What a key looks like, for the paste box.
    pub hint: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "anthropic",
        name: "Anthropic",
        vars: &["ANTHROPIC_API_KEY"],
        clear: &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"],
        agents: &["claude", "opencode"],
        page: "https://console.anthropic.com/settings/keys",
        hint: "sk-ant-…",
    },
    Provider {
        id: "openai",
        name: "OpenAI",
        // `codex exec` reads CODEX_API_KEY; OpenCode reads OPENAI_API_KEY.
        vars: &["OPENAI_API_KEY", "CODEX_API_KEY"],
        clear: &["OPENAI_BASE_URL"],
        agents: &["codex", "opencode"],
        page: "https://platform.openai.com/api-keys",
        hint: "sk-…",
    },
    Provider {
        id: "google",
        name: "Google Gemini",
        vars: &["GEMINI_API_KEY", "GOOGLE_GENERATIVE_AI_API_KEY"],
        clear: &["GOOGLE_GENAI_USE_VERTEXAI"],
        agents: &["gemini", "antigravity", "opencode"],
        page: "https://aistudio.google.com/apikey",
        hint: "AIza…",
    },
    Provider {
        id: "xai",
        name: "xAI",
        vars: &["XAI_API_KEY"],
        clear: &[],
        agents: &["grok", "opencode"],
        page: "https://console.x.ai",
        hint: "xai-…",
    },
    Provider {
        id: "cursor",
        name: "Cursor",
        vars: &["CURSOR_API_KEY"],
        clear: &[],
        agents: &["cursor"],
        page: "https://cursor.com/dashboard?tab=integrations",
        hint: "key_…",
    },
    Provider {
        id: "github",
        name: "GitHub Copilot",
        vars: &["COPILOT_GITHUB_TOKEN"],
        clear: &[],
        agents: &["copilot"],
        page: "https://github.com/settings/personal-access-tokens/new",
        hint: "github_pat_…",
    },
    Provider {
        id: "amp",
        name: "Amp",
        vars: &["AMP_API_KEY"],
        clear: &[],
        agents: &["amp"],
        page: "https://ampcode.com/settings",
        hint: "sgamp_…",
    },
    Provider {
        id: "factory",
        name: "Factory",
        vars: &["FACTORY_API_KEY"],
        clear: &[],
        agents: &["droid"],
        page: "https://app.factory.ai/settings/api-keys",
        hint: "fk-…",
    },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// The providers whose keys `agent` runs on.
pub fn providers_for(agent: &str) -> impl Iterator<Item = &'static Provider> + '_ {
    PROVIDERS.iter().filter(move |p| p.agents.contains(&agent))
}

/// A key the person added: what Squad Code saves about it. The secret is in the keychain.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiKey {
    pub id: String,
    pub provider: String,
    /// The person's name for it, such as "Personal" or "Work".
    pub label: String,
    /// The secret's last four characters, to tell keys apart.
    pub last4: String,
    pub added: i64,
    /// When it was last checked with its provider; 0 never.
    pub checked: i64,
    pub status: Status,
    /// What the check said, in a sentence.
    pub note: String,
    /// The models the provider lists for this key.
    pub models: Vec<String>,
    /// Kept in a private file because no keychain was found (Linux without one).
    pub in_file: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Not checked yet, or the provider has no way to check it.
    #[default]
    Unchecked,
    Works,
    /// The provider said no: wrong, revoked, or out of credit.
    Refused,
    /// The provider couldn't be reached.
    Offline,
}

impl ApiKey {
    /// The run choice that uses this key.
    pub fn choice(&self) -> String {
        format!("{}:{}", self.provider, self.id)
    }

    /// "Anthropic · Personal · …a1b2".
    pub fn title(&self) -> String {
        let name = provider(&self.provider).map_or(self.provider.as_str(), |p| p.name);
        let mut bits = vec![name.to_string()];
        if !self.label.trim().is_empty() {
            bits.push(self.label.trim().to_string());
        }
        bits.push(format!("\u{2026}{}", self.last4));
        bits.join(" \u{b7} ")
    }
}

/// The provider and key id in a run choice, when it names a key.
pub fn parse_choice(choice: &str) -> Option<(&str, &str)> {
    let (provider, id) = choice.split_once(':')?;
    (!provider.is_empty() && !id.is_empty()).then_some((provider, id))
}

/// A new key id: short, unique enough for one person's keys.
pub fn new_id() -> String {
    crate::agent::new_uuid()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(10)
        .collect()
}

pub fn last4(secret: &str) -> String {
    let chars: Vec<char> = secret.trim().chars().collect();
    chars[chars.len().saturating_sub(4)..].iter().collect()
}

// Where secrets live.

const SERVICE: &str = "Squad Code";

fn account(id: &str) -> String {
    format!("key:{id}")
}

/// Secrets read this run, so the keychain is asked once per key. Also what `scrub` hides.
static SECRETS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn remember(id: &str, secret: &str) {
    if let Ok(mut s) = SECRETS.lock() {
        s.get_or_insert_with(HashMap::new)
            .insert(id.to_string(), secret.to_string());
    }
}

/// Keep `secret` for key `id`. Returns whether it went to a private file instead of the
/// keychain (only where there is none).
pub fn store(id: &str, secret: &str) -> Result<bool, String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err("The key is empty.".into());
    }
    let in_file = vault::store(id, secret)?;
    remember(id, secret);
    Ok(in_file)
}

/// The secret of key `id`, from memory or the keychain.
pub fn secret(id: &str) -> Option<String> {
    if let Some(s) = SECRETS
        .lock()
        .ok()
        .and_then(|s| s.as_ref().and_then(|m| m.get(id).cloned()))
    {
        return Some(s);
    }
    let s = vault::read(id)?;
    remember(id, &s);
    Some(s)
}

/// Forget key `id` here and in the keychain.
pub fn forget(id: &str) {
    vault::delete(id);
    if let Ok(mut s) = SECRETS.lock() {
        if let Some(m) = s.as_mut() {
            m.remove(id);
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
mod vault {
    use super::{account, SERVICE};

    fn entry(id: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, &account(id)).map_err(|e| e.to_string())
    }

    pub fn store(id: &str, secret: &str) -> Result<bool, String> {
        entry(id)?
            .set_password(secret)
            .map(|_| false)
            .map_err(|e| format!("The keychain didn't take the key: {e}"))
    }

    pub fn read(id: &str) -> Option<String> {
        entry(id).ok()?.get_password().ok()
    }

    pub fn delete(id: &str) {
        if let Ok(e) = entry(id) {
            let _ = e.delete_credential();
        }
    }
}

/// Linux and the rest: the Secret Service through `secret-tool` (the secret goes on its
/// stdin, never in the process list), else a file only this user can read.
#[cfg(not(any(target_os = "macos", windows)))]
mod vault {
    use super::{account, SERVICE};
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn tool(args: &[&str], input: Option<&str>) -> Option<String> {
        let mut child = Command::new("secret-tool")
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin.write_all(text.as_bytes()).ok()?;
        }
        let out = child.wait_with_output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn file(id: &str) -> std::path::PathBuf {
        crate::store::data_dir().join("keys").join(id)
    }

    pub fn store(id: &str, secret: &str) -> Result<bool, String> {
        let acct = account(id);
        let label = format!("{SERVICE} API key");
        if tool(
            &[
                "store", "--label", &label, "service", SERVICE, "account", &acct,
            ],
            Some(secret),
        )
        .is_some()
        {
            let _ = std::fs::remove_file(file(id));
            return Ok(false);
        }
        let path = file(id);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut f = {
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                o.mode(0o600);
            }
            o.open(&path).map_err(|e| e.to_string())?
        };
        f.write_all(secret.as_bytes()).map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub fn read(id: &str) -> Option<String> {
        let acct = account(id);
        tool(&["lookup", "service", SERVICE, "account", &acct], None)
            .or_else(|| std::fs::read_to_string(file(id)).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn delete(id: &str) {
        let acct = account(id);
        let _ = tool(&["clear", "service", SERVICE, "account", &acct], None);
        let _ = std::fs::remove_file(file(id));
    }
}

// Handing a key to a run.

/// Each agent's sign-in choice from Settings, for the commands built without settings at
/// hand (quick questions, Peek's warm engine). Kept current by `sync`.
static DEFAULTS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Take each agent's sign-in choice from `st`.
pub fn sync(st: &crate::settings::AppSettings) {
    let map = st
        .agents
        .iter()
        .filter(|(_, a)| !a.key.trim().is_empty())
        .map(|(id, a)| (id.clone(), a.key.clone()))
        .collect();
    if let Ok(mut d) = DEFAULTS.lock() {
        *d = Some(map);
    }
}

/// Agent `agent`'s sign-in choice from Settings.
pub fn default_for(agent: &str) -> String {
    DEFAULTS
        .lock()
        .ok()
        .and_then(|d| d.as_ref().and_then(|m| m.get(agent).cloned()))
        .unwrap_or_default()
}

/// What a run's sign-in choice does to its environment: variables to set, and ones to
/// remove. `secret` finds a key's secret by id.
pub fn env_for(
    agent: &str,
    choice: &str,
    secret: &dyn Fn(&str) -> Option<String>,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut set = Vec::new();
    let mut remove = Vec::new();
    if choice.trim().is_empty() {
        return (set, remove);
    }
    if choice == LOGIN {
        for p in providers_for(agent) {
            remove.extend(p.vars.iter().chain(p.clear).map(|v| v.to_string()));
        }
        // Claude Code also bills a token or another endpoint before its login.
        if agent == "claude" {
            remove.extend(["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"].map(String::from));
        }
        remove.dedup();
        return (set, remove);
    }
    let Some((pid, id)) = parse_choice(choice) else {
        return (set, remove);
    };
    let Some(p) = provider(pid).filter(|p| p.agents.contains(&agent)) else {
        return (set, remove);
    };
    remove.extend(p.clear.iter().map(|v| v.to_string()));
    match secret(id) {
        Some(s) => set.extend(p.vars.iter().map(|v| (v.to_string(), s.clone()))),
        // The key is gone: take away the provider's variables too, so the run says
        // it isn't signed in instead of billing something else.
        None => {
            remove.extend(p.vars.iter().map(|v| v.to_string()));
            crate::applog::warn(
                "keys",
                format!("no secret for {pid} key {id}; the run goes without one"),
            );
        }
    }
    (set, remove)
}

/// Apply a run's sign-in choice to its command. Keys stay on this computer: a run
/// elsewhere (`local` false) only gets the removals.
pub fn apply(command: &mut Command, agent: &str, choice: &str, local: bool) {
    let (set, remove) = env_for(agent, choice, &secret);
    for key in remove {
        command.env_remove(key);
    }
    if local {
        for (key, value) in set {
            command.env(key, value);
        }
    }
}

/// Whether a run's choice gives it a stored key.
pub fn uses_key(choice: &str) -> bool {
    parse_choice(choice).is_some()
}

/// A line of an agent's output with any key Squad Code handed out replaced by its last
/// four characters, so a key printed by a command never reaches the screen or a log.
pub fn scrub(line: String) -> String {
    let Ok(guard) = SECRETS.lock() else {
        return line;
    };
    let Some(map) = guard.as_ref() else {
        return line;
    };
    let mut out = line;
    for s in map.values().filter(|s| s.len() >= 12) {
        if out.contains(s.as_str()) {
            out = out.replace(
                s.as_str(),
                &format!("\u{2022}\u{2022}\u{2022}\u{2022}{}", last4(s)),
            );
        }
    }
    out
}

// Checking a key.

/// What a provider said about a key.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub status: Status,
    pub note: String,
    pub models: Vec<String>,
}

/// Ask the provider whether `secret` works, and which models it can use. Runs curl,
/// with the key on its stdin so it never shows in the process list.
pub fn check(provider: &str, secret: &str) -> Check {
    let secret = secret.trim();
    let (url, headers): (&str, Vec<String>) =
        match provider {
            "anthropic" => (
                "https://api.anthropic.com/v1/models?limit=1000",
                vec![
                    format!("x-api-key: {secret}"),
                    "anthropic-version: 2023-06-01".into(),
                ],
            ),
            "openai" => (
                "https://api.openai.com/v1/models",
                vec![format!("Authorization: Bearer {secret}")],
            ),
            "google" => (
                "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000",
                vec![format!("x-goog-api-key: {secret}")],
            ),
            "xai" => (
                "https://api.x.ai/v1/models",
                vec![format!("Authorization: Bearer {secret}")],
            ),
            "github" => (
                "https://api.github.com/user",
                vec![format!("Authorization: Bearer {secret}")],
            ),
            _ => return Check {
                status: Status::Unchecked,
                note: "Saved. This provider has no way to check a key, so its first run will tell."
                    .into(),
                models: Vec::new(),
            },
        };
    match fetch(url, &headers) {
        Some((code, body)) => judge(provider, code, &body),
        None => Check {
            status: Status::Offline,
            note: "Couldn't reach the provider to check the key. It's saved; Recheck when you're online."
                .into(),
            models: Vec::new(),
        },
    }
}

fn fetch(url: &str, headers: &[String]) -> Option<(u16, String)> {
    let mut child = Command::new("curl")
        .args(["-sS", "-m", "15", "-H", "@-", "-w", "\n%{http_code}", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let mut text = headers.join("\n");
        text.push_str("\nAccept: application/json\n");
        stdin.write_all(text.as_bytes()).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.trim_end().rsplit_once('\n')?;
    let code: u16 = code.trim().parse().ok()?;
    (code != 0).then(|| (code, body.to_string()))
}

/// A provider's answer to a check, in words.
pub fn judge(provider: &str, code: u16, body: &str) -> Check {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    if (200..300).contains(&code) {
        if provider == "github" {
            let who = v["login"].as_str().unwrap_or_default();
            return Check {
                status: Status::Works,
                note: if who.is_empty() {
                    "Works.".into()
                } else {
                    format!("Works, as {who}.")
                },
                models: Vec::new(),
            };
        }
        let list = v["data"].as_array().or_else(|| v["models"].as_array());
        let mut models: Vec<String> = list
            .into_iter()
            .flatten()
            .filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()))
            .map(|m| m.trim_start_matches("models/").to_string())
            .collect();
        models.sort();
        models.dedup();
        let note = match models.len() {
            0 => "Works.".to_string(),
            1 => "Works \u{b7} 1 model.".to_string(),
            n => format!("Works \u{b7} {n} models."),
        };
        return Check {
            status: Status::Works,
            note,
            models,
        };
    }
    let said = v["error"]["message"]
        .as_str()
        .or_else(|| v["message"].as_str())
        .or_else(|| v["error"].as_str())
        .unwrap_or_default()
        .trim();
    let said: String = said.chars().take(160).collect();
    let note = match code {
        401 => "The provider refused this key: it's wrong or was revoked.".to_string(),
        402 => "The key works but the account has no credit left.".to_string(),
        403 => "The key isn't allowed to do this. Check its permissions or region.".to_string(),
        429 => "The provider is limiting this key right now. Try again in a minute.".to_string(),
        c if c >= 500 => {
            return Check {
                status: Status::Offline,
                note: format!("The provider had a problem ({c}). Recheck in a while."),
                models: Vec::new(),
            }
        }
        c => format!("The provider said no ({c})."),
    };
    let note = if said.is_empty() {
        note
    } else {
        format!("{note} It said: \u{201c}{said}\u{201d}")
    };
    Check {
        status: if code == 429 {
            Status::Offline
        } else {
            Status::Refused
        },
        note,
        models: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn auto_changes_nothing() {
        let (set, remove) = env_for("claude", "", &none);
        assert!(set.is_empty() && remove.is_empty());
    }

    #[test]
    fn login_strips_inherited_keys() {
        let (set, remove) = env_for("claude", LOGIN, &none);
        assert!(set.is_empty());
        for v in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
        ] {
            assert!(remove.iter().any(|r| r == v), "{v} kept");
        }
        let (_, remove) = env_for("codex", LOGIN, &none);
        assert!(remove.iter().any(|r| r == "CODEX_API_KEY"));
        assert!(!remove.iter().any(|r| r.starts_with("ANTHROPIC")));
    }

    #[test]
    fn a_key_sets_the_agents_variables() {
        let secret = |id: &str| (id == "k1").then(|| "sk-ant-api03-abcdefghijkl".to_string());
        let (set, remove) = env_for("claude", "anthropic:k1", &secret);
        assert_eq!(
            set,
            vec![(
                "ANTHROPIC_API_KEY".to_string(),
                "sk-ant-api03-abcdefghijkl".to_string()
            )]
        );
        assert!(remove.iter().any(|r| r == "ANTHROPIC_BASE_URL"));
        let secret = |_: &str| Some("sk-proj-xyz".to_string());
        let (set, _) = env_for("codex", "openai:k2", &secret);
        let names: Vec<&str> = set.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["OPENAI_API_KEY", "CODEX_API_KEY"]);
    }

    #[test]
    fn a_key_for_another_agent_or_a_missing_one_sets_nothing() {
        let secret = |_: &str| Some("sk-ant-x".to_string());
        let (set, remove) = env_for("codex", "anthropic:k1", &secret);
        assert!(set.is_empty() && remove.is_empty());
        let (set, remove) = env_for("claude", "anthropic:gone", &none);
        assert!(set.is_empty());
        assert!(remove.iter().any(|r| r == "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn choices_parse() {
        assert_eq!(parse_choice("xai:abc"), Some(("xai", "abc")));
        assert_eq!(parse_choice(LOGIN), None);
        assert_eq!(parse_choice(""), None);
        assert!(uses_key("openai:1") && !uses_key(LOGIN));
    }

    #[test]
    fn scrub_hides_a_known_key() {
        remember("t-scrub", "xai-0123456789abcdef");
        let line = scrub("token is xai-0123456789abcdef ok".into());
        assert_eq!(line, "token is \u{2022}\u{2022}\u{2022}\u{2022}cdef ok");
    }

    #[test]
    fn checks_read_in_words() {
        let ok = judge(
            "openai",
            200,
            r#"{"data":[{"id":"gpt-5"},{"id":"gpt-5-mini"}]}"#,
        );
        assert_eq!(ok.status, Status::Works);
        assert_eq!(ok.models, ["gpt-5", "gpt-5-mini"]);
        let g = judge(
            "google",
            200,
            r#"{"models":[{"name":"models/gemini-2.5-pro"}]}"#,
        );
        assert_eq!(g.models, ["gemini-2.5-pro"]);
        let bad = judge(
            "anthropic",
            401,
            r#"{"error":{"message":"invalid x-api-key"}}"#,
        );
        assert_eq!(bad.status, Status::Refused);
        assert!(bad.note.contains("invalid x-api-key"));
        assert_eq!(judge("xai", 503, "").status, Status::Offline);
    }

    #[test]
    fn every_provider_names_known_agents() {
        for p in PROVIDERS {
            for a in p.agents {
                assert!(
                    crate::agent::PROVIDERS.iter().any(|q| q.id == *a),
                    "{} names unknown agent {a}",
                    p.id
                );
            }
        }
    }
}
