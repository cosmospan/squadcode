//! The fonts the window is drawn in: one for the interface and one for code
//! (the terminal, diffs, paths and the small labels set in mono). Each is
//! picked under Settings > Appearance from a few that ship with the app, so
//! they look the same on every machine whether or not they're installed.

use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui_kit::App;

/// A font Settings offers.
pub struct Face {
    pub id: &'static str,
    /// The name Settings shows.
    pub name: &'static str,
    /// The family name the text system knows it by.
    pub family: &'static str,
    /// One line on why you'd pick it.
    pub line: &'static str,
}

/// Interface fonts, in the order Settings shows them. The first is the default.
pub const SANS: [Face; 6] = [
    Face {
        id: "geist",
        name: "Geist",
        family: "Geist",
        line: "Squad Code's own: crisp and modern",
    },
    Face {
        id: "inter",
        name: "Inter",
        family: "Inter",
        line: "The classic screen face, easy to read",
    },
    Face {
        id: "plex-sans",
        name: "IBM Plex Sans",
        family: "IBM Plex Sans",
        line: "Engineered and warm, with a little character",
    },
    Face {
        id: "figtree",
        name: "Figtree",
        family: "Figtree",
        line: "Friendly and round, light on the eye",
    },
    Face {
        id: "atkinson",
        name: "Atkinson Hyperlegible",
        family: "Atkinson Hyperlegible Next",
        line: "Made for legibility: every letter stands apart",
    },
    Face {
        id: "system",
        name: "System",
        family: ".SystemUIFont",
        line: "Your computer's own interface font",
    },
];

/// Code fonts, in the order Settings shows them. The first is the default.
/// Every one is 0.6em wide, which the terminal's columns count on.
pub const CODE: [Face; 4] = [
    Face {
        id: "jetbrains-mono",
        name: "JetBrains Mono",
        family: "JetBrains Mono",
        line: "Tall letters, made for reading code all day",
    },
    Face {
        id: "geist-mono",
        name: "Geist Mono",
        family: "Geist Mono",
        line: "Geist's mono twin: clean and quiet",
    },
    Face {
        id: "plex-mono",
        name: "IBM Plex Mono",
        family: "IBM Plex Mono",
        line: "A typewriter's warmth, with clear shapes",
    },
    Face {
        id: "fira-code",
        name: "Fira Code",
        family: "Fira Code",
        line: "Warm and round, with the most joined-up signs",
    },
];

static SANS_AT: AtomicUsize = AtomicUsize::new(0);
static CODE_AT: AtomicUsize = AtomicUsize::new(0);

fn find(faces: &[Face], id: &str) -> usize {
    faces.iter().position(|f| f.id == id).unwrap_or(0)
}

/// Puts the saved fonts in use; an unknown or empty id means the default.
pub fn set(sans: &str, code: &str) {
    SANS_AT.store(find(&SANS, sans), Ordering::Relaxed);
    CODE_AT.store(find(&CODE, code), Ordering::Relaxed);
}

/// The interface font in use.
pub fn sans_face() -> &'static Face {
    &SANS[SANS_AT.load(Ordering::Relaxed).min(SANS.len() - 1)]
}

/// The code font in use.
pub fn code_face() -> &'static Face {
    &CODE[CODE_AT.load(Ordering::Relaxed).min(CODE.len() - 1)]
}

/// The interface font's family name.
pub fn sans() -> &'static str {
    sans_face().family
}

/// The code font's family name.
pub fn mono() -> &'static str {
    code_face().family
}

macro_rules! faces {
    ($($file:literal),* $(,)?) => {
        vec![$(Cow::Borrowed(include_bytes!(concat!("../../assets/fonts/", $file)) as &[u8])),*]
    };
}

/// Hands every bundled font to the text system, once at start.
pub fn load(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = faces![
        "Geist-Regular.otf",
        "Geist-Medium.otf",
        "Geist-SemiBold.otf",
        "Geist-Bold.otf",
        "Inter-Regular.ttf",
        "Inter-Medium.ttf",
        "Inter-SemiBold.ttf",
        "Inter-Bold.ttf",
        "IBMPlexSans-Regular.ttf",
        "IBMPlexSans-Medium.ttf",
        "IBMPlexSans-SemiBold.ttf",
        "IBMPlexSans-Bold.ttf",
        "Figtree-Regular.ttf",
        "Figtree-Medium.ttf",
        "Figtree-SemiBold.ttf",
        "Figtree-Bold.ttf",
        "AtkinsonHyperlegibleNext-Regular.ttf",
        "AtkinsonHyperlegibleNext-Medium.ttf",
        "AtkinsonHyperlegibleNext-SemiBold.ttf",
        "AtkinsonHyperlegibleNext-Bold.ttf",
        "JetBrainsMono-Regular.ttf",
        "JetBrainsMono-Medium.ttf",
        "JetBrainsMono-SemiBold.ttf",
        "JetBrainsMono-Bold.ttf",
        "GeistMono-Regular.ttf",
        "GeistMono-Medium.ttf",
        "GeistMono-SemiBold.ttf",
        "GeistMono-Bold.ttf",
        "IBMPlexMono-Regular.ttf",
        "IBMPlexMono-Medium.ttf",
        "IBMPlexMono-SemiBold.ttf",
        "IBMPlexMono-Bold.ttf",
        "FiraCode-Regular.ttf",
        "FiraCode-Medium.ttf",
        "FiraCode-SemiBold.ttf",
        "FiraCode-Bold.ttf",
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_empty_picks_the_defaults() {
        set("no-such-font", "");
        assert_eq!(sans(), "Geist");
        assert_eq!(mono(), "JetBrains Mono");
        set("inter", "fira-code");
        assert_eq!(sans(), "Inter");
        assert_eq!(mono(), "Fira Code");
        set("geist", "jetbrains-mono");
    }

    #[test]
    fn ids_are_unique() {
        for list in [&SANS[..], &CODE[..]] {
            for (i, a) in list.iter().enumerate() {
                assert!(list[i + 1..].iter().all(|b| b.id != a.id), "{}", a.id);
            }
        }
    }
}
