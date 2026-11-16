//! A review guide for each finished job: a short tour of the change in the
//! order to read it, with "look closely" stops at the parts that need a
//! careful eye before pressing Approve.
//!
//! It's written once Checker's run of the checks is back, so the guide can
//! say what passed and what didn't. Guard writes it when the change touches
//! something risky (sign-in, payments, secrets, data); otherwise Explainer
//! does, in plain words for people who may not code. Peek's engine runs it,
//! in the background, as it does thread summaries. The files a stop points
//! at come from Squad Code's own list of what changed, so a stop can't send
//! you to a file the job never touched.

use serde::{Deserialize, Serialize};

use super::super::data::Member;
use super::super::store::Changed;
use crate::proof::CheckRun;

/// The most stops a guide keeps.
const MOST_STOPS: usize = 12;
/// The most diff lines of one file the writer reads.
const FILE_LINES: usize = 220;
/// The most diff the writer reads in all, in characters.
const DIFF_CHARS: usize = 24_000;
/// The most characters of one stop's words, and of the opening.
const NOTE_CHARS: usize = 400;

/// A finished job's review guide.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Guide {
    /// The bot that wrote it.
    pub by: Option<Member>,
    /// What the change does, in a sentence or two.
    pub intro: String,
    /// The stops, in reading order.
    pub stops: Vec<Stop>,
    /// When it was written.
    pub at: u64,
}

/// One stop on the tour.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stop {
    /// The changed file it's about; empty for a stop about the whole change.
    pub file: String,
    /// Where in the file, such as "40-62"; may be empty.
    pub lines: String,
    /// A short name for the stop.
    pub title: String,
    /// What this part does, in plain words.
    pub note: String,
    /// Why to look closely; empty when it reads as routine.
    pub look: String,
}

impl Stop {
    pub fn closely(&self) -> bool {
        !self.look.trim().is_empty()
    }
}

impl Guide {
    /// The stops marked to look closely at.
    pub fn closely(&self) -> usize {
        self.stops.iter().filter(|s| s.closely()).count()
    }

    /// One line on it for the side panel: "6 stops, 2 to look closely at".
    pub fn line(&self) -> String {
        let n = self.stops.len();
        let look = self.closely();
        let stops = format!("{n} stop{}", if n == 1 { "" } else { "s" });
        match look {
            0 => format!("{stops}, nothing flagged"),
            k => format!("{stops}, {k} to look closely at"),
        }
    }
}

/// Words in a path that mean the change needs Guard's eye.
const RISKY: &[&str] = &[
    "auth",
    "login",
    "signin",
    "session",
    "password",
    "passwd",
    "token",
    "secret",
    "apikey",
    "api_key",
    "cred",
    "oauth",
    "jwt",
    "payment",
    "billing",
    "stripe",
    "checkout",
    "invoice",
    "charge",
    "migration",
    "migrate",
    "schema",
    "permission",
    "role",
    "admin",
    "crypto",
    "encrypt",
    ".env",
];

/// Whether what changed touches something risky, going by the files' paths.
pub fn risky(changes: &[Changed]) -> bool {
    changes.iter().any(|c| {
        let p = c.path.to_lowercase();
        RISKY.iter().any(|w| p.contains(w))
    })
}

/// Who writes the guide: Guard for a risky change, Explainer otherwise, the
/// other one when the first isn't on the crew, and Peek when neither is.
pub fn writer(changes: &[Changed], on_crew: impl Fn(Member) -> bool) -> Member {
    let order = if risky(changes) {
        [Member::Guard, Member::Explainer]
    } else {
        [Member::Explainer, Member::Guard]
    };
    order
        .into_iter()
        .find(|m| on_crew(*m))
        .unwrap_or(Member::Fin)
}

