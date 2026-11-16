//! The agents Squad Code can run, described as data rather than code. Each agent's facts (its
//! command, how it installs and signs in, which output format and protocols it speaks,
//! and which of them only newer releases have) come from a manifest: `agents.json`,
//! built into Squad Code, with the person's own `agents.json` in Squad Code's folder on top. That
//! file can change a built-in agent's facts or add an agent of its own, such as any ACP
//! agent, without a new release of Squad Code.
//!
//! A fact that depends on the installed release is a `Gate`: true, false, or text the
//! CLI's `--help` must mention, checked against what `caps` found when Squad Code started.
//! Nothing here starts a model or costs tokens.
//!
//! Newer manifests come over the air: alongside the check for a new Squad Code, `fetch`
//! downloads the manifest published with the `agent-registry` release, and keeps it when
//! its Ed25519 signature checks out against the key built into Squad Code (`agents.pub`), its
//! version is newer than any Squad Code has, and this Squad Code is new enough for it. It is used
//! from the next start, so the list of agents never changes under a running window.

use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;

/// The manifest built into Squad Code.
const BUILT_IN: &str = include_str!("agents.json");

/// The public half of the key published manifests are signed with: 32 bytes of Ed25519,
/// in base64. Empty, Squad Code downloads nothing and keeps the manifest it was built with.
const PUBLIC_KEY: &str = include_str!("agents.pub");

/// A fact that holds always, never, or once the CLI's help mentions some text.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Gate {
    Always(bool),
    /// The installed CLI's `--help` mentions this text.
    Mentions(String),
}

impl Default for Gate {
    fn default() -> Gate {
        Gate::Always(false)
    }
}

impl Gate {
    pub fn holds(&self, provider: &str) -> bool {
        match self {
            Gate::Always(on) => *on,
            Gate::Mentions(text) => crate::caps::mentions(provider, text),
        }
    }
}

/// Arguments to use once the CLI's help mentions `when` (always, without it).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Args {
    #[serde(default)]
    pub when: Option<String>,
    pub args: Vec<String>,
}

/// An output format, once the CLI's help mentions `when`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Format {
    #[serde(default)]
    pub when: Option<String>,
    pub format: String,
}

/// How a turn's command takes its prompt and later messages on stdin instead: the
/// prompt after `flag` at the end of the arguments goes, and `args` take its place.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Live {
    pub flag: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub when: Option<String>,
}

/// How an agent installs: its npm package, or its vendor's install script.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Install {
    #[serde(default)]
    pub npm: Option<String>,
    /// Use npm when it is there, the script otherwise.
    #[serde(default)]
    pub prefer_npm: bool,
    #[serde(default)]
    pub unix: Option<String>,
    #[serde(default)]
    pub windows: Option<String>,
}

/// The command that lists an agent's models.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Models {
    pub args: Vec<String>,
    /// The list is a table whose header names `provider` and `model` columns.
    #[serde(default)]
    pub table: bool,
}

