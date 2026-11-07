//! The app's motion: a few timings every screen shares, and small helpers that
//! ease things in rather than snapping them.
//!
//! Everything here is quick and quiet. Hovers fade over a tenth of a second,
//! panels and dialogs settle in about a quarter, pages and lists a third, and
//! nothing bounces. With Settings → Animations off, or Reduce motion on in
//! macOS (Accessibility → Display), every helper gives its end state at once and
//! nothing asks for extra frames.
//!
//! The state lives on the UI thread (the window draws there), so the helpers are
//! plain functions any view can call while it renders. The shell starts each
//! frame with [`begin`] and keeps drawing while [`settling`] says something is
//! still on its way.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::ui::{c, ca};

/// Hovers and presses: there almost before you notice.
pub const QUICK: f64 = 0.14;
/// Panels, popovers, drawers and dialogs.
pub const BASE: f64 = 0.24;
/// Pages and the rows of a list.
pub const GENTLE: f64 = 0.34;
/// How long after the row above it each row of a list starts.
pub const STEP: f64 = 0.03;
/// Rows past this many in a list come in together, so a long list never drags.
pub const STEP_MAX: usize = 10;
/// How far a row or card lifts as it comes in, in pixels.
pub const LIFT: f32 = 6.;
/// One breath of a live status dot, in seconds.
pub const BREATH: f64 = 1.8;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(true) };
    /// The main window is rendering. Other windows (the menu bar tray) use the
    /// same buttons but don't keep drawing frames, so there they stay still.
    static DRAWING: Cell<bool> = const { Cell::new(false) };
    static EPOCH: Instant = Instant::now();
    static FRAME: Cell<u64> = const { Cell::new(0) };
    /// Frames are wanted until then: something is still easing.
    static UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
    /// What came on screen when, by key, and the last frame it was drawn in.
    static SHOWN: RefCell<HashMap<u64, (Instant, u64)>> = RefCell::new(HashMap::new());
    /// Each hover-faded element: pointer over it, when that changed, and how
    /// far it had faded then.
    static HOVERS: RefCell<HashMap<ElementId, (bool, Instant, f32)>> =
        RefCell::new(HashMap::new());
}

/// The shell starts a frame. `on`: motion is allowed (Animations on and Reduce
/// motion off). Things not drawn last frame are forgotten, so they ease in again
/// the next time they show.
pub fn begin(on: bool) {
    ON.set(on);
    DRAWING.set(true);
    let frame = FRAME.get() + 1;
    FRAME.set(frame);
    SHOWN.with(|s| s.borrow_mut().retain(|_, (_, last)| *last + 1 >= frame));
    if !on {
        UNTIL.set(None);
    }
}

/// The shell has built its frame.
pub fn end() {
    DRAWING.set(false);
}

/// Whether things may move here and now: motion is on and the main window is
/// rendering.
fn live() -> bool {
    ON.get() && DRAWING.get()
}

/// Whether something is still easing, so the window should draw the next frame
/// the display shows.
pub fn settling() -> bool {
    ON.get() && UNTIL.get().is_some_and(|t| Instant::now() < t)
}

/// Keep frames coming for `secs` more.
fn hold(secs: f64) {
    let until = Instant::now() + Duration::from_secs_f64(secs.max(0.0));
    UNTIL.set(Some(UNTIL.get().map_or(until, |t| t.max(until))));
}

/// A key for [`shown`], from anything hashable, like `("job-row", id)`.
pub fn key(k: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    k.hash(&mut h);
    h.finish()
}

/// How far 0 → 1 something that started at `at` has eased, settling in about
/// `settle` seconds after `delay`. 1 at once with motion off.
pub fn ease(at: Instant, settle: f64, delay: f64) -> f32 {
    if !live() {
        return 1.0;
    }
    let t = at.elapsed().as_secs_f64() - delay;
    if t < settle {
        hold(settle - t);
    }
    runo_holo::anim::spring(t.max(0.0), settle) as f32
}

