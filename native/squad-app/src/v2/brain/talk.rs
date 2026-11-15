//! Bots talking to each other, in the thread, like teammates in a channel.
//!
//! A bot @mentions another at the end of its reply (`@Guard: is this safe to
//! ship?`). Squad Code posts that in the thread, where the person sees it,
//! and hands it to that bot: as the next turn of its job in the thread, or as
//! a new job when it has none. Its answer can @mention back, so two bots can
//! ask, answer and push back on each other.
//!
//! Peek leads that talk. Each message one bot sends another goes past Peek
//! first ([`prompt`], [`parse`]): it passes it on, maybe with a short note
//! to keep the other bot on track, gives it to a bot whose role fits
//! better, or stops the talk and tells the person where the bots got to and
//! what it needs from them. A message that repeats an earlier one stops the
//! talk without asking ([`repeats`]). After [`HOPS`] messages between bots
//! with no word from the person, Peek sums up and hands it to the person.

use serde_json::Value;

use super::super::data::Member;

/// Messages between bots in a thread before the person is asked to step in.
pub const HOPS: u32 = 6;

/// The @mentions at the start of lines in a bot's reply: who, and what it
/// says to them. A bot never mentions itself, and Peek isn't messaged.
pub fn mentions(reply: &str, from: Member, crew: &[Member]) -> Vec<(Member, String)> {
    let mut out: Vec<(Member, String)> = Vec::new();
    for line in reply.lines() {
        let line = line.trim().trim_start_matches(['*', '-', '>', ' ']);
        let Some(rest) = line.strip_prefix('@') else {
            continue;
        };
        let Some(who) = crew
            .iter()
            .copied()
            .filter(|m| *m != from && *m != Member::Fin)
            // The longest name first, so "@Builder2" isn't taken for Builder.
            .filter(|m| {
                let n = m.name();
                rest.get(..n.len())
                    .is_some_and(|h| h.eq_ignore_ascii_case(n))
                    && !rest
                        .get(n.len()..)
                        .and_then(|t| t.chars().next())
                        .is_some_and(|c| c.is_alphanumeric())
            })
            .max_by_key(|m| m.name().len())
        else {
            continue;
        };
        let said = rest
            .get(who.name().len()..)
            .unwrap_or("")
            .trim_start_matches([':', ',', '*'])
            .trim()
            .to_string();
        if said.is_empty() {
            continue;
        }
        match out.iter_mut().find(|(m, _)| *m == who) {
            Some((_, s)) => {
                s.push('\n');
                s.push_str(&said);
            }
            None => out.push((who, said)),
        }
    }
    out
}

/// What a bot is told about its crew and how to talk to them.
pub fn crew_rule(me: Member, crew: &[Member]) -> String {
    let others: Vec<String> = crew
        .iter()
        .filter(|m| **m != me && **m != Member::Fin)
        .map(|m| format!("{} ({})", m.name(), m.role().trim_end_matches('.')))
        .collect();
    if others.is_empty() {
        return String::new();
    }
    format!(
        " You work in a shared thread with the rest of the crew: {}. To ask or tell one of them something, end your reply with a line like \"@Guard: is the login safe to ship?\"; they'll answer in the thread, and you'll get their answer. Do it when the person asks for it or when their role would really help, not by habit. When another bot's point is wrong, push back plainly with your reasons; when it's right, say so and act on it. \
Peek leads the crew and reads every message between bots: it may pass yours on with a note, give it to a bot that fits better, or stop the talk and ask the person. \
So ask one clear thing at a time, and once a question is settled, stop: don't @mention a bot only to agree or say thanks.",
        others.join(", ")
    )
}

/// What a bot gets when another bot @mentions it: the line meant for it,
/// and the whole reply it ends, since that line often points back at it
/// ("can you fix items 1 and 2?").
/// `note` is Peek's word to keep the bot on track; empty when it has none.
pub fn relay_text(from: Member, from_job: u64, said: &str, whole: &str, note: &str) -> String {
    let whole = whole.trim();
    let context = if whole.is_empty() || whole == said.trim() {
        String::new()
    } else {
        format!(
            "Their whole message, for what it refers to:\n\n<message from=\"{}\">\n{whole}\n</message>\n\n",
            from.name()
        )
    };
    let note = note.trim();
    let note = if note.is_empty() {
        String::new()
    } else {
        format!("Peek, who leads the crew, adds: {note}\n\n")
    };
    format!(
        "{} (another bot on your crew, working on #{from_job}) says to you in the thread:\n\n{said}\n\n{context}{note}Answer them. If you need to reply to {} directly, end with a line starting \"@{}:\". Push back if you disagree.",
        from.name(),
        from.name(),
        from.name()
    )
}