/// A print-mode command for an agent Squad Code has no code for: the arguments, the flag that
/// picks a model, and the prompt last.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Print {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// One agent.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct AgentSpec {
    pub id: String,
    pub name: String,
    pub bin: String,
    /// Other names the CLI installs under.
    #[serde(default)]
    pub alt_bins: Vec<String>,
    #[serde(default)]
    pub mark: String,
    /// "#rrggbb".
    #[serde(default)]
    pub color: String,
    /// How Squad Code talks to an agent it has no code of its own for: "acp", or "pi" for
    /// Pi's RPC mode. Empty for the agents Squad Code knows.
    #[serde(default)]
    pub driver: String,
    /// The npm package Squad Code checks for updates.
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub install: Install,
    /// Arguments of the CLI's sign-in command; none means the CLI asks on first run.
    #[serde(default)]
    pub login: Vec<String>,
    /// Arguments of the CLI's sign-out command; none means the CLI has no logout.
    #[serde(default)]
    pub logout: Vec<String>,
    /// The CLI keeps its own conversation that a follow-up can resume.
    #[serde(default)]
    pub resumes: Gate,
    /// The CLI takes a conversation id Squad Code picks on the first run.
    #[serde(default)]
    pub names_conversation: Gate,
    /// Images can be attached for the agent.
    #[serde(default)]
    pub images: Gate,
    /// Squad Code's permission levels the CLI can hold to.
    #[serde(default = "all_modes")]
    pub modes: Vec<String>,
    /// The output format to ask for: the first whose `when` the help mentions.
    #[serde(default)]
    pub output: Vec<Format>,
    /// Arguments that run the agent over ACP: the first whose `when` the help mentions.
    #[serde(default)]
    pub acp: Vec<Args>,
    /// Arguments that run the agent over its own protocol: Codex's app server, Pi's RPC
    /// mode.
    #[serde(default)]
    pub rpc: Vec<Args>,
    #[serde(default)]
    pub live: Vec<Live>,
    #[serde(default)]
    pub models: Option<Models>,
    /// Reasoning levels the agent takes, for the composer's Effort menu.
    #[serde(default)]
    pub efforts: Vec<String>,
    #[serde(default)]
    pub print: Option<Print>,
}

fn all_modes() -> Vec<String> {
    ["edit", "plan", "full"].map(String::from).to_vec()
}

impl AgentSpec {
    /// The first `args` whose `when` the installed CLI's help mentions.
    pub fn pick<'a>(&self, choices: &'a [Args]) -> Option<&'a [String]> {
        choices
            .iter()
            .find(|c| {
                c.when
                    .as_deref()
                    .is_none_or(|w| crate::caps::mentions(&self.id, w))
            })
            .map(|c| c.args.as_slice())
    }

    /// The output format to ask the installed CLI for.
    pub fn format(&self) -> &str {
        self.output
            .iter()
            .find(|f| {
                f.when
                    .as_deref()
                    .is_none_or(|w| crate::caps::mentions(&self.id, w))
            })
            .map(|f| f.format.as_str())
            .unwrap_or("text")
    }

    /// The colour as a number for the UI.
    pub fn color(&self) -> u32 {
        u32::from_str_radix(self.color.trim_start_matches('#'), 16).unwrap_or(0x8a8a93)
    }
}

#[derive(Deserialize)]
struct Manifest {
    /// Raised with every change, so an older manifest never replaces a newer one.
    #[serde(default)]
    version: u64,
    /// The oldest Squad Code that understands this manifest.
    /// Named `min_runo` in manifests written before the rename.
    #[serde(default, alias = "min_runo")]
    min_squadcode: String,
    #[serde(default)]
    agents: Vec<Value>,
}

/// Whether `signature` (base64) is `key`'s Ed25519 signature of `text`.
pub fn verify(text: &[u8], signature: &str, key: &str) -> bool {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let (Ok(signature), Ok(key)) = (b64.decode(signature.trim()), b64.decode(key.trim())) else {
        return false;
    };
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(text, &signature)
        .is_ok()
}

/// The version of manifest `text`, when this Squad Code can use it: well formed, and not
/// written for a newer Squad Code.
fn usable(text: &str) -> Option<u64> {
    let manifest: Manifest = serde_json::from_str(text).ok()?;
    let ours = env!("CARGO_PKG_VERSION");
    if !manifest.min_squadcode.is_empty() && crate::alerts::is_newer(&manifest.min_squadcode, ours)
    {
        return None;
    }
    merge(text, None).ok()?;
    Some(manifest.version)
}

fn version_of(text: &str) -> u64 {
    serde_json::from_str::<Manifest>(text).map_or(0, |m| m.version)
}

/// The downloaded manifest kept in `dir`, when its signature checks out against `key`,
/// it is newer than the built-in one, and this Squad Code can use it.
fn kept(dir: &std::path::Path, key: &str, built_in: &str) -> Option<String> {
    if key.trim().is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(dir.join("agents.json")).ok()?;
    let signature = std::fs::read_to_string(dir.join("agents.json.sig")).ok()?;
    if !verify(text.as_bytes(), &signature, key) {
        return None;
    }
    (usable(&text)? > version_of(built_in)).then_some(text)
}

