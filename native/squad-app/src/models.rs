//! The models each agent can use, with version numbers, from what the CLI itself reports:
//! Claude Code's `/model` aliases and the model ids its build knows, plus the models
//! used in past conversations, and model lists or `--model` choices from other CLIs.

use crate::caps::{Choice, ModelInfo};
use std::path::Path;

/// A Claude model id split into family and version: claude-opus-5-5 → ("opus", [5, 5]).
/// Dated snapshot ids (claude-haiku-4-5-20251001) are left out; the plain id covers them.
pub(crate) fn claude_id(id: &str) -> Option<(String, Vec<u64>)> {
    let rest = id.strip_prefix("claude-")?;
    let mut parts = rest.split('-');
    let family = parts.next()?.to_string();
    if !["fable", "opus", "sonnet", "haiku"].contains(&family.as_str()) {
        return None;
    }
    let version: Vec<u64> = parts
        .map(|p| p.parse::<u64>().ok().filter(|n| *n < 1000))
        .collect::<Option<Vec<_>>>()?;
    (!version.is_empty() && version.len() <= 2).then_some((family, version))
}

fn family_name(family: &str) -> String {
    let mut c = family.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn dotted(version: &[u64]) -> String {
    version
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

/// Model ids in a block of text or bytes, such as a CLI build or a transcript.
pub(crate) fn claude_ids_in(text: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let needle = b"claude-";
    let mut i = 0;
    while i + needle.len() < text.len() {
        if &text[i..i + needle.len()] == needle {
            let end = text[i..]
                .iter()
                .position(|b| !(b.is_ascii_alphanumeric() || *b == b'-'))
                .map(|n| i + n)
                .unwrap_or(text.len());
            if let Ok(id) = std::str::from_utf8(&text[i..end]) {
                let id = id.trim_end_matches('-');
                if claude_id(id).is_some() && !found.iter().any(|f| f == id) {
                    found.push(id.to_string());
                }
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    found
}

/// The model ids in a file, read 4 MB at a time so a 300 MB binary never sits
/// in memory whole. Each chunk is cut after its last byte that can't be part of
/// an id, so an id split across two reads is still found whole.
pub(crate) fn claude_ids_in_file(path: &Path) -> std::io::Result<Vec<String>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 4 << 20];
    let mut keep = 0usize;
    let mut found: Vec<String> = Vec::new();
    loop {
        let n = f.read(&mut buf[keep..])?;
        let end = keep + n;
        let last = n == 0;
        let cut = if last {
            end
        } else {
            buf[..end]
                .iter()
                .rposition(|b| !(b.is_ascii_alphanumeric() || *b == b'-'))
                .map(|i| i + 1)
                .unwrap_or(end)
        };
        for id in claude_ids_in(&buf[..cut]) {
            if !found.contains(&id) {
                found.push(id);
            }
        }
        if last {
            return Ok(found);
        }
        // Carry the unfinished word over; a run of id bytes this long isn't an id.
        let rest = end - cut;
        keep = if rest <= 256 {
            buf.copy_within(cut..end, 0);
            rest
        } else {
            0
        };
    }
}

/// The last `max` bytes of a file, from the start of a line.
fn read_tail(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let from = len.saturating_sub(max);
    f.seek(SeekFrom::Start(from))?;
    let mut out = Vec::with_capacity((len - from) as usize);
    f.read_to_end(&mut out)?;
    if from > 0 {
        let start = out
            .iter()
            .position(|b| *b == b'\n')
            .map_or(out.len(), |i| i + 1);
        out.drain(..start);
    }
    Ok(out)
}

/// What Claude Code's `/model` command prints: the default model's name and the aliases.
pub(crate) fn parse_model_command(text: &str) -> (String, Vec<String>) {
    let default = text
        .lines()
        .find_map(|l| l.split_once("Current model:"))
        .map(|(_, rest)| {
            rest.trim()
                .trim_matches('`')
                .trim_end_matches("(default)")
                .trim()
                .trim_matches('`')
                .to_string()
        })
        .unwrap_or_default();
    let aliases = text
        .lines()
        .find_map(|l| l.split_once("Available:"))
        .map(|(_, rest)| {
            rest.split(',')
                .map(|a| a.trim().trim_end_matches('.').trim())
                .filter(|a| {
                    !a.is_empty() && *a != "default" && !a.starts_with("or ") && !a.contains(' ')
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    (default, aliases)
}

/// Claude Code's model menu: the aliases (each with the version it points to now), then
/// every specific version, newest first. `known` are ids this CLI build knows, `used` are
/// ids from past conversations. Returns the menu and the default model's name.
pub(crate) fn claude_menu(
    model_command: &str,
    help_aliases: &[String],
    known: &[String],
    used: &[String],
) -> (Vec<ModelInfo>, String) {
    let (default, mut aliases) = parse_model_command(model_command);
    if aliases.is_empty() {
        aliases = help_aliases.to_vec();
    }
    // Versions by family, newest first. Keep the newest three per family from the build,
    // plus anything this person has actually used.
    let mut versions: Vec<(String, Vec<u64>, String)> = Vec::new();
    for id in known.iter().chain(used) {
        if let Some((family, version)) = claude_id(id) {
            if !versions
                .iter()
                .any(|(f, v, _)| *f == family && *v == version)
            {
                versions.push((family, version, id.clone()));
            }
        }
    }
    let order = |f: &str| {
        ["fable", "opus", "sonnet", "haiku"]
            .iter()
            .position(|x| *x == f)
    };
    versions.sort_by(|a, b| order(&a.0).cmp(&order(&b.0)).then(b.1.cmp(&a.1)));
    let mut kept: Vec<(String, Vec<u64>, String)> = Vec::new();
    for (family, version, id) in versions {
        let in_family = kept.iter().filter(|(f, _, _)| *f == family).count();
        if in_family < 3 || used.contains(&id) {
            kept.push((family, version, id));
        }
    }
    let newest = |family: &str| {
        kept.iter()
            .find(|(f, _, _)| f == family)
            .map(|(_, v, _)| dotted(v))
    };
    let mut menu = Vec::new();
    for alias in &aliases {
        let (base, million) = match alias.strip_suffix("[1m]") {
            Some(base) => (base, true),
            None => (alias.as_str(), false),
        };
        let (label, desc) = match base {
            "best" => (
                "Best".to_string(),
                "The most capable model you have".to_string(),
            ),
            "opusplan" => (
                "Opus plan".to_string(),
                "Opus while planning, then Sonnet to build".to_string(),
            ),
            family => {
                let name = family_name(family);
                match newest(family) {
                    Some(v) => (format!("{name} {v}"), "Always the latest".to_string()),
                    None => (name.clone(), "Always the latest".to_string()),
                }
            }
        };
        let (label, desc) = if million {
            (
                format!("{label} · 1M context"),
                format!("{desc}, long context"),
            )
        } else {
            (label, desc)
        };
        menu.push(ModelInfo {
            choice: Choice {
                value: alias.clone(),
                label,
                desc,
            },
            ..Default::default()
        });
    }
    for (family, version, id) in &kept {
        menu.push(ModelInfo {
            choice: Choice {
                value: id.clone(),
                label: format!("{} {}", family_name(family), dotted(version)),
                desc: format!("{id} · stays on this version"),
            },
            ..Default::default()
        });
    }
    (menu, default)
}

/// Claude model ids used in recent conversations on this computer.
pub(crate) fn claude_used_ids(home: &Path) -> Vec<String> {
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = Vec::new();
    if let Ok(projects) = std::fs::read_dir(home.join(".claude").join("projects")) {
        for project in projects.flatten() {
            if let Ok(entries) = std::fs::read_dir(project.path()) {
                for e in entries.flatten() {
                    let path = e.path();
                    if path.extension().is_some_and(|x| x == "jsonl") {
                        let modified = e
                            .metadata()
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::UNIX_EPOCH);
                        files.push((modified, path));
                    }
                }
            }
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    let mut ids = Vec::new();
    for (_, path) in files.into_iter().take(40) {
        // The end of each session is enough to see its model, and keeps a long
        // history from being read whole at every launch.
        if let Ok(bytes) = read_tail(&path, 256 << 10) {
            for line in bytes.split(|b| *b == b'\n') {
                // Only the model that answered, not ids mentioned in text.
                if let Some(at) = find(line, b"\"model\":\"claude-") {
                    for id in claude_ids_in(&line[at + 9..(at + 60).min(line.len())]) {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
            }
        }
    }
    ids
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Models from a CLI's model list (`agent models`): one model per line, an id first,
/// optionally followed by a name after " - ", "  " or a tab.
pub(crate) fn parse_model_list(text: &str) -> Vec<ModelInfo> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches(['*', '-', '•', '>']).trim();
        let (id, rest) = line
            .split_once(char::is_whitespace)
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((line, ""));
        let id = id.trim_end_matches(':');
        // `agent models` (Cursor) names each model after " - ", so an id there needs no
        // digit or dash, as with `auto - Auto` or `cheetah - Cheetah` (but is lower case,
        // unlike a line such as `Error - not logged in`).
        let named = rest.starts_with("- ") || rest.starts_with("– ");
        let looks_like_model = !id.is_empty()
            && id.len() < 60
            && ((named && !id.chars().any(|c| c.is_ascii_uppercase()))
                || id.chars().any(|c| c.is_ascii_digit() || c == '-'))
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._/:".contains(c));
        if !looks_like_model {
            continue;
        }
        let name = rest.trim_start_matches(['-', '–', '(', '|']).trim();
        // Cursor marks its picks as `sonnet-4.5 - Claude 4.5 Sonnet  (current, default)`.
        let name = match name.rsplit_once('(') {
            Some((before, mark))
                if mark.trim_end_matches(')').split(',').all(|w| {
                    matches!(
                        w.trim().to_lowercase().as_str(),
                        "current" | "default" | "selected"
                    )
                }) =>
            {
                before.trim()
            }
            _ => name.trim_end_matches(')').trim(),
        };
        // `grok models` marks its default as `* grok-4.7 (default)`, which is no name.
        let (label, desc) = if name.is_empty() || name.eq_ignore_ascii_case("default") {
            (id.to_string(), String::new())
        } else {
            (name.to_string(), id.to_string())
        };
        if !out.iter().any(|m: &ModelInfo| m.choice.value == id) {
            out.push(ModelInfo {
                choice: Choice {
                    value: id.to_string(),
                    label,
                    desc,
                },
                ..Default::default()
            });
        }
    }
    out
}

/// Models from a CLI's model table, whose header names its columns, such as Pi's
/// `provider  model  context …`: each row's model, as `provider/model` when the table
/// has a provider column.
pub(crate) fn parse_model_table(text: &str) -> Vec<ModelInfo> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let columns: Vec<&str> = header.split_whitespace().collect();
    let Some(model) = columns.iter().position(|c| *c == "model") else {
        return Vec::new();
    };
    let provider = columns.iter().position(|c| *c == "provider");
    let mut out: Vec<ModelInfo> = Vec::new();
    for line in lines {
        let cells: Vec<&str> = line.split_whitespace().collect();
        if cells.len() != columns.len() {
            continue;
        }
        let id = cells[model];
        let value = match provider {
            Some(p) => format!("{}/{id}", cells[p]),
            None => id.to_string(),
        };
        if out.iter().any(|m| m.choice.value == value) {
            continue;
        }
        out.push(ModelInfo {
            choice: Choice {
                label: id.to_string(),
                desc: provider.map(|p| cells[p].to_string()).unwrap_or_default(),
                value,
            },
            ..Default::default()
        });
    }
    out
}

/// `--model` choices listed in a CLI's help, like `(choices: "gpt-5", "claude-sonnet-4.5")`.
pub(crate) fn help_model_choices(help: &str) -> Vec<ModelInfo> {
    let mut block = String::new();
    let mut inside = false;
    for line in help.lines() {
        let t = line.trim_start();
        let flag = t.starts_with('-');
        if inside && flag {
            break;
        }
        if !inside && flag && t.contains("--model") {
            inside = true;
        }
        if inside {
            block.push_str(line);
            block.push(' ');
        }
    }
    let Some((_, list)) = block.split_once("choices:") else {
        return Vec::new();
    };
    let list = list.split(')').next().unwrap_or("");
    list.split(',')
        .map(|c| c.trim().trim_matches(['"', '\'']).trim())
        .filter(|c| !c.is_empty() && !c.contains(' '))
        .map(|id| ModelInfo {
            choice: Choice {
                value: id.to_string(),
                label: id.to_string(),
                desc: String::new(),
            },
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    #[test]
    fn ids_split_across_reads_are_found_whole() {
        let dir = std::env::temp_dir().join(format!("squadcode-ids-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bin");
        // An id that straddles the 4 MB read boundary, and one well inside.
        let id = b"claude-opus-15-25";
        let mut bytes = vec![0u8; (4 << 20) - 14];
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&[0u8; 100]);
        bytes.extend_from_slice(b" claude-sonnet-4-5 ");
        std::fs::write(&path, &bytes).unwrap();
        let found = claude_ids_in_file(&path).unwrap();
        assert_eq!(found, claude_ids_in(&bytes));
        assert!(found.iter().any(|f| f == "claude-opus-15-25"), "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claude_menu_shows_versions() {
        let command = "Current model: `Sonnet 5.5 (default)`\nUsage: /model <name>. Available: sonnet, opus, haiku, fable, best, sonnet[1m], opus[1m], fable[1m], opusplan, default, or a full model ID.";
        let (default, aliases) = parse_model_command(command);
        assert_eq!(default, "Sonnet 5.5");
        assert_eq!(aliases.len(), 9);
        let known = claude_ids_in(
            br#"x"claude-opus-5"y"claude-opus-4-8""claude-opus-5-5""claude-fable-5-1""claude-sonnet-5-5""claude-haiku-4-5-20251001""claude-haiku-5-5""claude-opus-4-0""claude-sonnet-4-6""#,
        );
        assert!(!known.iter().any(|k| k.contains("2025")));
        let (menu, default) = claude_menu(command, &[], &known, &["claude-opus-4-0".into()]);
        assert_eq!(default, "Sonnet 5.5");
        let labels: Vec<&str> = menu.iter().map(|m| m.choice.label.as_str()).collect();
        assert_eq!(labels[0], "Sonnet 5.5");
        assert_eq!(labels[1], "Opus 5.5");
        assert_eq!(labels[3], "Fable 5.1");
        assert!(labels.contains(&"Opus 5.5 · 1M context"));
        // Pinned versions: Opus newest three, plus 4.0 because it was used.
        let opus: Vec<&str> = menu
            .iter()
            .filter(|m| m.choice.value.starts_with("claude-opus"))
            .map(|m| m.choice.label.as_str())
            .collect();
        assert_eq!(opus, ["Opus 5.5", "Opus 5", "Opus 4.8", "Opus 4.0"]);
        assert_eq!(menu[0].choice.value, "sonnet");
    }

    #[test]
    fn other_cli_model_lists() {
        let list = "Available models:\n  gpt-5.5 - GPT-5.5\n  sonnet-5.5-thinking  Claude Sonnet 5.5 Thinking\n  auto\n";
        let models = parse_model_list(list);
        let ids: Vec<&str> = models.iter().map(|m| m.choice.value.as_str()).collect();
        assert_eq!(ids, ["gpt-5.5", "sonnet-5.5-thinking"]);
        assert_eq!(models[0].choice.label, "GPT-5.5");
        // Exactly what `grok models` (xAI's Grok Build 1.0.70) prints, signed in.
        let grok = "You are logged in with grok.com.\n\nDefault model: grok-4.7\n\nAvailable models:\n  * grok-4.7 (default)\n  - grok-4.7-build-fast\n  - grok-4.6\n  - grok-4.5\n";
        let models = parse_model_list(grok);
        let ids: Vec<&str> = models.iter().map(|m| m.choice.value.as_str()).collect();
        assert_eq!(
            ids,
            ["grok-4.7", "grok-4.7-build-fast", "grok-4.6", "grok-4.5"]
        );
        assert_eq!(models[0].choice.label, "grok-4.7");
        // What `agent models` (Cursor) prints: an id, " - ", a name, and its picks marked.
        let cursor = "Available models\n\nauto - Auto\ncheetah - Cheetah\nsonnet-4.5 - Claude 4.5 Sonnet  (current, default)\ngpt-5 - GPT-5 (default)\n\nTip: use --model <id> (or /model <id> in interactive mode) to switch.\n";
        let models = parse_model_list(cursor);
        let got: Vec<(&str, &str)> = models
            .iter()
            .map(|m| (m.choice.value.as_str(), m.choice.label.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("auto", "Auto"),
                ("cheetah", "Cheetah"),
                ("sonnet-4.5", "Claude 4.5 Sonnet"),
                ("gpt-5", "GPT-5")
            ]
        );
        let help = "  --model <model>  Set the AI model (choices: \"claude-sonnet-4.5\", \"gpt-5\",\n                   \"gemini-3-pro\")\n  --other";
        let ids: Vec<String> = help_model_choices(help)
            .into_iter()
            .map(|m| m.choice.value)
            .collect();
        assert_eq!(ids, ["claude-sonnet-4.5", "gpt-5", "gemini-3-pro"]);
    }
}
