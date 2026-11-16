//! Guard rails: asks that are hard or impossible to undo wait for a yes.
//!
//! Plain rules, not a model: a rule that misses is a bug to fix here, and a
//! model's guess is not something to trust with production or someone's data.
//! What's caught is what the person asked for; the engines' own permission
//! settings still govern each command a bot runs.

use super::words;

/// Words that mean "every one of them".
const ALL: &[&str] = &["all", "every", "whole", "entire", "everything", "everyone"];
/// Features named with "live" that are about the app, not production:
/// "add live search" is a feature to build, not a change to the live site.
const LIVE_FEATURES: &[&str] = &[
    "search",
    "chat",
    "preview",
    "reload",
    "update",
    "updates",
    "typing",
    "validation",
    "editing",
    "collaboration",
    "cursor",
    "cursors",
    "counter",
    "feed",
    "notifications",
];
/// Things whose bulk removal can't easily be undone.
const BULK: &[&str] = &[
    "folder",
    "directory",
    "dir",
    "tables",
    "table",
    "rows",
    "records",
    "bucket",
    "database",
    "db",
    "accounts",
    "users",
    "customers",
    "branches",
    "issues",
    "history",
    "data",
];
const DELETE: &[&str] = &[
    "delete", "remove", "wipe", "drop", "purge", "truncate", "erase", "nuke", "destroy",
];

fn has(ws: &[String], set: &[&str]) -> bool {
    ws.iter().any(|w| set.contains(&w.as_str()))
}

/// Each kind of hard-to-undo ask, in plain words, when `text` is one.
fn reasons(text: &str) -> Vec<&'static str> {
    let lower = format!(" {} ", text.to_lowercase());
    let ws = words(text);
    let w = |x: &str| ws.iter().any(|y| y == x);
    let p = |x: &str| lower.contains(x);
    let mut out = Vec::new();

    let live = w("prod")
        || w("production")
        || p(" live ") && !LIVE_FEATURES.iter().any(|f| p(&format!(" live {f}")))
        || p("all users")
        || p("to customers");
    let changes = !ws
        .first()
        .is_some_and(|f| ["show", "read", "explain", "list"].contains(&f.as_str()));
    if live && changes {
        out.push("it touches production");
    }
    let main = w("main") || w("master");
    if (main && (w("straight") || w("directly") || w("direct") || w("strait")))
        || p("skip the pr")
        || p("no pr")
        || p("without a pr")
        || (main && w("push") && !w("branch"))
    {
        out.push("it changes the main branch without a review");
    }
    if p("force push")
        || p("force-push")
        || p("push --force")
        || p("push -f ")
        || (w("squash") && main)
        || (w("rewrite") && (w("history") || w("commits")))
        || p("reset --hard")
        || p("git clean")
    {
        out.push("it rewrites history, which can lose work");
    }
    let bulk_delete = has(&ws, DELETE)
        && (has(&ws, ALL) || has(&ws, BULK))
        && !p("delete button")
        && !p("delete buttons")
        && !p("delete icon");
    if bulk_delete || p("rm -rf") {
        out.push("it deletes in bulk");
    }
    if p("api key")
        || p("secret key")
        || p("live key")
        || p("private key")
        || w("credentials")
        || (w("rotate") && (w("key") || w("keys") || w("secret") || w("secrets") || w("token")))
        || (w("leaked") && (w("key") || w("secret") || w("token")))
    {
        out.push("it touches secrets");
    }
    let mail =
        w("email") || w("emails") || w("newsletter") || w("sms") || w("message") || w("messages");
    if ((mail
        && (has(&ws, ALL)
            || w("team")
            || w("list")
            || w("customers")
            || w("users")
            || w("clients")
            || w("send")))
        || p("post to slack")
        || w("tweet"))
        && !p("don't send")
        && !p("do not send")
        && !p("dont send")
    {
        out.push("it sends something to other people");
    }
    if [
        "refund every",
        "refund all",
        "refund the",
        "refund them",
        "refund customer",
        "refund order",
        "refund everyone",
        "charge the",
        "charge customer",
    ]
    .iter()
    .any(|x| p(x))
        || p("charge customers")
        || w("payout")
        || w("payouts")
    {
        out.push("it moves money");
    }
    if p("chmod 777") || p("sudo ") || p("disable auth") || p("turn off auth") {
        out.push("it weakens security");
    }
    out.dedup();
    out
}

/// Asking about a risky thing isn't doing it.
fn only_asks(text: &str) -> bool {
    let ws = words(text);
    let Some(first) = ws.first() else {
        return false;
    };
    matches!(
        first.as_str(),
        "how"
            | "why"
            | "what"
            | "where"
            | "who"
            | "when"
            | "explain"
            | "is"
            | "are"
            | "does"
            | "do"
            | "if"
            | "review"
            | "check"
            | "audit"
            | "find"
            | "should"
            | "would"
            | "describe"
    )
}

/// Why `text` needs a yes before a bot starts on it, or None when it doesn't.
pub fn risk(text: &str) -> Option<String> {
    if only_asks(text) {
        return None;
    }
    match reasons(text).as_slice() {
        [] => None,
        [one] => Some(format!("This needs your yes first: {one}.")),
        [first, rest @ ..] => Some(format!(
            "This needs your yes first: {first}, and {}.",
            rest.join(", and ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_to_undo_asks_wait_for_a_yes() {
        for t in [
            "force push the rebased branch",
            "drop table users and recreate it",
            "deploy the new pricing page to production",
            "rotate the stripe api key in .env",
            "email everyone on the waitlist",
            "rm -rf the build folder and the caches",
            "push the fix to the live site",
            "change the price on live",
        ] {
            assert!(risk(t).is_some(), "{t}");
        }
        let two = risk("deploy to prod and email all customers").unwrap();
        assert!(two.contains("production") && two.contains("sends"), "{two}");
    }

    #[test]
    fn everyday_work_and_questions_go_ahead() {
        for t in [
            "fix the login redirect",
            "add a dark mode toggle",
            "how does our deploy script work?",
            "review the payment code for risks",
            "make sure we never log the password",
            "write tests for the discount code",
            "fix the password reset email",
            "the token refresh fails after an hour",
            "fix the deploy script",
            "the buy button is misaligned",
            "see if you can do live search now",
            "add live chat to the support page",
            "turn on live reload",
        ] {
            assert!(risk(t).is_none(), "{t}: {:?}", risk(t));
        }
    }
}