fn cache() -> std::path::PathBuf {
    crate::store::data_dir().join("registry")
}

/// The manifest `text` with `over` on top: an agent `over` names by an id `text` has
/// gets the fields `over` gives replaced; a new id adds an agent, run over ACP when it
/// gives ACP arguments and no driver.
pub fn merge(text: &str, over: Option<&str>) -> Result<Vec<AgentSpec>, String> {
    let base: Manifest = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut agents = base.agents;
    if let Some(over) = over {
        let over: Manifest = serde_json::from_str(over).map_err(|e| format!("agents.json: {e}"))?;
        for agent in over.agents {
            let Some(id) = agent["id"].as_str() else {
                continue;
            };
            match agents.iter_mut().find(|a| a["id"] == id) {
                Some(Value::Object(known)) => {
                    if let Value::Object(fields) = agent {
                        known.extend(fields);
                    }
                }
                _ => {
                    let mut agent = agent;
                    if agent.get("driver").is_none() && agent.get("acp").is_some() {
                        agent["driver"] = "acp".into();
                    }
                    agents.push(agent);
                }
            }
        }
    }
    let mut specs = Vec::new();
    for agent in agents {
        let id = agent["id"].as_str().unwrap_or("?").to_string();
        let spec: AgentSpec =
            serde_json::from_value(agent).map_err(|e| format!("agent {id}: {e}"))?;
        if spec.id.is_empty()
            || spec.bin.is_empty()
            || specs.iter().any(|s: &AgentSpec| s.id == spec.id)
        {
            continue;
        }
        specs.push(spec);
    }
    Ok(specs)
}

struct Registry {
    specs: &'static [AgentSpec],
    providers: &'static [crate::agent::Provider],
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// The person's own manifest, read once when Squad Code starts.
fn own() -> Option<String> {
    if cfg!(test) {
        return None;
    }
    std::fs::read_to_string(crate::store::data_dir().join("agents.json")).ok()
}

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| {
        let own = own();
        let downloaded = if cfg!(test) {
            None
        } else {
            kept(&cache(), PUBLIC_KEY, BUILT_IN)
        };
        let base = downloaded.as_deref().unwrap_or(BUILT_IN);
        let specs = match merge(base, own.as_deref()) {
            Ok(specs) => specs,
            Err(why) => {
                // A broken file of the person's leaves the built-in agents as they are.
                eprintln!("Squad Code couldn't read your agents.json, so it was left out: {why}");
                merge(base, None).expect("a kept manifest reads")
            }
        };
        let specs: &'static [AgentSpec] = Box::leak(specs.into_boxed_slice());
        let providers: Vec<crate::agent::Provider> = specs
            .iter()
            .map(|s| crate::agent::Provider {
                id: &s.id,
                name: &s.name,
                bin: &s.bin,
                mark: if s.mark.is_empty() {
                    &s.name[..s.name.chars().next().map_or(0, char::len_utf8)]
                } else {
                    &s.mark
                },
                color: s.color(),
            })
            .collect();
        Registry {
            specs,
            providers: Box::leak(providers.into_boxed_slice()),
        }
    })
}

/// Every agent, the built-in ones first, in a fixed order for the whole run.
pub fn providers() -> &'static [crate::agent::Provider] {
    registry().providers
}

/// The other names the CLI whose usual command is `bin` installs under.
pub fn alt_bins(bin: &str) -> impl Iterator<Item = &'static str> + '_ {
    registry()
        .specs
        .iter()
        .filter(move |s| s.bin == bin)
        .flat_map(|s| s.alt_bins.iter().map(String::as_str))
}