/// How far 0 → 1 the thing at `key` has eased in since it came on screen: the
/// first frame it is drawn after a frame without it starts it again. `delay`
/// holds it back, for rows that follow each other in.
pub fn shown(key: u64, settle: f64, delay: f64) -> f32 {
    let frame = FRAME.get();
    let at = SHOWN.with(|s| {
        let mut s = s.borrow_mut();
        let e = s.entry(key).or_insert((Instant::now(), frame));
        if e.1 + 1 < frame {
            e.0 = Instant::now();
        }
        e.1 = frame;
        e.0
    });
    ease(at, settle, delay)
}

/// Row `i` of a list, eased in a beat after the row above it.
pub fn row(key: u64, i: usize) -> f32 {
    shown(key, GENTLE, STEP * i.min(STEP_MAX) as f64)
}

/// Fades `el` up from a few pixels lower as `k` runs 0 → 1.
pub fn rise<E: Styled>(el: E, k: f32) -> E {
    // Always positioned, so what's inside lays out the same once it settles.
    let el = el.relative();
    if k >= 1.0 {
        return el;
    }
    el.opacity(k.clamp(0.0, 1.0)).top(px(LIFT * (1. - k)))
}

/// Fades `el` in while sliding it `dx`, `dy` pixels into place.
pub fn slide<E: Styled>(el: E, k: f32, dx: f32, dy: f32) -> E {
    let el = el.relative();
    if k >= 1.0 {
        return el;
    }
    let k = k.clamp(0.0, 1.0);
    el.opacity(0.2 + 0.8 * k)
        .left(px(dx * (1. - k)))
        .top(px(dy * (1. - k)))
}

/// A list row or card, eased in as it comes on screen: the first time a list
/// shows, its rows follow each other in; a row added later comes in alone.
pub fn enter<E: Styled>(el: E, key: u64, i: usize) -> E {
    rise(el, row(key, i))
}

/// A panel, popover, drawer or dialog: fades in, settling `dy` pixels into
/// place (negative from above).
pub fn panel<E: Styled>(el: E, key: u64, dx: f32, dy: f32) -> E {
    slide(el, shown(key, BASE, 0.0), dx, dy)
}

/// A slow breath, 0 → 1 → 0 every [`BREATH`] seconds, for something live;
/// a steady 1 with motion off. Ask for frames while it shows with
/// [`breathing`].
pub fn breath() -> f32 {
    if !ON.get() {
        return 1.0;
    }
    let t = EPOCH.with(|e| e.elapsed().as_secs_f64());
    runo_holo::anim::alternate(t, BREATH / 2.0, runo_holo::anim::SOFT) as f32
}

/// Where a ring spreading out from a live dot is, 0 → 1, once a breath; 0
/// with motion off.
fn ring() -> f32 {
    if !ON.get() {
        return 0.0;
    }
    let t = EPOCH.with(|e| e.elapsed().as_secs_f64());
    runo_holo::anim::RISE.at((t / BREATH).rem_euclid(1.0)) as f32
}

/// A status dot of `size` pixels in `color`. `live` (running, or waiting on
/// you): a soft ring breathes out from it now and then.
pub fn dot(color: u32, size: f32, live: bool) -> Div {
    let core = div().size(px(size)).rounded_full().bg(c(color)).flex_none();
    if !live || !self::live() {
        return core;
    }
    hold(0.1);
    let p = ring();
    let r = size * (1. + 1.6 * p);
    let off = (size - r) / 2.;
    div()
        .size(px(size))
        .flex_none()
        .relative()
        .child(
            div()
                .absolute()
                .left(px(off))
                .top(px(off))
                .size(px(r))
                .rounded_full()
                .bg(ca(color, 0.45 * (1. - p))),
        )
        .child(core.absolute().top_0().left_0())
}

