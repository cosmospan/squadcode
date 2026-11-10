//! Fin, the crew's mascot, and how each bot wears it.
//!
//! Every bot is drawn as Fin, in its own colour and dressed for its job: the
//! Builder in a hard hat with a wrench, the Guard as security with a shield and
//! baton, and so on. Bots made on Crew wear what they were given in the maker.
//! Settings → Mascot changes any of it. Choices live in `AppSettings`
//! (`fin_line`, `fin_looks`); a parsed copy is kept here so drawing a bot
//! never has to reach for the settings.

use std::collections::HashMap;
use std::sync::RwLock;

use squad_holo::fin::{Fin, Line, Outfit, Tool};
use squad_holo::{Figure, Mood, Tint};

use crate::settings::AppSettings;

use super::data::Member;
use super::engine::bot_key;

/// The colours a Fin can be, with the names they're saved under.
pub const COLOURS: [(&str, &str, Tint); 7] = [
    ("orange", "Orange", Tint::ORANGE),
    ("green", "Green", Tint::GREEN),
    ("amber", "Amber", Tint::AMBER),
    ("red", "Red", Tint::RED),
    ("blue", "Blue", Tint::BLUE),
    ("violet", "Violet", Tint::VIOLET),
    ("teal", "Teal", Tint::TEAL),
];

pub fn colour_id(t: Tint) -> &'static str {
    COLOURS
        .iter()
        .find(|c| c.2 == t)
        .map(|c| c.0)
        .unwrap_or("violet")
}

fn colour(id: &str) -> Option<Tint> {
    COLOURS.iter().find(|c| c.0 == id).map(|c| c.2)
}

/// Where `t` sits in [`COLOURS`], the form a made bot's colour is saved in.
pub fn colour_index(t: Tint) -> u8 {
    COLOURS.iter().position(|c| c.2 == t).unwrap_or(5) as u8
}

/// The colour at `i` in [`COLOURS`]; out of range is violet.
pub fn colour_at(i: u8) -> Tint {
    COLOURS.get(i as usize).map_or(Tint::VIOLET, |c| c.2)
}

/// What a bot carries when the person hasn't changed it: a prop that fits its job.
pub fn default_look(who: Member) -> (Tint, Outfit) {
    let carry = |right| Outfit {
        right,
        ..Outfit::PLAIN
    };
    match who {
        Member::Builder => (Tint::ORANGE, carry(Tool::Wrench)),
        Member::Checker => (Tint::GREEN, carry(Tool::TestTube)),
        Member::Guard => (Tint::AMBER, carry(Tool::Shield)),
        Member::Fixer => (Tint::RED, carry(Tool::Bandage)),
        Member::Explainer => (Tint::BLUE, carry(Tool::Pencil)),
        Member::Keeper => (Tint::VIOLET, carry(Tool::Key)),
        Member::Shipper => (Tint::TEAL, carry(Tool::Branch)),
        Member::Fin => (Tint::VIOLET, carry(Tool::Compass)),
        Member::Made(_) => who.made_look().unwrap_or((Tint::VIOLET, Outfit::PLAIN)),
    }
}

/// `colour.hat.left.right.badge`, the saved form of a look.
pub fn code(tint: Tint, outfit: Outfit) -> String {
    format!("{}.{}", colour_id(tint), Fin::outfit_code(outfit))
}

fn parse(code: &str) -> Option<(Tint, Outfit)> {
    let (c, rest) = code.split_once('.')?;
    Some((colour(c)?, Fin::outfit_from_code(rest)))
}

#[derive(Default)]
struct Looks {
    line: Option<Line>,
    saved: HashMap<String, String>,
    parsed: HashMap<String, (Tint, Outfit)>,
}

static LOOKS: std::sync::LazyLock<RwLock<Looks>> =
    std::sync::LazyLock::new(|| RwLock::new(Looks::default()));

/// Takes the person's choices from the settings; cheap when nothing changed.
pub fn sync(st: &AppSettings) {
    let line = Line::from_code(&st.fin_line);
    {
        let l = LOOKS.read().unwrap_or_else(|e| e.into_inner());
        if l.line == Some(line) && l.saved == st.fin_looks {
            return;
        }
    }
    let mut l = LOOKS.write().unwrap_or_else(|e| e.into_inner());
    l.line = Some(line);
    l.saved = st.fin_looks.clone();
    l.parsed = st
        .fin_looks
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), parse(v)?)))
        .collect();
}

/// The bot's look as saved, or its default.
pub fn look_in(st: &AppSettings, who: Member) -> (Tint, Outfit) {
    st.fin_looks
        .get(&bot_key(who))
        .and_then(|c| parse(c))
        .unwrap_or_else(|| default_look(who))
}

/// The bot's Fin as the settings have it.
pub fn fin_in(st: &AppSettings, who: Member) -> Fin {
    let (tint, outfit) = look_in(st, who);
    Fin {
        tint,
        line: Line::from_code(&st.fin_line),
        ears: has_ears(who),
        outfit,
    }
}

/// Saves a bot's look; the default look is stored as no entry at all.
pub fn set_look(st: &mut AppSettings, who: Member, tint: Tint, outfit: Outfit) {
    let key = bot_key(who);
    if (tint, outfit) == default_look(who) {
        st.fin_looks.remove(&key);
    } else {
        st.fin_looks.insert(key, code(tint, outfit));
    }
}

/// Only Peek, the crew's lead, wears ears.
pub fn has_ears(who: Member) -> bool {
    who == Member::Fin
}

/// How `who` is drawn in `mood`, as the last [`sync`] left things.
pub fn figure(who: Member, mood: Mood) -> Figure {
    figure_with(who, mood, (0, 0), 0)
}

/// [`figure`], looking towards `gaze` with its clock shifted by `offset` steps.
pub fn figure_with(who: Member, mood: Mood, gaze: (i8, i8), offset: u8) -> Figure {
    let l = LOOKS.read().unwrap_or_else(|e| e.into_inner());
    let (tint, outfit) = l
        .parsed
        .get(&bot_key(who))
        .copied()
        .unwrap_or_else(|| default_look(who));
    Figure::Fin(
        Fin {
            tint,
            line: l.line.unwrap_or_default(),
            ears: has_ears(who),
            outfit,
        }
        .pose(mood)
        .looking(gaze)
        .offset(offset),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use squad_holo::fin::ROLES;

    #[test]
    fn looks_round_trip_and_defaults_are_not_saved() {
        let mut st = AppSettings::default();
        let (t, o) = default_look(Member::Guard);
        set_look(&mut st, Member::Guard, t, o);
        assert!(st.fin_looks.is_empty());
        set_look(&mut st, Member::Guard, Tint::RED, ROLES[6].outfit);
        assert_eq!(look_in(&st, Member::Guard), (Tint::RED, ROLES[6].outfit));
    }

    #[test]
    fn every_built_in_bot_wears_something() {
        for who in std::iter::once(Member::Fin).chain(Member::CREW) {
            assert_ne!(default_look(who).1, Outfit::PLAIN, "{who:?}");
        }
    }
}