/// The manifest entry of `id`.
pub fn spec(id: &str) -> Option<&'static AgentSpec> {
    registry().specs.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_manifest_names_every_agent_once() {
        let specs = merge(BUILT_IN, None).unwrap();
        let ids: Vec<&str> = specs.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "claude",
                "codex",
                "opencode",
                "cursor",
                "gemini",
                "copilot",
                "amp",
                "droid",
                "qwen",
                "grok",
                "antigravity",
                "pi"
            ]
        );
        let claude = &specs[0];
        assert_eq!(claude.color(), 0xd9633f);
        assert_eq!(claude.resumes, Gate::Always(true));
        assert_eq!(claude.modes, ["edit", "plan", "full"]);
        let gemini = specs.iter().find(|s| s.id == "gemini").unwrap();
        assert_eq!(
            gemini.names_conversation,
            Gate::Mentions("--session-id".into())
        );
        assert_eq!(gemini.acp.len(), 2);
    }

    #[test]
    fn the_persons_file_changes_facts_and_adds_acp_agents() {
        let own = r#"{"agents":[
            {"id":"grok","modes":["full","edit"]},
            {"id":"kimi","name":"Kimi","bin":"kimi","acp":[{"args":["--acp"]}]},
            {"name":"no id","bin":"x"}
        ]}"#;
        let specs = merge(BUILT_IN, Some(own)).unwrap();
        let grok = specs.iter().find(|s| s.id == "grok").unwrap();
        assert_eq!(grok.modes, ["full", "edit"]);
        assert_eq!(grok.name, "Grok", "fields it doesn't give stay");
        let kimi = specs.last().unwrap();
        assert_eq!(kimi.id, "kimi");
        assert_eq!(kimi.driver, "acp");
        assert_eq!(kimi.acp[0].args, ["--acp"]);
        assert_eq!(kimi.modes, ["edit", "plan", "full"]);
        assert!(merge(BUILT_IN, Some("{not json")).is_err());
    }

    #[test]
    fn signatures_made_by_openssl_check_out() {
        // `openssl pkeyutl -sign -rawin` with a throwaway key, as the publishing workflow
        // signs, and the key's public half as `agents.pub` holds it.
        let public = "loK6moR+lVv/gchdpEtWUC3ykzTj6m9/qKsr8E7dnzc=";
        let signature = "/hlNev7mhRsBBKLyk96Syfl4k1Egz/BV9dRj0K4LX0Chx9E5N7yLFraHKSA58A+tc5pvODX6i63h+kXP8Y3VCA==";
        assert!(verify(b"{\"version\":2}\n", signature, public));
        assert!(!verify(b"{\"version\":3}\n", signature, public));
        assert!(!verify(b"x", "not base64!", public));
    }

    #[test]
    fn a_manifest_for_a_newer_app_is_refused_under_either_name() {
        for key in ["min_squadcode", "min_runo"] {
            let text = format!(r#"{{"version":99,"{key}":"999.0.0","agents":[]}}"#);
            assert!(usable(&text).is_none(), "{key}");
        }
    }

    #[test]
    fn the_built_in_manifest_has_a_version() {
        assert!(version_of(BUILT_IN) >= 1);
        assert!(usable(BUILT_IN).is_some());
    }

    #[test]
    fn gates_and_choices_follow_the_installed_help() {
        let spec: AgentSpec = serde_json::from_value(serde_json::json!({
            "id": "registry-test", "name": "T", "bin": "t",
            "output": [{ "when": "--json", "format": "json" }],
            "acp": [{ "when": "--acp", "args": ["--acp"] }, { "args": ["acp"] }],
        }))
        .unwrap();
        assert_eq!(spec.format(), "text");
        assert_eq!(spec.pick(&spec.acp).unwrap(), ["acp"]);
        crate::caps::set(
            "registry-test",
            crate::caps::Caps {
                help: "--json  --acp".into(),
                ..Default::default()
            },
        );
        assert_eq!(spec.format(), "json");
        assert_eq!(spec.pick(&spec.acp).unwrap(), ["--acp"]);
        assert!(Gate::Mentions("--acp".into()).holds("registry-test"));
        assert!(!Gate::Always(false).holds("registry-test"));
    }
}
