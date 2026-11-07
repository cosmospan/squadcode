//! Bots talking to each other, in the thread, like teammates in a channel.
//!
//! A bot @mentions another at the end of its reply (`@Guard: is this safe to
//! ship?`). Squad Code posts that in the thread, where the person sees it,
//! and hands it to that bot: as the next turn of its job in the thread, or as
//! a new job when it has none. Its answer can @mention back, so two bots can
//! ask, answer and push back on each other. After [`HOPS`] messages between
//! bots with no word from the person, they stop and hand it to the person.

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
        " You work in a shared thread with the rest of the crew: {}. To ask or tell one of them something, end your reply with a line like \"@Guard: is the login safe to ship?\"; they'll answer in the thread, and you'll get their answer. Do it when the person asks for it or when their role would really help, not by habit. When another bot's point is wrong, push back plainly with your reasons; when it's right, say so and act on it.",
        others.join(", ")
    )
}

/// What a bot gets when another bot @mentions it.
pub fn relay_text(from: Member, from_job: u64, said: &str) -> String {
    format!(
        "{} (another bot on your crew, working on #{from_job}) says to you in the thread:\n\n{said}\n\nAnswer them. If you need to reply to {} directly, end with a line starting \"@{}:\". Push back if you disagree.",
        from.name(),
        from.name(),
        from.name()
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
        let t = relay_text(Member::Guard, 4, "The cookie isn't httpOnly.");
        assert!(t.contains("#4") && t.contains("@Guard:"));
    }
}