/// What Peek does with one bot's message to another.
#[derive(Clone, Debug, PartialEq)]
pub enum Steer {
    /// It goes on to `to`: the bot it was for, or one whose role fits better.
    /// `note` goes with it, to keep that bot on track; `say` is Peek's line
    /// in the thread, when it gave it to another bot.
    Go {
        to: Member,
        note: String,
        say: String,
    },
    /// The talk stops here: `say` tells the person where the bots got to and
    /// what Peek needs from them, with `choices` as buttons.
    Stop { say: String, choices: Vec<String> },
}

/// What Peek knows when one bot messages another.
pub struct Moment<'a> {
    /// The project's name.
    pub project: &'a str,
    /// The thread's name, without the #.
    pub thread: &'a str,
    /// The thread's running summary; empty when it has none yet.
    pub summary: &'a str,
    /// The last thing the person said in the thread: what the talk is for.
    pub asked: &'a str,
    /// What was said in the thread since then, oldest first, as (who, words).
    pub since: &'a [(String, String)],
    pub from: Member,
    pub to: Member,
    /// What `from` says to `to`.
    pub said: &'a str,
    /// The bots on the crew.
    pub crew: &'a [Member],
    /// Messages between bots since the person last spoke.
    pub hops: u32,
    /// The talk has reached [`HOPS`]: Peek must stop it and sum up.
    pub last: bool,
}

fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "\u{2026}"
    }
}

/// What Peek's engine reads to decide what happens to one bot's message to
/// another.
pub fn prompt(m: &Moment) -> String {
    let crew: Vec<String> = m
        .crew
        .iter()
        .filter(|b| **b != Member::Fin)
        .map(|b| {
            format!(
                "- {} ({} work): {}",
                b.name(),
                b.mode().label().to_lowercase(),
                b.role()
            )
        })
        .collect();
    let shown = m.since.len().saturating_sub(10);
    let since: Vec<String> = m.since[shown..]
        .iter()
        .map(|(who, w)| format!("{who}: {}", clip(&w.replace('\n', " "), 500)))
        .collect();
    let summary = if m.summary.trim().is_empty() {
        String::new()
    } else {
        format!("What the thread is about so far:\n{}\n", m.summary.trim())
    };
    let last = if m.last {
        format!(
            "\nThe bots have now sent each other {HOPS} messages without the person, the most allowed, so you must \"stop\" and sum up for the person.\n"
        )
    } else {
        String::new()
    };
    format!(
        "You are Peek, who leads a crew of AI bots in Squad Code. The bots talk to each other in the person's thread by @mentioning each other, \
and every message between bots comes to you first: you keep that talk on track, like a good lead in a team channel. Don't use any tools; just reply.\n\n\
Project: {project}\n\
Thread: #{thread}\n\
{summary}\
The crew:\n{crew}\n\n\
What the person last asked in the thread: \"{asked}\"\n\
Said in the thread since then, oldest first:\n{since}\n\n\
Now {from} wants to send {to} this:\n\"{said}\"\n\
Messages between bots so far: {hops} of at most {HOPS}.\n{last}\n\
Decide what happens to this message:\n\
- \"pass\": it moves the person's ask forward, so it goes on. Leave \"to\" as {to}, or name another bot when the ask is for a different role \
(a security question is for the bot that reviews, a bug for the one that fixes), and then say why in \"say\", in one short sentence. \
Most messages need no \"note\": leave it empty. Add one, a single short sentence, only when the bot would likely go off track without it, like \"Just the cookie question; Builder makes the change.\"\n\
- \"stop\": the talk has done its job, is going round in circles, has drifted from what the person asked, or needs the person to decide \
(a choice between options, anything risky, costly or hard to undo). Write \"say\" for the person in two or three short sentences: where the bots got to, \
then what you need from them. Offer up to 3 short \"choices\" when it's a choice.\n\
Pass while the bots are making progress on what the person asked. Stop rather than let them repeat themselves, thank or agree back and forth, or start work the person didn't ask for. \
Before passing, check the bot it's for fits the ask; if another bot's role fits it better, name that bot.\n\n\
Reply with JSON only, on one line:\n\
{{\"do\": \"pass|stop\", \"to\": \"a bot's name\", \"note\": \"\", \"say\": \"\", \"choices\": []}}\n\
Keep \"say\" and \"note\" warm, plain and short, no markdown.",
        project = m.project,
        thread = if m.thread.is_empty() { "general" } else { m.thread },
        crew = crew.join("\n"),
        asked = clip(m.asked, 600).replace('"', "'"),
        since = if since.is_empty() {
            "(nothing)".to_string()
        } else {
            since.join("\n")
        },
        from = m.from.name(),
        to = m.to.name(),
        said = clip(m.said, 1200).replace('"', "'"),
        hops = m.hops,
    )
}

