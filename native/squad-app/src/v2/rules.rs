//! A project's rules for parts of its code, kept next to that code.
//!
//! Two kinds of file:
//!
//! - `.squad/rules/*.md`, each with an optional `files:` pattern at the top:
//!
//!   ```markdown
//!   ---
//!   files: src/api/**, src/db/**/*.sql
//!   description: How we write API handlers
//!   ---
//!   Every handler checks the session first, with `requireUser()`.
//!   ```
//!
//!   A rule without `files:` is for the whole project. `globs:` (Cursor's
//!   word) and `fileMatchPattern:` (Kiro's) mean the same as `files:`, so
//!   rules written for those tools work here as they are.
//! - `AGENTS.md` in a subfolder, which is a rule for that folder's files. The
//!   one at the top of the project is the engines' own business, so it's left
//!   to them.
//!
//! A bot that changes files, and Guard when it reviews, is given in full the
//! rules for the files its job touches: the files its ask names, what it has
//! changed so far, and for a review the changes it was pointed at. Every other
//! rule is one line naming the rule and its files, so the prompt stays short
//! and a bot that wanders into those files knows to read it first.

use std::path::Path;

/// Where the rule files live, from the top of the project.
pub const DIR: &str = ".squad/rules";

/// Most text of one rule given in full; the rest is cut and the bot is told
/// where to read it whole.
const ONE: usize = 6_000;
/// Most text of all the rules given in full; rules past it are listed instead.
const ALL: usize = 16_000;
/// Most subfolder AGENTS.md files looked at.
const MOST_AGENTS: usize = 50;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rule {
    /// The file, from the top of the project (`.squad/rules/api.md`).
    pub path: String,
    /// The paths it's for; empty when it's for every file.
    pub files: Vec<String>,
    pub description: String,
    pub body: String,
}

impl Rule {
    /// Whether the rule is for every file, whatever the job touches.
    pub fn always(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether the rule is for `path` (from the top of the project).
    pub fn covers(&self, path: &str) -> bool {
        let path = path.trim_start_matches("./");
        self.files.iter().any(|g| glob(g, path))
    }

    /// Whether the rule is for anything in `touched`. A path named in an ask
    /// may be a folder ("src/api"), so files under it count too.
    fn applies(&self, touched: &[String]) -> bool {
        self.always()
            || touched.iter().any(|t| {
                let t = t.trim_end_matches('/');
                self.covers(t) || self.covers(&format!("{t}/x"))
            })
    }

    /// The rule's paths as words: "src/api/**, src/db/**".
    fn scope(&self) -> String {
        if self.always() {
            "every file".into()
        } else {
            self.files.join(", ")
        }
    }
}

/// The project's rules in `dir`: its `.squad/rules/*.md` by name, then the
/// AGENTS.md files in its subfolders.
pub fn load(dir: &str) -> Vec<Rule> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(Path::new(dir).join(DIR)) {
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| n.to_lowercase().ends_with(".md"))
            .collect();
        names.sort();
        for name in names {
            let rel = format!("{DIR}/{name}");
            if let Ok(text) = std::fs::read_to_string(Path::new(dir).join(&rel)) {
                out.push(parse(&rel, &text));
            }
        }
    }
    for rel in nested_agents(dir) {
        let Ok(text) = std::fs::read_to_string(Path::new(dir).join(&rel)) else {
            continue;
        };
        let folder = rel.trim_end_matches("AGENTS.md").trim_end_matches('/');
        out.push(Rule {
            path: rel.clone(),
            files: vec![format!("{folder}/**")],
            description: String::new(),
            body: text.trim().to_string(),
        });
    }
    out.retain(|r| !r.body.is_empty());
    out
}

/// The AGENTS.md files below the top of `dir`: the ones git tracks, else the
/// ones a short walk finds outside hidden and build folders.
fn nested_agents(dir: &str) -> Vec<String> {
    let mut found = crate::git::tracked_named(dir, "AGENTS.md").unwrap_or_else(|| {
        let mut found = Vec::new();
        walk(Path::new(dir), "", 0, &mut found);
        found
    });
    found.retain(|p| p.contains('/') && Path::new(dir).join(p).is_file());
    found.sort();
    found.truncate(MOST_AGENTS);
    found
}

fn walk(dir: &Path, rel: &str, depth: usize, found: &mut Vec<String>) {
    const SKIP: &[&str] = &["node_modules", "target", "vendor", "dist", "build"];
    if depth > 5 || found.len() >= MOST_AGENTS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let Ok(kind) = e.file_type() else {
            continue;
        };
        if kind.is_dir() && !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
            walk(&e.path(), &path, depth + 1, found);
        } else if kind.is_file() && name == "AGENTS.md" {
            found.push(path);
        }
    }
}

