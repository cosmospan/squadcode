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
    /// the project, so the engine should read the folder.
    Answer { question: String, code: bool },
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
    /// "run the app": Squad Code starts it in the terminal and keeps it up,
    /// with no job.
    RunApp,
}

/// Who answers a question when nobody was picked: the crew's explainer.
pub fn answerer(crew: &[Member]) -> Member {
    route::doer(Mode::Ask, crew)
}
