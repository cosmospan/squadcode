//! The v2 look: colour tokens and the few building blocks every screen uses
//! (cards, pills, chips, labels), taken from the Holo app screens design.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

// Named, not globbed: names like `fill` would clash with the kit's prelude.
#[allow(unused_imports)]
pub use super::theme::{
    add, ask_tone, bad_tone, bg, body, del, faint, fill, fill_hover, fill_press, float,
    float_shadow, grey_tone, hover, ink, line, line2, line_hi, muted, off, ok_tone, on_fill, panel,
    press, raised, run_tone, set_theme, t_amber, t_blue, t_green, t_red, t_violet, theme, well,
    Tone, RUNO, THEMES,
};

// Status and member colours: the same in every theme, as dots, bars and fills.
pub const ORANGE: u32 = 0xFF6A2B;
pub const GREEN: u32 = 0x1FC27A;
pub const AMBER: u32 = 0xF59E0B;
pub const BLUE: u32 = 0x3B82F6;
pub const VIOLET: u32 = 0x7B4DF5;
pub const RED: u32 = 0xF2364F;
pub const MONO: &str = "JetBrains Mono";

/// The spacing scale every screen keeps to, in pixels.
#[allow(dead_code)]
pub mod space {
    pub const S: f32 = 8.;
    pub const SM: f32 = 12.;
    pub const M: f32 = 16.;
    pub const L: f32 = 24.;
    pub const XL: f32 = 32.;
}

/// A colour from `0xRRGGBB`, or `0xAARRGGBB` for a see-through one in a frosted
/// theme (a top byte of zero is solid).
pub fn c(hex: u32) -> Hsla {
    let mut h: Hsla = rgb(hex & 0xff_ffff).into();
    h.a = alpha(hex);
    h
}

/// A colour at opacity `a`, on top of any opacity the colour already has.
pub fn ca(hex: u32, a: f32) -> Hsla {
    let mut h = c(hex);
    h.a *= a;
    h
}

fn alpha(hex: u32) -> f32 {
    match hex >> 24 {
        0 => 1.0,
        a => a as f32 / 255.0,
    }
}

/// The colours of a terminal: the drawer.
pub struct Term {
    pub bg: u32,
    pub fg: u32,
    pub dim: u32,
    pub line: u32,
}

/// A terminal is dark in every theme. A dark theme gives it its own sunken well,
/// so it sits in the theme (navy in Deep sea, violet in Aurora); a light theme
/// gets a neutral dark one. Always solid, so the output reads over any desktop.
pub fn term() -> Term {
    let t = theme();
    if t.light {
        return Term {
            bg: 0x15151C,
            fg: 0xD8D8E0,
            dim: 0x8A8A97,
            line: 0x2A2A34,
        };
    }
    let bg = t.well & 0xff_ffff;
    Term {
        bg,
        fg: t.body,
        dim: t.muted,
        line: super::theme::mix(bg, t.ink, 0.12),
    }
}