/// A rule file's front matter and body.
pub fn parse(path: &str, text: &str) -> Rule {
    let mut rule = Rule {
        path: path.to_string(),
        ..Default::default()
    };
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    let front =
        text.trim_start().starts_with("---") && lines.next().is_some_and(|l| l.trim() == "---");
    if !front {
        rule.body = text.trim().to_string();
        return rule;
    }
    let mut key = String::new();
    let mut ended = false;
    let mut body = Vec::new();
    for line in lines.by_ref() {
        if line.trim() == "---" {
            ended = true;
            break;
        }
        let t = line.trim();
        if let Some(item) = t.strip_prefix("- ") {
            // A list under the last key.
            if is_files(&key) {
                rule.files.extend(patterns(item));
            }
            continue;
        }
        let Some((k, v)) = t.split_once(':') else {
            continue;
        };
        key = k.trim().to_string();
        let v = v.trim();
        if is_files(&key) {
            rule.files.extend(patterns(v));
        } else if key == "description" {
            rule.description = unquote(v).to_string();
        }
    }
    if !ended {
        // No closing line: it wasn't front matter after all.
        return Rule {
            path: path.to_string(),
            body: text.trim().to_string(),
            ..Default::default()
        };
    }
    body.extend(lines);
    rule.body = body.join("\n").trim().to_string();
    rule.files.dedup();
    rule
}

fn is_files(key: &str) -> bool {
    matches!(key, "files" | "globs" | "fileMatchPattern" | "paths")
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(s)
}

/// The patterns in a `files:` value: one, a comma-separated line, or a
/// `[a, b]` list, each maybe quoted. Commas inside `{a,b}` stay put.
fn patterns(v: &str) -> Vec<String> {
    let v = v.trim();
    let v = v
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(v);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for c in v.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out.into_iter()
        .map(|p| unquote(&p).trim().trim_start_matches("./").to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Whether `path` matches the glob `pattern`: `*` is any run of characters
/// in one folder, `**` any number of folders, `?` one character, and `{a,b}`
/// either. A pattern without a `/` matches a file's name in any folder, as in
/// .gitignore, and one ending in `/` everything under that folder.
pub fn glob(pattern: &str, path: &str) -> bool {
    let p = pattern.trim().trim_start_matches("./");
    let p = if p.ends_with('/') {
        format!("{p}**")
    } else if !p.contains('/') {
        format!("**/{p}")
    } else {
        p.trim_start_matches('/').to_string()
    };
    expand(&p)
        .iter()
        .any(|p| matches(p.as_bytes(), path.as_bytes()))
}

/// A pattern with its `{a,b}` choices spelled out, one pattern each.
fn expand(p: &str) -> Vec<String> {
    let Some(open) = p.find('{') else {
        return vec![p.to_string()];
    };
    let Some(len) = p[open..].find('}') else {
        return vec![p.to_string()];
    };
    let close = open + len;
    let (head, tail) = (&p[..open], &p[close + 1..]);
    p[open + 1..close]
        .split(',')
        .flat_map(|alt| expand(&format!("{head}{alt}{tail}")))
        .collect()
}

fn matches(p: &[u8], s: &[u8]) -> bool {
    match p {
        [] => s.is_empty(),
        [b'*', b'*', rest @ ..] => {
            let rest = rest.strip_prefix(b"/").unwrap_or(rest);
            if rest.is_empty() {
                return true;
            }
            // Any number of whole folders, including none.
            (0..=s.len())
                .filter(|&i| i == 0 || s[i - 1] == b'/')
                .any(|i| matches(rest, &s[i..]))
        }
        [b'*', rest @ ..] => (0..=s.len())
            .take_while(|&i| i == 0 || s[i - 1] != b'/')
            .any(|i| matches(rest, &s[i..])),
        [b'?', rest @ ..] => matches!(s, [c, ..] if *c != b'/') && matches(rest, &s[1..]),
        [c, rest @ ..] => matches!(s, [d, ..] if d == c) && matches(rest, &s[1..]),
    }
}

/// The paths an ask names: words with a `/` or a file's extension, like
/// `src/api/users.ts` or `billing/`.
pub fn named_paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .split(|c: char| c.is_whitespace() || "`'\"()[]<>,;".contains(c))
        .map(|w| w.trim_end_matches(['.', ':', '!', '?']))
        .map(|w| w.trim_start_matches("./"))
        .filter(|w| !w.is_empty() && !w.contains("://") && !w.starts_with('@'))
        .filter(|w| {
            w.contains('/')
                || w.rsplit_once('.').is_some_and(|(stem, ext)| {
                    !stem.is_empty()
                        && (1..=5).contains(&ext.len())
                        && ext.chars().all(|c| c.is_ascii_alphanumeric())
                        && ext.chars().any(|c| c.is_ascii_alphabetic())
                })
        })
        .map(str::to_string)
        .collect();
    out.dedup();
    out
}

/// The rules among `rules` that are for `touched`, in order.
pub fn for_paths<'a>(rules: &'a [Rule], touched: &[String]) -> Vec<&'a Rule> {
    rules.iter().filter(|r| r.applies(touched)).collect()
}