/// How far the pointer's hover over `id` has faded in, 0 → 1.
fn hover_level(id: &ElementId) -> f32 {
    HOVERS.with(|h| {
        let mut h = h.borrow_mut();
        let Some(&(over, since, from)) = h.get(id) else {
            return 0.0;
        };
        let to = if over { 1.0 } else { 0.0 };
        let t = since.elapsed().as_secs_f64();
        let k = runo_holo::anim::spring(t, QUICK) as f32;
        if t < QUICK {
            hold(QUICK - t);
        } else if !over {
            h.remove(id);
        }
        from + (to - from) * k
    })
}

fn set_hover(id: ElementId, over: bool) {
    let from = hover_level(&id);
    HOVERS.with(|h| {
        h.borrow_mut().insert(id, (over, Instant::now(), from));
    });
}

/// Gives `el` a background that fades from `base` (or nothing) to `tint` while
/// the pointer is over it, instead of snapping. With motion off it is a plain
/// hover style. Uses the element's hover listener: don't add another
/// `on_hover` to it.
pub fn hover_fill(el: Stateful<Div>, base: Option<u32>, tint: u32) -> Stateful<Div> {
    hover_paint(el, base, tint, None)
}

/// [`hover_fill`], with the text colour fading from `text.0` to `text.1` too.
pub fn hover_paint(
    mut el: Stateful<Div>,
    base: Option<u32>,
    tint: u32,
    text: Option<(u32, u32)>,
) -> Stateful<Div> {
    let id = el.interactivity().element_id.clone();
    let Some(id) = id.filter(|_| live()) else {
        return el
            .when_some(base, |d, b| d.bg(c(b)))
            .when_some(text, |d, (t, _)| d.text_color(c(t)))
            .hover(move |s| {
                let s = s.bg(c(tint));
                match text {
                    Some((_, t)) => s.text_color(c(t)),
                    None => s,
                }
            });
    };
    let k = hover_level(&id);
    let bg = match base {
        Some(b) => mix(c(b), c(tint), k),
        None => ca(tint, k),
    };
    el.when(k > 0.0 || base.is_some(), |d| d.bg(bg))
        .when_some(text, |d, (from, to)| d.text_color(mix(c(from), c(to), k)))
        .on_hover(move |over, window, _| {
            set_hover(id.clone(), *over);
            window.refresh();
        })
}

/// `a` turned `k` of the way to `b`.
pub fn mix(a: Hsla, b: Hsla, k: f32) -> Hsla {
    if k <= 0.0 {
        return a;
    }
    if k >= 1.0 {
        return b;
    }
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    let l = |x: f32, y: f32| x + (y - x) * k;
    Rgba {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    #[test]
    fn off_means_end_states() {
        begin(false);
        assert_eq!(ease(Instant::now(), BASE, 0.0), 1.0);
        assert_eq!(shown(key("x"), BASE, 0.0), 1.0);
        assert_eq!(breath(), 1.0);
        assert!(!settling());
        begin(true);
    }

    #[test]
    fn outside_the_main_window_nothing_moves() {
        begin(true);
        end();
        assert_eq!(shown(key("tray"), BASE, 0.0), 1.0);
    }

    #[test]
    fn shown_starts_again_after_a_frame_away() {
        begin(true);
        let k = key(("row", 1u64));
        assert!(shown(k, BASE, 0.0) < 0.5);
        assert!(settling());
        // Drawn again next frame: keeps its start.
        let at = SHOWN.with(|s| s.borrow()[&k].0);
        begin(true);
        shown(k, BASE, 0.0);
        assert_eq!(SHOWN.with(|s| s.borrow()[&k].0), at);
        // Skipped a frame: forgotten, so it eases in again.
        begin(true);
        begin(true);
        assert!(SHOWN.with(|s| !s.borrow().contains_key(&k)));
    }

    #[test]
    fn mix_ends() {
        let a: Hsla = rgb(0x000000).into();
        let b: Hsla = rgb(0xffffff).into();
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        let m = Rgba::from(mix(a, b, 0.5));
        assert!((m.r - 0.5).abs() < 0.01);
    }
}