/// The background pattern picked under Settings > Appearance, on `area` (the page,
/// or the sidebar), in the theme's own ink warming into violet. Laid under what the
/// area holds; solid cards cover it.
pub fn pattern(area: crate::pattern::Area) -> Option<AnyElement> {
    use crate::pattern::{current, shows, Pattern};
    let p = current();
    if p == Pattern::Off || !shows(area) {
        return None;
    }
    let t = theme();
    let colors = (t.ink, VIOLET, !t.light);
    Some(
        canvas(
            |_, _, _| (),
            move |b, _, window, _| {
                use crate::pattern::Area;
                // The page's grid starts with a whole cell at its top-left corner;
                // the sidebar's lines up with it, so they meet as one grid.
                let anchor = if area == Area::Content {
                    crate::pattern::set_page_origin(b.origin);
                    b.origin
                } else {
                    crate::pattern::page_origin().unwrap_or(point(b.right(), b.top()))
                };
                crate::pattern::paint_in(p, b, None, Some(anchor), 1., colors, window)
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element(),
    )
}

/// `p` painted over the element it's laid in, `gain` times its usual strength.
pub fn pattern_of(p: crate::pattern::Pattern, gain: f32) -> AnyElement {
    let t = theme();
    let colors = (t.ink, VIOLET, !t.light);
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            crate::pattern::paint_in(p, b, None, Some(b.origin), gain, colors, window)
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// Marks `el` as a bar or header over the page: the background pattern stays out
/// from under it, and when it runs across the page's top, starts again on a whole
/// cell right below it.
pub fn chrome(el: Div) -> Div {
    el.relative().child(
        canvas(|b, _, _| crate::pattern::add_chrome(b), |_, _, _, _| ())
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
    )
}

/// A card's corner radius.
pub const CARD_R: f32 = 10.;

/// The ground under everything: the sidebar sits on it and the page canvas
/// floats over it. A shade darker than the page in every theme, keeping a
/// frosted theme's opacity.
pub fn ground() -> u32 {
    let t = theme();
    // Crew Chat's sidebar and Jobs panel: a cool, soft grey.
    if t.id == "crew-chat" {
        return super::theme::glass(0xF6F6F9);
    }
    let (base, k) = if t.light {
        (t.bg, 0.045)
    } else {
        (t.well, 0.3)
    };
    super::theme::glass(super::theme::mix(base & 0xff_ffff, 0x000000, k) | (base & 0xff00_0000))
}

/// A panel on the ground: flat, with a hairline edge and no shadow.
pub fn card() -> Div {
    div()
        .bg(c(panel()))
        .border_1()
        .border_color(c(line()))
        .rounded(px(CARD_R))
}

/// A quiet outline button, like "Look first" or "Pause": a hairline and
/// nothing behind it until you point at it.
pub fn ghost(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    ghost_base(id).child(label.into())
}

/// A ghost button led by an icon, like "⟲ Run again".
pub fn ghost_i(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    ghost_base(id)
        .pl(px(10.))
        .child(Icon::new(icon).size(px(14.)).text_color(c(muted())))
        .child(label.into())
}

fn ghost_base(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(12.))
        .h(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .rounded(px(8.))
        .border_1()
        .border_color(c(line_hi()))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(ink()))
        .cursor_pointer()
        .map(|d| super::motion::hover_fill(d, None, hover()))
        .active(|s| s.bg(c(press())))
}

/// The primary button, like "Approve & open PR": the theme's solid fill,
/// high contrast and nothing else.
pub fn solid(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    solid_base(id).child(label.into())
}

/// A primary button led by an icon, like "+ New job".
pub fn solid_i(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    solid_base(id)
        .pl(px(11.))
        .child(Icon::new(icon).size(px(14.)))
        .child(label.into())
}

fn solid_base(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(14.))
        .h(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .rounded(px(8.))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(on_fill()))
        .cursor_pointer()
        .map(|d| super::motion::hover_fill(d, Some(fill()), fill_hover()))
        .active(|s| s.bg(c(fill_press())))
}

/// A square icon-only button, like search or close: no edge, a soft tint on hover.
pub fn icon_btn(id: impl Into<ElementId>, icon: IconName) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(30.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .cursor_pointer()
        .map(|d| super::motion::hover_paint(d, None, hover(), Some((muted(), ink()))))
        .active(|s| s.bg(c(press())))
        .child(Icon::new(icon).size(px(16.)))
}

/// A small pill for filters and tabs; `on` is the selected one, shown as a
/// flat tint rather than a raised chip.
pub fn pill(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    pill_base(id, on).child(label.into())
}

/// A pill led by an icon, like the composer's "Split" and "Race" toggles.
pub fn pill_i(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    on: bool,
) -> Stateful<Div> {
    pill_base(id, on)
        .child(Icon::new(icon).size(px(13.)))
        .child(label.into())
}

fn pill_base(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    let id: ElementId = id.into();
    // A newly picked pill's tint fades in as the choice moves to it.
    let k = if on {
        super::motion::shown(
            super::motion::key(("pill-on", &id)),
            super::motion::QUICK,
            0.,
        )
    } else {
        1.
    };
    div()
        .id(id)
        .px(px(10.))
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(7.))
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(ca(raised(), k)).text_color(c(ink()))
            } else {
                super::motion::hover_paint(d, None, hover(), Some((muted(), ink())))
                    .active(|s| s.bg(c(press())))
            }
        })
}

