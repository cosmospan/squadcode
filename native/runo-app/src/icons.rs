//! The app's icon set: Lucide (ISC licence, bundled by gpui-kit), drawn at a lighter
//! 1.5 stroke instead of Lucide's default 2.
//!
//! At the 13 to 16 point sizes the app uses, the default stroke reads heavy and
//! rounded, like a toy. The lighter line is what team-chat and developer apps use, and
//! it keeps every icon in one weight, so new ones match without anyone thinking about it.
//!
//! [`Icons`] sits in front of gpui-kit's catalog. It changes nothing but the stroke,
//! plus a short list of glyphs swapped for calmer ones, so every `IconName` keeps
//! working at every call site.

use gpui_kit::assets::AllAssets;
use gpui_kit::AssetSource;
use std::borrow::Cow;

/// The stroke every outline icon is drawn with, in Lucide's 24-unit grid.
pub(crate) const STROKE: &str = "1.5";

/// Glyphs drawn in place of Lucide's own where the original reads as a game or a joke:
/// (asked for, drawn instead).
const SWAPS: &[(&str, &str)] = &[
    // "Race it": several bots take the same job. A fork says that; crossed swords don't.
    ("swords", "git-fork"),
];

/// The asset source the app is built with: gpui-kit's icons, restyled, and everything
/// else passed through untouched.
pub(crate) struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let Some(name) = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"))
        else {
            return AllAssets.load(path);
        };
        let name = SWAPS
            .iter()
            .find(|(asked, _)| *asked == name)
            .map_or(name, |(_, drawn)| drawn);
        let Some(bytes) = AllAssets.load(&format!("icons/{name}.svg"))? else {
            return Ok(None);
        };
        Ok(Some(Cow::Owned(restyle(&bytes).into_bytes())))
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        AllAssets.list(path)
    }
}

/// The SVG text of the icon `name` as the app draws it, for exports that inline icons.
pub(crate) fn svg(name: &str) -> Option<String> {
    let bytes = Icons.load(&format!("icons/{name}.svg")).ok()??;
    String::from_utf8(bytes.into_owned()).ok()
}

/// `svg` with Lucide's default stroke swapped for [`STROKE`]. Icons drawn with any other
/// stroke, or filled, are left as they are.
fn restyle(svg: &[u8]) -> String {
    String::from_utf8_lossy(svg)
        .replace("stroke-width=\"2\"", &format!("stroke-width=\"{STROKE}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_draw_with_the_lighter_stroke() {
        let search = svg("search").expect("search icon");
        assert!(search.contains("stroke-width=\"1.5\""), "{search}");
        assert!(!search.contains("stroke-width=\"2\""), "{search}");
    }

    #[test]
    fn swapped_glyphs_are_drawn_from_their_replacement() {
        assert_eq!(svg("swords"), svg("git-fork"));
        assert!(svg("swords").is_some());
    }

    #[test]
    fn other_assets_and_missing_icons_pass_through() {
        assert!(Icons.load("icons/no-such-icon.svg").is_err());
        assert_eq!(Icons.load("").ok().flatten(), None);
    }
}