/// Peek's decision as Squad Code acts on it. None when the reply holds no
/// decision that makes sense, or passes a message on when the talk must
/// stop: the message then goes on as it was, or the talk stops with Squad
/// Code's own note.
pub fn parse(reply: &str, m: &Moment) -> Option<Steer> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let v: Value = serde_json::from_str(reply.get(start..=end)?).ok()?;
    let text = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    match v["do"].as_str()?.trim() {
        "stop" => {
            let say = text("say");
            if say.is_empty() {
                return None;
            }
            let choices = v["choices"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str())
                        .map(|c| c.trim().to_string())
                        .filter(|c| !c.is_empty() && c.chars().count() <= 60)
                        .take(3)
                        .collect()
                })
                .unwrap_or_default();
            Some(Steer::Stop { say, choices })
        }
        "pass" if !m.last => {
            let named = text("to").trim_start_matches('@').to_lowercase();
            // Only a bot on the crew, never Peek or the bot sending it.
            let to = m
                .crew
                .iter()
                .copied()
                .find(|b| *b != Member::Fin && *b != m.from && b.name().to_lowercase() == named)
                .unwrap_or(m.to);
            Some(Steer::Go {
                to,
                note: text("note"),
                say: if to == m.to {
                    String::new()
                } else {
                    text("say")
                },
            })
        }
        _ => None,
    }
}

/// Whether `said` says again what one of `earlier` said: most of its words
/// are the same. Bots doing that are going round in circles.
pub fn repeats(said: &str, earlier: &[String]) -> bool {
    let set = |s: &str| -> std::collections::HashSet<String> {
        super::words(s)
            .into_iter()
            .filter(|w| w.len() > 2)
            .collect()
    };
    let now = set(said);
    if now.len() < 3 {
        return false;
    }
    earlier.iter().any(|e| {
        let then = set(e);
        let both = now.intersection(&then).count();
        let either = now.union(&then).count();
        either > 0 && both * 10 >= either * 8
    })
}

/// Whether `said` is only a courtesy, like "Great, thanks!" or "Looks good
/// to me": no question and nothing asked of the other bot. Peek doesn't
/// pass it on, so two bots never thank each other back and forth.
pub fn courtesy(said: &str) -> bool {
    if said.contains('?') {
        return false;
    }
    let w = super::words(said);
    if w.is_empty() || w.len() > 10 {
        return false;
    }
    let text = format!(" {} ", w.join(" "));
    let kind = [
        " thanks ",
        " thank ",
        " thx ",
        " ty ",
        " great ",
        " perfect ",
        " nice ",
        " awesome ",
        " cheers ",
        " looks good ",
        " sounds good ",
        " lgtm ",
        " agreed ",
        " got it ",
        " noted ",
        " will do ",
        " good work ",
        " well done ",
        " appreciate ",
    ];
    // Anything asked or still to do makes it more than a courtesy.
    let asks = [
        " please ",
        " can ",
        " could ",
        " would ",
        " also ",
        " need ",
        " needs ",
        " must ",
        " should ",
        " add ",
        " fix ",
        " change ",
        " check ",
        " but ",
        " however ",
        " still ",
        " missing ",
        " wrong ",
        " instead ",
        " don't ",
        " not ",
        " let ",
        " make ",
    ];
    kind.iter().any(|k| text.contains(k)) && !asks.iter().any(|a| text.contains(a))
}