/// A tinted status badge, like "Ready for review": the tone's text on its
/// own soft fill, with a dot.
pub fn badge(tone: Tone, label: impl Into<SharedString>) -> Div {
    let (text, fill, _) = tone;
    div()
        .h(px(22.))
        .px(px(8.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(6.))
        .bg(c(fill))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(text))
        .child(super::motion::dot(text, 6., live_tone(tone)))
        .child(label.into())
}

/// Whether a badge in `tone` is for something live: running, or waiting on
/// you. Its dot breathes.
fn live_tone(tone: Tone) -> bool {
    tone == super::theme::run_tone() || tone == ask_tone()
}

/// A keyboard hint, like "⌘K".
pub fn kbd(text: impl Into<SharedString>) -> Div {
    div()
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(c(line()))
        .text_size(px(10.5))
        .font_family(MONO)
        .text_color(c(faint()))
        .child(text.into())
}

/// A stat card: a quiet label with its icon on a soft tile in `tone`, the
/// figure, and a note.
pub fn stat(
    (icon, tone): (IconName, u32),
    label: &'static str,
    value: String,
    note: String,
) -> Div {
    div()
        .flex_1()
        .flex_basis(px(0.))
        .min_w_0()
        .px(px(16.))
        .pt(px(14.))
        .pb(px(16.))
        .bg(c(panel()))
        .border_1()
        .border_color(c(line()))
        .rounded(px(CARD_R))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(muted()))
                .child(label)
                .child(
                    div()
                        .size(px(26.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(7.))
                        .bg(ca(tone, 0.12))
                        .text_color(c(tone))
                        .child(Icon::new(icon).size(px(14.))),
                ),
        )
        .child(
            div()
                .mt(px(6.))
                .text_size(px(26.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value),
        )
        .child(
            div()
                .mt(px(2.))
                .text_size(px(12.))
                .text_color(c(faint()))
                .truncate()
                .child(note),
        )
}

/// A section's mark, the same on every screen: a small uppercase label, with
/// an optional status dot before it and a count at the far end.
pub fn heading(label: &str, dot: Option<u32>, count: Option<usize>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(7.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(faint()))
        .when_some(dot, |d, col| {
            d.child(div().size(px(6.)).rounded_full().bg(c(col)))
        })
        .child(label.to_uppercase())
        .child(div().flex_1())
        .when_some(count, |d, n| d.child(n.to_string()))
}

/// The small uppercase mono caption the design puts over and under things.
pub fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(MONO)
        .text_size(px(9.5))
        .text_color(c(faint()))
        .child(text.into())
}

/// A coloured dot and a word, like "● Working".
pub fn status(color: u32, label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(color))
        .child(div().size(px(7.)).rounded_full().bg(c(color)))
        .child(label.into())
}

/// A thin progress bar.
pub fn bar(frac: f32, color: u32) -> Div {
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(c(raised()))
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(c(color))
                .w(relative(frac.clamp(0.0, 1.0))),
        )
}

/// A thin bar with a soft segment sweeping across, for work with no known end.
pub fn sweep(color: u32, _phase: f32) -> Div {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let p = runo_holo::anim::SOFT.at((t / 1.6).rem_euclid(1.0)) as f32;
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(c(raised()))
        .relative()
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(0.3))
                .left(relative(-0.3 + 1.3 * p))
                .rounded_full()
                .bg(c(color)),
        )
}

/// Eases an element in: `k` runs 0 → 1, fading it up from a few pixels lower.
pub fn rise<E: Styled>(el: E, k: f32) -> E {
    super::motion::rise(el, k)
}

/// Puts text in a box with the caret after it, so typing carries on from there.
pub(super) fn put_text(
    i: &mut gpui_kit::component::input::TextareaState,
    text: impl Into<String>,
    window: &mut Window,
    cx: &mut Context<gpui_kit::component::input::TextareaState>,
) {
    let text: String = text.into();
    let line = text.matches('\n').count() as u32;
    let character = text
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .encode_utf16()
        .count() as u32;
    i.set_value(text, window, cx);
    i.set_cursor_position(
        gpui_kit::component::input::Position { line, character },
        window,
        cx,
    );
}
