//! Squad Code's own intelligence layer, around the person's engine.
//!
//! - [`think`]: Peek reads every message with the person's own engine (kept
//!   warm by [`warm`]) and decides: reply, ask back, stop, run the app,
//!   answer a question, or hand work to a bot. There are no keyword rules;
//!   when the engine can't be reached Peek says so and starts nothing.
//! - [`front`]: the shape of that decision.
//! - [`route`]: the bot and kind of work picked, and Peek's line as it
//!   onboards or invites the bot.
//! - [`guard`]: risky asks wait for a yes, whoever decided.
//! - [`permit`]: what a bot may do on its own, what it asks about first,
//!   and what it never does.
//! - [`place`]: whether a job runs on this computer or on the person's server,
//!   and on which engine, with the reason said in plain words.
//! - [`ideas`]: a bot worth making, when the same kind of job keeps coming up
//!   and no bot on the crew is for it. The person decides; nothing is made for them.
//! - [`voice`]: what Peek says when something happens.
//! - [`talk`]: bots @mentioning each other in the thread, to ask, answer and
//!   push back, until the person is needed.
//! - [`tasks`]: with Smart Peek on, a short title for each job's card and a
//!   recap when it ends, from a small model on this computer ([`model`]).

pub mod cli;
pub mod digest;
pub mod front;
pub mod guard;
pub mod helper;
pub mod ideas;
pub mod maker;
pub mod model;
pub mod permit;
pub mod place;
pub mod route;
pub mod talk;
pub mod tasks;
pub mod think;
pub mod voice;
pub mod warm;

pub use place::{place, JobAsk, Site, Sites};

/// Words in `text`, lowercased, split on anything that isn't a letter, digit,
/// `'` or `-`.
pub(crate) fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `w` is `stem` or starts with it (so "crash" matches "crashes").
pub(crate) fn stem_match(w: &str, stem: &str) -> bool {
    w == stem || (stem.len() >= 4 && w.starts_with(stem))
}
