//! What Peek does with a message the person sends. The person's own engine
//! decides (see [`super::think`]); this is only the decision's shape. There
//! are no keyword rules: an engine that can't be reached says so, and work
//! never starts on a guess.

use super::super::data::{Member, Mode};
use super::route::{self, Route};

/// What Peek does with a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Front {
    /// Peek answers itself, with this.
    Chat { reply: String },
    /// A question for an engine to answer, read-only. `code` when it is about
    /// the project, so the engine should read the folder. `by` is the bot
    /// Peek picked to answer, like the one whose question a short reply
    /// such as "1" answers; None when Peek answers itself.
    Answer {
        question: String,
        code: bool,
        by: Option<Member>,
    },
    /// Too vague to start: ask this, offering `choices` when there are some.
    AskBack {
        question: String,
        choices: Vec<String>,
    },
    /// Real work, for this bot.
    Hand(Route),
    /// Real work that can't easily be undone (production, deleting, secrets,
    /// sending): show `why` and start it as `Hand` only on the person's yes.
    Confirm { why: String, route: Route },
    /// "stop app", "cancel that", "kill the server": stop what's running
    /// here, with no new job.
    Stop,
    /// "run the app": Squad Code starts it itself, as a card, and keeps it up,
    /// with no job.
    RunApp,
}

/// What Peek decided, when the person named the bot with @: the named bot
/// answers what Peek would answer and takes the work Peek would hand on,
/// doing what that bot does. Work that needs code changed, named to a bot
/// that doesn't change code, isn't moved quietly: Peek asks once whether to
/// give it to the bot it picked. Stop, run the app and asking back stay
/// Peek's.
pub fn pinned(front: Front, bot: Member, text: &str) -> Front {
    let answer = || Front::Answer {
        question: text.to_string(),
        code: true,
        by: None,
    };
    let to = |mut r: Route| {
        r.bot = bot;
        r.mode = bot.mode();
        r
    };
    // The named bot can't do it when it needs code changed and the bot
    // doesn't change code.
    let cant = |r: &Route| r.bot != bot && r.mode.writes() && !bot.mode().writes();
    match front {
        Front::Chat { .. } => answer(),
        Front::Hand(r) if cant(&r) => Front::Confirm {
            why: switch_line(bot, r.bot),
            route: switched(r),
        },
        Front::Hand(_) if bot.mode() == Mode::Ask => answer(),
        Front::Hand(r) => Front::Hand(to(r)),
        Front::Confirm { why, route } if cant(&route) => Front::Confirm {
            why: format!("{why} {}", switch_line(bot, route.bot)),
            route: switched(route),
        },
        Front::Confirm { why, route } => Front::Confirm {
            why,
            route: to(route),
        },
        other => other,
    }
}

/// How a route Peek offers to switch to is marked, so its yes reads "Give it
/// to Builder" rather than "Go ahead".
pub const SWITCH: &str = "switch";

fn switched(mut r: Route) -> Route {
    r.mode = r.bot.mode();
    r.by = SWITCH.into();
    r
}

/// Peek's one question when the named bot doesn't do this kind of work.
fn switch_line(named: Member, to: Member) -> String {
    format!(
        "{} doesn't change code ({}). Give this to {} instead?",
        named.name(),
        named.role().to_lowercase(),
        to.name()
    )
}

/// Who answers a question when nobody was picked: the crew's explainer.
pub fn answerer(crew: &[Member]) -> Member {
    route::doer(Mode::Ask, crew)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(bot: Member) -> Route {
        Route {
            mode: bot.mode(),
            bot,
            sure: 1.0,
            why: String::new(),
            by: "engine".into(),
            site: String::new(),
        }
    }

    #[test]
    fn small_talk_to_a_named_bot_is_its_to_answer_not_a_job() {
        let f = pinned(
            Front::Chat {
                reply: "Hi!".into(),
            },
            Member::Builder,
            "who are you",
        );
        assert!(matches!(f, Front::Answer { ref question, .. } if question == "who are you"));
    }

    #[test]
    fn work_goes_to_the_named_bot() {
        let f = pinned(
            Front::Hand(route(Member::Fixer)),
            Member::Builder,
            "add a login page",
        );
        match f {
            Front::Hand(r) => assert_eq!(r.bot, Member::Builder),
            other => panic!("{other:?}"),
        }
        assert_eq!(pinned(Front::Stop, Member::Builder, "stop"), Front::Stop);
    }

    #[test]
    fn a_review_bot_named_for_a_review_does_it_as_a_job() {
        let f = pinned(
            Front::Hand(route(Member::Guard)),
            Member::Guard,
            "check the coupon code",
        );
        match f {
            Front::Hand(r) => assert_eq!((r.bot, r.mode), (Member::Guard, Mode::Review)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn building_named_to_a_bot_that_doesnt_build_asks_once() {
        let f = pinned(
            Front::Hand(route(Member::Builder)),
            Member::Guard,
            "add a login page",
        );
        match f {
            Front::Confirm { why, route } => {
                assert!(why.contains("Guard") && why.contains("Builder"), "{why}");
                assert_eq!((route.bot, route.by.as_str()), (Member::Builder, SWITCH));
            }
            other => panic!("{other:?}"),
        }
    }
}