/// What a bot is told about the project's rules: the ones for `touched` in
/// full, and a line for each of the others. Empty when the project has none.
pub fn brief(rules: &[Rule], touched: &[String]) -> String {
    if rules.is_empty() {
        return String::new();
    }
    let mut full = String::new();
    let mut left = ALL;
    let mut listed: Vec<&Rule> = Vec::new();
    for r in rules {
        if !r.applies(touched) {
            listed.push(r);
            continue;
        }
        let body = cut(&r.body, ONE);
        if body.len() > left {
            listed.push(r);
            continue;
        }
        left -= body.len();
        let more = if body.len() < r.body.len() {
            format!("\n(Cut short: read {} for the rest.)", r.path)
        } else {
            String::new()
        };
        full.push_str(&format!(
            "From {} (for {}):\n{body}{more}\n\n",
            r.path,
            r.scope()
        ));
    }
    let mut out = String::new();
    if !full.is_empty() {
        out.push_str("The project's rules for the files this job touches. Follow them:\n\n");
        out.push_str(&full);
    }
    if !listed.is_empty() {
        out.push_str(
            "The project's other rules, for other files. Before you change a file one of these is for, read the rule and follow it:\n",
        );
        for r in listed {
            out.push_str(&format!("- {} (for {})", r.path, r.scope()));
            if !r.description.trim().is_empty() {
                out.push_str(&format!(": {}", r.description.trim()));
            }
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

/// At most `n` bytes of `text`, cut at a line where it can be.
fn cut(text: &str, n: usize) -> &str {
    if text.len() <= n {
        return text;
    }
    let mut end = n;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = &text[..end];
    head.rfind('\n').map(|i| &head[..i]).unwrap_or(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(files: &[&str], body: &str) -> Rule {
        Rule {
            path: ".squad/rules/r.md".into(),
            files: files.iter().map(|f| f.to_string()).collect(),
            description: String::new(),
            body: body.into(),
        }
    }

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn globs_match_like_cursor_and_gitignore() {
        assert!(glob("src/api/**", "src/api/users.ts"));
        assert!(glob("src/api/**", "src/api/v2/users.ts"));
        assert!(!glob("src/api/**", "src/apix/users.ts"));
        assert!(glob("src/**/*.sql", "src/db/m/001.sql"));
        assert!(glob("src/**/*.sql", "src/001.sql"));
        assert!(!glob("src/*.sql", "src/db/001.sql"));
        assert!(glob("*.tsx", "web/components/Button.tsx"));
        assert!(glob("web/", "web/a/b.css"));
        assert!(glob("**/*.{ts,tsx}", "a/b.tsx"));
        assert!(!glob("**/*.{ts,tsx}", "a/b.js"));
        assert!(glob("./docs/?.md", "docs/a.md"));
        assert!(!glob("docs/?.md", "docs/ab.md"));
        assert!(glob("migrations/**", "migrations/001.sql"));
    }

    #[test]
    fn front_matter_takes_each_tools_spelling() {
        let r = parse(
            ".squad/rules/api.md",
            "---\nfiles: src/api/**, \"src/db/**/*.{sql,ts}\"\ndescription: API handlers\n---\n\nCheck the session first.\n",
        );
        assert_eq!(r.files, vec!["src/api/**", "src/db/**/*.{sql,ts}"]);
        assert_eq!(r.description, "API handlers");
        assert_eq!(r.body, "Check the session first.");

        let cursor = parse(
            "x.md",
            "---\nglobs: [\"*.tsx\", web/**]\nalwaysApply: false\n---\nUse tokens.",
        );
        assert_eq!(cursor.files, vec!["*.tsx", "web/**"]);

        let kiro = parse(
            "x.md",
            "---\ninclusion: fileMatch\nfileMatchPattern:\n  - 'components/**'\n  - hooks/*.ts\n---\nHooks first.",
        );
        assert_eq!(kiro.files, vec!["components/**", "hooks/*.ts"]);

        let plain = parse("x.md", "# Always\nBe kind.");
        assert!(plain.always());
        assert_eq!(plain.body, "# Always\nBe kind.");

        let unclosed = parse("x.md", "---\nnot front matter");
        assert!(unclosed.always() && unclosed.body.contains("not front matter"));
    }

    #[test]
    fn asks_name_paths_and_folders() {
        assert_eq!(
            named_paths(
                "Fix the redirect in `src/api/login.ts`, and tidy billing/. See https://x.io/a."
            ),
            vec!["src/api/login.ts", "billing/"]
        );
        assert_eq!(
            named_paths("bump to 2.0 and fix README.md!"),
            vec!["README.md"]
        );
        assert!(named_paths("add a dark mode toggle").is_empty());
    }

    #[test]
    fn only_the_rules_for_touched_files_are_given_in_full() {
        let rules = vec![
            rule(&[], "Keep commits small."),
            Rule {
                path: ".squad/rules/api.md".into(),
                description: "API handlers".into(),
                ..rule(&["src/api/**"], "Check the session first.")
            },
            Rule {
                path: "web/AGENTS.md".into(),
                ..rule(&["web/**"], "Use the design tokens.")
            },
        ];
        let b = brief(&rules, &paths(&["src/api/users.ts"]));
        assert!(b.contains("Keep commits small."), "{b}");
        assert!(b.contains("Check the session first."), "{b}");
        assert!(!b.contains("Use the design tokens."), "{b}");
        assert!(b.contains("- web/AGENTS.md (for web/**)"), "{b}");

        // A folder named in the ask counts for the files in it.
        let b = brief(&rules, &paths(&["web"]));
        assert!(b.contains("Use the design tokens."), "{b}");
        assert!(
            b.contains("- .squad/rules/api.md (for src/api/**): API handlers"),
            "{b}"
        );

        assert_eq!(brief(&[], &paths(&["a.rs"])), "");
    }

    #[test]
    fn long_rules_are_cut_and_too_many_are_listed() {
        let long = "line\n".repeat(3_000);
        let rules: Vec<Rule> = (0..5).map(|_| rule(&[], &long)).collect();
        let b = brief(&rules, &[]);
        assert!(b.contains("Cut short: read .squad/rules/r.md"));
        assert!(b.len() < ALL + 2_000, "{}", b.len());
        assert!(b.contains("The project's other rules"));
    }

    #[test]
    fn rule_files_and_nested_agents_load_from_the_folder() {
        let dir = std::env::temp_dir().join(format!("squad-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(DIR)).unwrap();
        std::fs::create_dir_all(dir.join("web/ui")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::write(
            dir.join(DIR).join("b-api.md"),
            "---\nfiles: src/api/**\n---\nCheck the session.",
        )
        .unwrap();
        std::fs::write(dir.join(DIR).join("a-all.md"), "Small commits.").unwrap();
        std::fs::write(dir.join(DIR).join("notes.txt"), "not a rule").unwrap();
        std::fs::write(dir.join(DIR).join("empty.md"), "---\nfiles: x/**\n---\n").unwrap();
        std::fs::write(dir.join("AGENTS.md"), "Top-level, the engine's.").unwrap();
        std::fs::write(dir.join("web/ui/AGENTS.md"), "Use tokens.").unwrap();
        std::fs::write(dir.join("node_modules/x/AGENTS.md"), "Not ours.").unwrap();
        let rules = load(dir.to_str().unwrap());
        let got: Vec<(&str, Vec<String>)> = rules
            .iter()
            .map(|r| (r.path.as_str(), r.files.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (".squad/rules/a-all.md", vec![]),
                (".squad/rules/b-api.md", paths(&["src/api/**"])),
                ("web/ui/AGENTS.md", paths(&["web/ui/**"])),
            ]
        );
        assert!(rules[2].covers("web/ui/Button.tsx") && !rules[2].covers("web/app.ts"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn in_a_repository_ignored_agents_files_dont_count() {
        let dir = std::env::temp_dir().join(format!("squad-rules-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("api")).unwrap();
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::fs::write(dir.join("api/AGENTS.md"), "Check the session.").unwrap();
        std::fs::write(dir.join("out/AGENTS.md"), "Built, not ours.").unwrap();
        std::fs::write(dir.join(".gitignore"), "out/\n").unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        let rules = load(dir.to_str().unwrap());
        let got: Vec<&str> = rules.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(got, vec!["api/AGENTS.md"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