/// The checks, one line each, for the writer to read.
fn checks_lines(checks: &[CheckRun]) -> String {
    if checks.is_empty() {
        return "(no checks ran)".into();
    }
    checks
        .iter()
        .map(|c| {
            let how = if c.ok {
                "passed".to_string()
            } else if !c.infra.is_empty() {
                format!("couldn't run ({})", c.infra)
            } else {
                let tail: String = c
                    .tail
                    .lines()
                    .rev()
                    .take(6)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" | ");
                let tail: String = tail.chars().take(500).collect();
                format!("FAILED: {tail}")
            };
            format!("- {}: {how}", c.command)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the writer reads: the job, what Checker found, what changed and each
/// file's diff (cut short when long). `diffs` holds (path, diff).
pub fn prompt(
    who: Member,
    task: &str,
    bot: &str,
    checks: &[CheckRun],
    changes: &[Changed],
    diffs: &[(String, String)],
) -> String {
    let files = changes
        .iter()
        .map(|c| format!("- {} ({}, +{} -{})", c.path, c.status, c.added, c.removed))
        .collect::<Vec<_>>()
        .join("\n");
    let mut left = DIFF_CHARS;
    let mut shown = Vec::new();
    for (path, diff) in diffs {
        if left == 0 {
            shown.push(format!("--- {path}: (not shown, the change is long)"));
            continue;
        }
        let lines: Vec<&str> = diff.lines().collect();
        let mut text = lines
            .iter()
            .take(FILE_LINES)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        if lines.len() > FILE_LINES {
            text.push_str(&format!("\n… {} more lines", lines.len() - FILE_LINES));
        }
        let text: String = text.chars().take(left).collect();
        left = left.saturating_sub(text.chars().count());
        shown.push(format!("--- {path}\n{text}"));
    }
    let task: String = task.chars().take(1500).collect();
    let focus = if who == Member::Guard {
        "You are Guard, a bot in Squad Code's crew that keeps the team's rules. Look hardest at risks to sign-in, payments, data and secrets."
    } else {
        "You are Explainer, a bot in Squad Code's crew who explains code in plain words for people who may not code."
    };
    format!(
        "{focus}\n\n\
{bot} just finished a job in Squad Code and its change waits for the person to approve it. \
Write a review guide: a short tour of the change in the order a reviewer should read it, \
so someone who doesn't code can tell what it does and where to look closely before approving. \
Don't use any tools and don't change anything; just reply.\n\n\
What the person asked {bot} to do:\n{task}\n\n\
Checker's run of the project's checks:\n{checks}\n\n\
Files it changed:\n{files}\n\n\
The changes:\n{diffs}\n\n\
Reply with only one JSON object, no other words:\n\
{{\"intro\": \"what the change does, in one or two plain sentences\",\n \
\"stops\": [{{\"file\": \"a path from the list above, or empty for the change as a whole\", \
\"lines\": \"where in the new file, like 40-62, or empty\", \
\"title\": \"a short name for this part\", \
\"note\": \"what this part does and why, in one or two plain sentences\", \
\"look\": \"why to look closely here, or empty when it's routine\"}}]}}\n\n\
Put the stops in reading order: start where the change begins (what the person sees or what's called first) \
and follow it through, with tests last. Use 2 to {most} stops; group small related edits into one. \
Set \"look\" only where it earns a careful eye: a failing check, a risk to sign-in, payments, data or secrets, \
behaviour that changed for existing users, a guess the bot made about what was wanted, missing tests, \
or code that's hard to follow. Most stops should leave it empty. \
Only say what the changes show; never guess. Plain words, no markdown.",
        checks = checks_lines(checks),
        diffs = if shown.is_empty() {
            "(no diff could be read)".to_string()
        } else {
            shown.join("\n\n")
        },
        most = MOST_STOPS,
    )
}

fn cut(s: &str) -> String {
    let s = s.trim();
    let mut out: String = s.chars().take(NOTE_CHARS).collect();
    if out.len() < s.len() {
        out.push('…');
    }
    out
}

/// The engine's guide, read and checked: stops on files the job didn't
/// change are dropped, and so is a guide with nothing in it.
pub fn parse(reply: &str, changes: &[Changed], by: Member, at: u64) -> Option<Guide> {
    let reply = reply.replace("```json", "").replace("```", "");
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    if end <= start {
        return None;
    }
    let mut g: Guide = serde_json::from_str(&reply[start..=end]).ok()?;
    let known = |f: &str| changes.iter().any(|c| c.path == f);
    g.stops.retain_mut(|s| {
        s.file = s.file.trim().trim_start_matches("./").to_string();
        if !s.file.is_empty() && !known(&s.file) {
            // A model may give the end of the path; take it when it's clear.
            let ends: Vec<&Changed> = changes
                .iter()
                .filter(|c| c.path.ends_with(&format!("/{}", s.file)))
                .collect();
            match ends.as_slice() {
                [one] => s.file = one.path.clone(),
                _ => return false,
            }
        }
        s.title = cut(&s.title);
        s.note = cut(&s.note);
        s.look = cut(&s.look);
        s.lines = s.lines.trim().chars().take(24).collect();
        !(s.title.is_empty() && s.note.is_empty())
    });
    g.stops.truncate(MOST_STOPS);
    g.intro = cut(&g.intro);
    if g.stops.is_empty() {
        return None;
    }
    g.by = Some(by);
    g.at = at;
    Some(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(path: &str) -> Changed {
        Changed {
            path: path.into(),
            status: "M".into(),
            added: 3,
            removed: 1,
        }
    }

    #[test]
    fn guard_writes_it_for_risky_changes_and_explainer_otherwise() {
        let all = |_: Member| true;
        assert_eq!(writer(&[ch("src/auth/login.ts")], all), Member::Guard);
        assert_eq!(writer(&[ch("src/pages/about.tsx")], all), Member::Explainer);
        let no_guard = |m: Member| m != Member::Guard;
        assert_eq!(
            writer(&[ch("billing/stripe.py")], no_guard),
            Member::Explainer
        );
        assert_eq!(writer(&[ch("a.rs")], |_| false), Member::Fin);
    }

    #[test]
    fn the_prompt_carries_the_task_checks_files_and_diffs() {
        let checks = vec![
            CheckRun {
                command: "npm test".into(),
                ok: false,
                tail: "ok\nexpected 3, got 4".into(),
                ..Default::default()
            },
            CheckRun {
                command: "npm run lint".into(),
                ok: true,
                ..Default::default()
            },
        ];
        let p = prompt(
            Member::Explainer,
            "Add a coupon field to checkout",
            "Builder",
            &checks,
            &[ch("src/cart.ts")],
            &[("src/cart.ts".into(), "+ const coupon = 1;".into())],
        );
        for want in [
            "You are Explainer",
            "Add a coupon field to checkout",
            "npm test: FAILED: ok | expected 3, got 4",
            "npm run lint: passed",
            "- src/cart.ts (M, +3 -1)",
            "+ const coupon = 1;",
            "\"look\"",
        ] {
            assert!(p.contains(want), "{want}");
        }
        assert!(prompt(Member::Guard, "t", "Builder", &[], &[], &[]).contains("You are Guard"));
    }

    #[test]
    fn long_diffs_are_cut_short() {
        let big = "+ line\n".repeat(FILE_LINES + 50);
        let p = prompt(
            Member::Explainer,
            "t",
            "Builder",
            &[],
            &[ch("a.rs")],
            &[("a.rs".into(), big)],
        );
        assert!(p.contains("… 50 more lines"));
        let many: Vec<(String, String)> = (0..40)
            .map(|i| (format!("f{i}.rs"), "+ x\n".repeat(FILE_LINES)))
            .collect();
        let p = prompt(Member::Explainer, "t", "Builder", &[], &[], &many);
        assert!(p.contains("(not shown, the change is long)"));
    }

    #[test]
    fn parse_keeps_stops_on_changed_files_in_order() {
        let changes = [
            ch("src/cart.ts"),
            ch("src/api/coupons.ts"),
            ch("test/cart.test.ts"),
        ];
        let reply = r#"Here you go:
```json
{"intro": "Adds a coupon box to the cart.",
 "stops": [
  {"file": "src/cart.ts", "lines": "10-30", "title": "The coupon box", "note": "Shows a field.", "look": ""},
  {"file": "coupons.ts", "title": "Checking the code", "note": "Asks the server.", "look": "It trusts the price from the browser."},
  {"file": "src/nowhere.ts", "title": "Made up", "note": "Not changed."},
  {"file": "", "title": "Overall", "note": "Tests cover the happy path."}
 ]}
```"#;
        let g = parse(reply, &changes, Member::Guard, 7).unwrap();
        assert_eq!(g.intro, "Adds a coupon box to the cart.");
        assert_eq!(g.by, Some(Member::Guard));
        assert_eq!(g.at, 7);
        let files: Vec<&str> = g.stops.iter().map(|s| s.file.as_str()).collect();
        assert_eq!(files, vec!["src/cart.ts", "src/api/coupons.ts", ""]);
        assert_eq!(g.closely(), 1);
        assert_eq!(g.line(), "3 stops, 1 to look closely at");
    }

    #[test]
    fn parse_refuses_what_isnt_a_guide() {
        let changes = [ch("a.rs")];
        assert!(parse("I can't do that", &changes, Member::Explainer, 0).is_none());
        assert!(parse(
            r#"{"intro": "x", "stops": []}"#,
            &changes,
            Member::Explainer,
            0
        )
        .is_none());
        assert!(parse(
            r#"{"intro": "x", "stops": [{"file": "b.rs", "title": "t"}]}"#,
            &changes,
            Member::Explainer,
            0
        )
        .is_none());
    }

    #[test]
    fn a_line_for_the_side_panel() {
        let g = Guide {
            stops: vec![Stop::default(); 1],
            ..Default::default()
        };
        assert_eq!(g.line(), "1 stop, nothing flagged");
    }
}