/// What Peek says when a bot asks another for something that needs the
/// person's yes first, like a deploy or a delete. `why` is the guard's reason.
pub fn needs_yes(from: Member, to: Member, said: &str, why: &str) -> String {
    format!(
        "{} asked {} to {}. {why} Say \"@{} go ahead\" if you want it done.",
        from.name(),
        to.name(),
        clip(said.trim_end_matches(['.', '!']), 160)
            .trim_start_matches(|c: char| c.is_whitespace()),
        to.name()
    )
}

/// What Peek says when the bots repeat themselves and it stops them without
/// asking its engine.
pub fn circling(from: Member, to: Member) -> String {
    format!(
        "{} and {} are going round in circles, so I've stopped them here. Tell me which way to go, or say \"@{} go ahead\" to let {} carry on.",
        from.name(),
        to.name(),
        from.name(),
        from.name()
    )
}

/// What Peek says when the bots reach [`HOPS`] and its engine couldn't sum
/// up.
pub fn limit(from: Member, to: Member) -> String {
    format!(
        "The bots have messaged each other {HOPS} times without you, so they've stopped here. {} was about to ask {}. Tell one of them what to do next, like \"@{} go ahead\", and they'll carry on.",
        from.name(),
        to.name(),
        from.name(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_at_the_start_of_a_line_go_to_that_bot() {
        let crew = Member::CREW.to_vec();
        let reply = "Done, the login page works.\n\n@Guard: can you check the session cookie is safe?\n@guard also the CSRF token\n@Builder: talking to myself\nemail me @Fixer later";
        let m = mentions(reply, Member::Builder, &crew);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].0, Member::Guard);
        assert!(m[0].1.contains("session cookie") && m[0].1.contains("CSRF"));
    }

    #[test]
    fn peek_and_empty_mentions_are_ignored() {
        let crew = Member::CREW.to_vec();
        assert!(mentions("@Peek: hi\n@Fixer:", Member::Builder, &crew).is_empty());
        assert!(mentions("@Fixers: hi", Member::Builder, &crew).is_empty());
    }

    #[test]
    fn a_bot_hears_who_is_on_its_crew() {
        let r = crew_rule(Member::Builder, &Member::CREW);
        assert!(r.contains("Guard") && r.contains("@Guard:") && !r.contains("Builder ("));
        assert!(
            r.contains("Peek leads the crew") && r.contains("don't @mention a bot only to agree")
        );
        let t = relay_text(Member::Guard, 4, "The cookie isn't httpOnly.", "", "");
        assert!(t.contains("#4") && t.contains("@Guard:") && !t.contains("<message"));
        assert!(!t.contains("Peek"));
        let whole = "1. The cookie isn't httpOnly.\n2. No rate limit.\n\n@Builder: can you fix items 1 and 2?";
        let t = relay_text(
            Member::Guard,
            4,
            "can you fix items 1 and 2?",
            whole,
            "Only item 1 for now.",
        );
        assert!(t.contains("No rate limit") && t.contains("can you fix items 1 and 2?"));
        assert!(t.contains("Peek, who leads the crew, adds: Only item 1 for now."));
    }

    fn moment<'a>(since: &'a [(String, String)], crew: &'a [Member], last: bool) -> Moment<'a> {
        Moment {
            project: "shop",
            thread: "login",
            summary: "",
            asked: "build the login page and have Guard check it",
            since,
            from: Member::Builder,
            to: Member::Guard,
            said: "is the session cookie safe to ship?",
            crew,
            hops: if last { HOPS } else { 1 },
            last,
        }
    }

    #[test]
    fn peek_reads_the_talk_and_what_the_person_asked() {
        let crew = Member::CREW.to_vec();
        let since = vec![("builder".to_string(), "The login page works.".to_string())];
        let p = prompt(&moment(&since, &crew, false));
        for want in [
            "Thread: #login",
            "build the login page and have Guard check it",
            "builder: The login page works.",
            "Now Builder wants to send Guard this",
            "is the session cookie safe to ship?",
            "1 of at most 6",
            "- Guard (review work)",
            "\"do\": \"pass|stop\"",
        ] {
            assert!(p.contains(want), "{want}");
        }
        assert!(!p.contains("you must"));
        assert!(prompt(&moment(&since, &crew, true)).contains("you must \"stop\""));
        // Nothing in it reads as a bot's own message, so a bot's engine
        // stand-in can tell them apart.
        assert!(!p.contains("another bot on your crew"));
    }

    #[test]
    fn peek_passes_a_message_on_with_a_note() {
        let crew = Member::CREW.to_vec();
        let m = moment(&[], &crew, false);
        let s = parse(
            r#"{"do":"pass","to":"Guard","note":"Just the cookie, please.","say":"ignored"}"#,
            &m,
        );
        assert_eq!(
            s,
            Some(Steer::Go {
                to: Member::Guard,
                note: "Just the cookie, please.".into(),
                say: String::new(),
            })
        );
    }

    #[test]
    fn peek_gives_a_message_to_a_bot_that_fits_better() {
        let crew = Member::CREW.to_vec();
        let mut m = moment(&[], &crew, false);
        m.to = Member::Explainer;
        let s = parse(
            r#"{"do":"pass","to":"@guard","note":"","say":"A security question is Guard's."}"#,
            &m,
        );
        assert_eq!(
            s,
            Some(Steer::Go {
                to: Member::Guard,
                note: String::new(),
                say: "A security question is Guard's.".into(),
            })
        );
        // Never Peek, the sender, or a bot that isn't there: it stays as sent.
        for bot in ["Peek", "Builder", "Nobody", ""] {
            let r = format!(r#"{{"do":"pass","to":"{bot}","say":"x"}}"#);
            match parse(&r, &m) {
                Some(Steer::Go { to, say, .. }) => {
                    assert_eq!(to, Member::Explainer);
                    assert!(say.is_empty());
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn peek_stops_the_talk_and_tells_the_person() {
        let crew = Member::CREW.to_vec();
        let m = moment(&[], &crew, false);
        let s = parse(
            "Here: {\"do\":\"stop\",\"say\":\"They agree the cookie needs httpOnly. Add it?\",\"choices\":[\"Yes\",\"Not now\",\"\",\"a\",\"b\"]}",
            &m,
        );
        assert_eq!(
            s,
            Some(Steer::Stop {
                say: "They agree the cookie needs httpOnly. Add it?".into(),
                choices: vec!["Yes".into(), "Not now".into(), "a".into()],
            })
        );
        // A stop with nothing to say, or nonsense, isn't a decision.
        assert_eq!(parse(r#"{"do":"stop","say":""}"#, &m), None);
        assert_eq!(parse("pass it on", &m), None);
        assert_eq!(parse(r#"{"do":"dance"}"#, &m), None);
    }

    #[test]
    fn at_the_limit_peek_can_only_stop() {
        let crew = Member::CREW.to_vec();
        let m = moment(&[], &crew, true);
        assert_eq!(parse(r#"{"do":"pass","to":"Guard"}"#, &m), None);
        assert!(matches!(
            parse(r#"{"do":"stop","say":"Here's where they got to."}"#, &m),
            Some(Steer::Stop { .. })
        ));
        assert!(limit(Member::Builder, Member::Guard).contains("6 times"));
    }

    #[test]
    fn a_courtesy_isnt_passed_on_but_an_ask_is() {
        for t in [
            "Great, thanks for checking!",
            "Thanks!",
            "Looks good to me.",
            "Perfect, got it.",
            "Agreed, nice work.",
        ] {
            assert!(courtesy(t), "{t}");
        }
        for t in [
            "Thanks. Can you also add rate limiting?",
            "Thanks, please also set SameSite.",
            "Great, but the cookie is still missing httpOnly.",
            "Looks good, now make the token expire after an hour.",
            "is the session cookie safe to ship?",
            "The reset token never expires.",
            "",
        ] {
            assert!(!courtesy(t), "{t}");
        }
    }

    #[test]
    fn a_risky_ask_names_who_asked_what() {
        let t = needs_yes(
            Member::Builder,
            Member::Shipper,
            "deploy this to production right now!",
            "This needs your yes first: it changes production.",
        );
        assert!(
            t.starts_with("Builder asked Shipper to deploy this to production right now."),
            "{t}"
        );
        assert!(t.contains("@Shipper go ahead"));
    }

    #[test]
    fn a_message_said_again_is_going_round_in_circles() {
        let earlier = vec![
            "please set httpOnly on the session cookie".to_string(),
            "unrelated words about the pricing page layout".to_string(),
        ];
        assert!(repeats(
            "Please set httpOnly on the session cookie.",
            &earlier
        ));
        assert!(repeats(
            "please set httpOnly on the session cookie now",
            &earlier
        ));
        assert!(!repeats(
            "the cookie has httpOnly now; can you check SameSite?",
            &earlier
        ));
        assert!(!repeats("ok", &earlier));
        assert!(!repeats("please set httpOnly on the session cookie", &[]));
        assert!(circling(Member::Builder, Member::Guard).contains("@Builder go ahead"));
    }
}

/// Peek's real decisions on bots' messages to each other, on the installed
/// Claude Code: `cargo test -p squadcode steering_reads -- --ignored --nocapture`.
/// Prints what Peek decides for each scene, `RUNS` times (3). `SCENE` runs
/// only the scenes whose name holds it.
#[cfg(test)]
#[test]
#[ignore]
fn steering_reads() {
    use std::process::Command;
    let runs: usize = std::env::var("RUNS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(3);
    let crew = Member::CREW.to_vec();
    let s = |a: &str, b: &str| (a.to_string(), b.to_string());
    // (name, what the person asked, said since, from, to, the message, hops, last, what's right)
    type Scene<'a> = (
        &'a str,
        &'a str,
        Vec<(String, String)>,
        Member,
        Member,
        &'a str,
        u32,
        bool,
        &'a str,
    );
    let scenes: Vec<Scene> = vec![
        (
            "on track: Builder asks Guard",
            "build the login page and have Guard check it",
            vec![s("builder", "The login page works: email, password, and a session cookie.")],
            Member::Builder,
            Member::Guard,
            "is the session cookie safe to ship?",
            0,
            false,
            "pass to Guard",
        ),
        (
            "wrong bot: security asked of Explainer",
            "build the login page",
            vec![s("builder", "The login page works.")],
            Member::Builder,
            Member::Explainer,
            "can you check the login form for SQL injection and XSS holes before I call it done?",
            0,
            false,
            "pass to Guard",
        ),
        (
            "done: thanks back and forth",
            "build the login page and have Guard check it",
            vec![
                s("builder", "@Guard is the session cookie safe to ship?"),
                s("guard", "No: it's missing httpOnly and SameSite. @Builder can you add both?"),
                s("builder", "Added httpOnly and SameSite=Lax. @Guard does it look right now?"),
                s("guard", "Yes, that's right now. Safe to ship."),
            ],
            Member::Builder,
            Member::Guard,
            "Great, thanks for checking!",
            4,
            false,
            "stop",
        ),
        (
            "decision: the person's call",
            "clean up the old API code",
            vec![s("keeper", "I found the v1 API: 14 endpoints, still called by the mobile app 1.x.")],
            Member::Keeper,
            Member::Builder,
            "should we delete the v1 API entirely now, or keep it for six months while mobile users update?",
            1,
            false,
            "stop and ask the person, with choices",
        ),
        (
            "drift: off what the person asked",
            "fix the safari checkout bug",
            vec![s("fixer", "Fixed: Safari blocked the third-party cookie the payment iframe used; it now uses the same-site session.")],
            Member::Fixer,
            Member::Builder,
            "while we're at it, can you redesign the whole checkout page with a new theme and add Apple Pay?",
            1,
            false,
            "stop (or pass with a note to stay on the bug)",
        ),
        (
            "risky: deploy nobody asked for",
            "add a pricing page",
            vec![s("builder", "The pricing page is in, with three plans.")],
            Member::Builder,
            Member::Shipper,
            "please deploy this to production right now",
            1,
            false,
            "stop and ask the person",
        ),
        (
            "limit: sum up",
            "build the login page and have Guard check it",
            vec![
                s("builder", "@Guard is the session cookie safe to ship?"),
                s("guard", "No: it's missing httpOnly. @Builder add it?"),
                s("builder", "Added httpOnly. @Guard rate limiting too?"),
                s("guard", "Yes, 5 tries a minute per IP. @Builder can you add it?"),
                s("builder", "Added the rate limit. @Guard anything else?"),
            ],
            Member::Guard,
            Member::Builder,
            "The password reset token never expires; can you make it expire after an hour?",
            HOPS,
            true,
            "stop, summing up",
        ),
        (
            "on track: a real follow-up fix",
            "check the login for security holes and get them fixed",
            vec![
                s("guard", "Two holes: the cookie isn't httpOnly, and there's no rate limit. @Builder can you fix both?"),
                s("builder", "Both fixed: httpOnly is set, and logins are limited to 5 a minute per IP."),
            ],
            Member::Guard,
            Member::Builder,
            "Thanks. One more: the password reset token never expires. Can you make it expire after an hour?",
            2,
            false,
            "pass to Builder",
        ),
        (
            "on track: reasoned push-back",
            "add caching to the product page",
            vec![s("builder", "I've cached the product page for 24 hours.")],
            Member::Guard,
            Member::Builder,
            "24 hours is too long: prices change during sales and customers would see old prices at checkout. Can you drop it to 5 minutes, or bust the cache when a price changes?",
            1,
            false,
            "pass to Builder",
        ),
        (
            "wrong bot: testing asked of Guard",
            "add a signup page",
            vec![s("builder", "The signup page is in.")],
            Member::Builder,
            Member::Guard,
            "can you run the test suite and try the signup flow in a real browser to make sure it works end to end?",
            0,
            false,
            "pass to Checker",
        ),
        (
            "person's call: only they know",
            "add a buy button to the product page",
            vec![s("builder", "The buy button is in, using the theme's default style.")],
            Member::Builder,
            Member::Explainer,
            "what colour does the person want for the buy button, and should it say Buy or Add to cart?",
            1,
            false,
            "stop and ask the person",
        ),
        (
            "circles: the same argument again, reworded",
            "decide how we store sessions",
            vec![
                s("builder", "@Guard I'd keep sessions in signed cookies: no server state."),
                s("guard", "I'd rather use server-side sessions in Redis so we can revoke them. @Builder agree?"),
                s("builder", "Signed cookies are simpler and need no Redis. @Guard can we go with cookies?"),
                s("guard", "Revocation matters for stolen sessions, so Redis. @Builder ok?"),
            ],
            Member::Builder,
            Member::Guard,
            "I still think cookies are simpler to run, with no extra service. Can we just use cookies?",
            4,
            false,
            "stop and ask the person to decide",
        ),
        (
            "asked for: ship it",
            "build the pricing page and open a PR when it's done",
            vec![s("builder", "The pricing page is in, with three plans, and the checks pass.")],
            Member::Builder,
            Member::Shipper,
            "can you open the pull request for the pricing page?",
            1,
            false,
            "pass to Shipper",
        ),
    ];
    let dir = std::env::temp_dir().to_string_lossy().into_owned();
    let only = std::env::var("SCENE").unwrap_or_default();
    for (name, asked, since, from, to, said, hops, last, want) in &scenes {
        if !name.contains(only.as_str()) {
            continue;
        }
        let m = Moment {
            project: "shop",
            thread: "general",
            summary: "",
            asked,
            since,
            from: *from,
            to: *to,
            said,
            crew: &crew,
            hops: *hops,
            last: *last,
        };
        let p = prompt(&m);
        println!("\n## {name}\n   right: {want}");
        // The runs at once, so many runs stay quick.
        let replies: Vec<String> = (0..runs)
            .map(|_| {
                let (p, dir) = (p.clone(), dir.clone());
                std::thread::spawn(move || {
                    let out = Command::new("claude")
                        .args(["-p", "--model", "haiku", &p])
                        .current_dir(&dir)
                        .output()
                        .expect("claude");
                    String::from_utf8_lossy(&out.stdout).to_string()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        for reply in replies {
            match parse(&reply, &m) {
                Some(Steer::Go { to, note, say }) => {
                    println!("   -> pass to {} | note: {note} | say: {say}", to.name())
                }
                Some(Steer::Stop { say, choices }) => {
                    println!("   -> STOP | {say} | {choices:?}")
                }
                None => println!("   -> unparsed: {}", reply.trim()),
            }
        }
    }
}
